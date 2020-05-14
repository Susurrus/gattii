extern crate core;
#[macro_use]
extern crate log;
extern crate serialport;

use core::num;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

pub use self::serialport::prelude::*;

#[derive(Debug)]
pub enum SerialCommand {
    CancelSendFile,
    ChangeBaud(u32),
    ChangeDataBits(DataBits),
    ChangeFlowControl(FlowControl),
    ChangeStopBits(StopBits),
    ChangeParity(Parity),
    ChangePort(String),
    ConnectToPort { name: String, baud: u32 },
    Disconnect,
    SendData(Vec<u8>),
    SendFile(PathBuf),
    LogToFile(PathBuf),
    CancelLogToFile,
}

#[derive(Debug)]
pub enum SerialResponse {
    Data(Vec<u8>),
    SendingFileCanceled,
    SendingFileComplete,
    /// Response to `SerialCommand::SendFile`. Confirms that the file has been opened successfully
    /// and data is going to be sent.
    SendingFileStarted,
    /// Status response indicating what percentage of transmission a file is at
    SendingFileProgress(u8),
    SendingFileError(String),
    OpenPortSuccess(String),
    OpenPortError(String),
    DisconnectSuccess,
    LogToFileError(String),
    LoggingFileCanceled,
    /// A port error has occurred that is likely the result of a serial device disconnected. This
    /// also returns a list of all still-attached serial devices.
    UnexpectedDisconnection(Vec<String>),
    /// A sorted list of ports found during a port scan. Guaranteed to contain the currently-active
    /// port if there is one.
    PortsFound(Vec<String>),
}

#[derive(Debug)]
pub enum GeneralError {
    Parse(num::ParseIntError),
    Send(SerialCommand),
}

pub enum ReadBytes {
    /// Number of bytes read from the file, 0 if none but not an error
    Bytes(usize),
    /// The end of the file has been reached so no data was read
    EndOfFile,
    /// An error was encountered reading from the file
    FileError,
    /// No read was actually attempted
    NoAttempt,
}

pub struct SerialThread {
    pub from_port_chan_rx: Receiver<SerialResponse>,
    pub to_port_chan_tx: Sender<SerialCommand>,
}

pub fn list_ports() -> serialport::Result<Vec<String>> {
    match serialport::available_ports() {
        Ok(ports) => Ok(ports.into_iter().map(|x| x.port_name).collect()),
        Err(e) => Err(e),
    }
}

impl SerialThread {
    pub fn new<F: Fn() + Send + 'static>(callback: F) -> Self {

        let (from_port_chan_tx, from_port_chan_rx) = channel();
        let (to_port_chan_tx, to_port_chan_rx) = channel();

        // Open a thread to monitor the active serial channel. This thread is always-running and
        // listening for various port-related commands, but is not necessarily always connected to
        // the port.
        thread::spawn(move || {
            let mut port: Option<Box<dyn SerialPort>> = None;
            let mut read_file: Option<Box<File>> = None;
            let mut bytes_read = 0u64;
            let mut bytes_total = 0u64;
            let mut last_percentage = 0u8;
            let mut write_file: Option<Box<File>> = None;

            let mut serial_buf: Vec<u8> = vec![0; 1000];
            let mut serial_buf_rx = [0; 1000];
            let mut last_send_time = Instant::now();
            let mut settings: SerialPortSettings = Default::default();

            let loop_time = 10usize; // ms
            let port_scan_time = Duration::from_secs(5);
            let mut last_port_scan_time = Instant::now();

            loop {
                // First check if we have any incoming commands
                match to_port_chan_rx.try_recv() {
                    Ok(SerialCommand::ConnectToPort { name, baud }) => {
                        settings.baud_rate = baud;
                        info!("Connecting to {} at {} with settings {:?}",
                              &name,
                              &baud,
                              &settings);
                        match serialport::open_with_settings(&name, &settings) {
                            Ok(p) => {
                                port = Some(p);
                                from_port_chan_tx
                                    .send(SerialResponse::OpenPortSuccess(name))
                                    .unwrap();
                            }
                            Err(serialport::Error {kind: serialport::ErrorKind::NoDevice, ..}) => {
                                let err_str = format!("Port '{}' is already in use or doesn't \
                                                       exist", &name);
                                let error = SerialResponse::OpenPortError(err_str);
                                from_port_chan_tx.send(error).unwrap();
                            }
                            Err(e) => {
                                from_port_chan_tx.send(SerialResponse::OpenPortError(e.description))
                                    .unwrap();
                            }
                        }
                        callback();
                    }
                    Ok(SerialCommand::ChangeBaud(baud)) => {
                        info!("Changing baud to {}", baud);
                        settings.baud_rate = baud;
                        if let Some(ref mut p) = port {
                            p.set_baud_rate(baud).unwrap();
                        }
                    }
                    Ok(SerialCommand::ChangeDataBits(data_bits)) => {
                        info!("Changing data bits to {:?}", data_bits);
                        settings.data_bits = data_bits;
                        if let Some(ref mut p) = port {
                            p.set_data_bits(data_bits).unwrap();
                        }
                    }
                    Ok(SerialCommand::ChangeFlowControl(flow_control)) => {
                        info!("Changing flow control to {:?}", flow_control);
                        settings.flow_control = flow_control;
                        if let Some(ref mut p) = port {
                            p.set_flow_control(flow_control).unwrap();
                        }
                    }
                    Ok(SerialCommand::ChangeStopBits(stop_bits)) => {
                        info!("Changing stop bits to {:?}", stop_bits);
                        settings.stop_bits = stop_bits;
                        if let Some(ref mut p) = port {
                            p.set_stop_bits(stop_bits).unwrap();
                        }
                    }
                    Ok(SerialCommand::ChangeParity(parity)) => {
                        info!("Changing parity to {:?}", parity);
                        settings.parity = parity;
                        if let Some(ref mut p) = port {
                            p.set_parity(parity).unwrap();
                        }
                    }
                    Ok(SerialCommand::ChangePort(name)) => {
                        if port.is_some() {
                            info!("Changing port to '{}' using settings {:?}",
                                  &name,
                                  &settings);

                            match serialport::open_with_settings(&name, &settings) {
                                Ok(p) => {
                                    port = Some(p);
                                    from_port_chan_tx
                                        .send(SerialResponse::OpenPortSuccess(name))
                                        .unwrap();
                                }
                                Err(_) => {
                                    port = None;
                                    let err_str = format!("Failed to open port '{}'", &name);
                                    let error = SerialResponse::OpenPortError(err_str);
                                    from_port_chan_tx.send(error).unwrap();
                                    callback();
                                }
                            }
                        }
                    }
                    Ok(SerialCommand::Disconnect) => {
                        info!("Disconnecting");
                        port = None;
                        read_file = None;
                        write_file = None;
                        from_port_chan_tx.send(SerialResponse::DisconnectSuccess).unwrap();
                        callback();
                    }
                    Ok(SerialCommand::SendData(d)) => {
                        if let Some(ref mut p) = port {
                            match p.write(d.as_ref()) {
                                Ok(_) => (),
                                Err(e) => error!("Error in SendData: {:?}", e),
                            }
                        }
                    }
                    Ok(SerialCommand::SendFile(f)) => {
                        if port.is_some() {
                            bytes_total = fs::metadata(&f).unwrap().len();
                            last_percentage = 0;
                            bytes_read = 0;
                            info!("Sending file {:?} ({} bytes)", f, bytes_total);
                            match File::open(f) {
                                Ok(file) => read_file = Some(Box::new(file)),
                                Err(e) => error!("{:?}", e),
                            }
                            from_port_chan_tx.send(SerialResponse::SendingFileStarted).unwrap();
                            callback();
                        } else {
                            let err_str = String::from("No open port to send file");
                            let error = SerialResponse::SendingFileError(err_str);
                            from_port_chan_tx.send(error).unwrap();
                            callback();
                        }
                    }
                    Ok(SerialCommand::CancelSendFile) => {
                        read_file = None;
                        from_port_chan_tx.send(SerialResponse::SendingFileCanceled).unwrap();
                        callback();
                    }
                    Ok(SerialCommand::LogToFile(f)) => {
                        if port.is_some() {
                            info!("Logging to file {:?}", f);
                            match File::create(f) {
                                Ok(file) => write_file = Some(Box::new(file)),
                                Err(e) => error!("{:?}", e),
                            }
                        } else {
                            let err_str = String::from("No open port to log file from");
                            let error = SerialResponse::LogToFileError(err_str);
                            from_port_chan_tx.send(error).unwrap();
                            callback();
                        }
                    }
                    Ok(SerialCommand::CancelLogToFile) => {
                        write_file = None;
                        from_port_chan_tx.send(SerialResponse::LoggingFileCanceled).unwrap();
                        callback();
                    }
                    Err(TryRecvError::Empty) |
                    Err(TryRecvError::Disconnected) => (),
                }

                // If a port is active, handle reading and writing to it.
                if let Some(ref mut p) = port {
                    // Read data from the port
                    let rx_data_len = match p.read(serial_buf.as_mut_slice()) {
                        Ok(t) => t,
                        Err(_) => 0,
                    };

                    // And send this data over the channel
                    if rx_data_len > 0 {
                        let send_data = SerialResponse::Data(serial_buf[..rx_data_len].to_vec());
                        from_port_chan_tx.send(send_data).unwrap();
                        callback();

                        // Write the data to a log file if one's set up
                        if let Some(ref mut file) = write_file {
                            match file.write(&serial_buf[..rx_data_len]) {
                                Err(e) => error!("{:?}", e),
                                Ok(l) => {
                                    if l < rx_data_len {
                                        warn!("Only {}/{} bytes logged", l, rx_data_len);
                                    }
                                }
                            }
                        }
                    }

                    // If a file has been opened, read the next 1ms of data from
                    // it as determined by the current baud rate.
                    let mut read_len: ReadBytes = ReadBytes::NoAttempt;
                    let mut byte_as_serial_bits = 1 + 8;
                    if p.parity().unwrap() != Parity::None {
                        byte_as_serial_bits += 1;
                    }
                    if p.stop_bits().unwrap() == StopBits::One {
                        byte_as_serial_bits += 1;
                    } else if p.stop_bits().unwrap() == StopBits::Two {
                        byte_as_serial_bits += 2;
                    }
                    // Write 10ms of data at a time to account for loop time
                    // variation
                    if last_send_time.elapsed().subsec_nanos() > loop_time as u32 * 1_000_000 {
                        let baud: u32 = p.baud_rate().unwrap().into();
                        let tx_data_len = baud as usize / byte_as_serial_bits / (1000 / loop_time);
                        if let Some(ref mut file) = read_file {
                            debug!("Reading {} bytes", tx_data_len);
                            match file.read(&mut serial_buf_rx[..tx_data_len]) {
                                Ok(0) => {
                                    debug!("END OF FILE!");
                                    read_len = ReadBytes::EndOfFile;
                                }
                                Ok(len) => {
                                    bytes_read += len as u64;
                                    debug!("Actually read {} bytes ({} total)", len, bytes_read);
                                    read_len = ReadBytes::Bytes(len);
                                    let percentage =
                                        (bytes_read as f32 / bytes_total as f32 * 100.0) as u8;
                                    if percentage >= last_percentage + 5 {
                                        from_port_chan_tx
                                            .send(SerialResponse::SendingFileProgress(percentage))
                                            .unwrap();
                                        callback();
                                        last_percentage = percentage;
                                    }
                                }
                                Err(e) => {
                                    error!("File error trying to read {} bytes", tx_data_len);
                                    error!("{:?}", e);
                                    read_len = ReadBytes::FileError;
                                }
                            }
                        }
                    }

                    match read_len {
                        ReadBytes::Bytes(x) => {
                            if p.write(&serial_buf_rx[..x]).is_err() {
                                // TODO: Check if port still exists and send different error if so
                                let err_str = format!("Failed to send {} bytes", x);
                                error!("Failed to send {} bytes", x);
                                read_file = None;
                                from_port_chan_tx.send(SerialResponse::SendingFileError(err_str))
                                    .unwrap();
                                callback();
                            }
                            last_send_time = Instant::now();
                        }
                        ReadBytes::EndOfFile | ReadBytes::FileError => {
                            read_file = None;
                            from_port_chan_tx.send(SerialResponse::SendingFileComplete).unwrap();
                            callback();
                        }
                        ReadBytes::NoAttempt => (),
                    }
                }

                // Scan for ports every so often
                if last_port_scan_time.elapsed() > port_scan_time {
                    last_port_scan_time = Instant::now();
                    let mut ports = list_ports().expect("Scanning for ports should never fail");
                    ports.sort();
                    debug!("Found ports: {:?}", &ports);

                    // Check if our port was disconnected
                    let message = {
                        if let Some(ref mut p) = port {
                            if let Some(name) = p.name() {
                                if ports.binary_search(&name).is_err() {
                                    SerialResponse::UnexpectedDisconnection(ports)
                                } else {
                                    SerialResponse::PortsFound(ports)
                                }
                            } else {
                                SerialResponse::PortsFound(ports)
                            }
                        } else {
                            SerialResponse::PortsFound(ports)
                        }
                    };
                    if let SerialResponse::UnexpectedDisconnection(_) = message {
                        port = None;
                    }
                    from_port_chan_tx.send(message).unwrap();
                    callback();
                }

                thread::sleep(Duration::from_millis(loop_time as u64));
            }
        });

        SerialThread {
            from_port_chan_rx: from_port_chan_rx,
            to_port_chan_tx: to_port_chan_tx,
        }
    }

    pub fn send_port_open_cmd(&self,
                              port_name: String,
                              baud_rate: String)
                              -> Result<(), GeneralError> {
        let baud_rate: u32 = baud_rate.parse().map_err(GeneralError::Parse)?;
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::ConnectToPort {
                      name: port_name,
                      baud: baud_rate,
                  })
            .map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_port_close_cmd(&self) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::Disconnect).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_port_change_baud_cmd(&self, baud_rate: String) -> Result<(), GeneralError> {
        let baud_rate: u32 = baud_rate.parse().map_err(GeneralError::Parse)?;
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::ChangeBaud(baud_rate)).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_port_change_data_bits_cmd(&self, data_bits: DataBits) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::ChangeDataBits(data_bits)).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_port_change_flow_control_cmd(&self,
                                             flow_control: FlowControl)
                                             -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::ChangeFlowControl(flow_control))
            .map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_port_change_stop_bits_cmd(&self, stop_bits: StopBits) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::ChangeStopBits(stop_bits)).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_port_change_parity_cmd(&self, parity: Parity) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::ChangeParity(parity)).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_port_change_port_cmd(&self, port_name: String) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::ChangePort(port_name)).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_port_data_cmd(&self, data: &[u8]) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::SendData(data.into())).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_port_file_cmd(&self, path: PathBuf) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::SendFile(path)).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_cancel_file_cmd(&self) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::CancelSendFile).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_log_to_file_cmd(&self, path: PathBuf) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::LogToFile(path)).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }

    pub fn send_cancel_log_to_file_cmd(&self) -> Result<(), GeneralError> {
        let tx = &self.to_port_chan_tx;
        // TODO: Remove in favor of impl From for GeneralError
        tx.send(SerialCommand::CancelLogToFile).map_err(|e| GeneralError::Send(e.0))?;
        Ok(())
    }
}
