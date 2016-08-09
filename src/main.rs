#![crate_type = "bin"]

extern crate argparse;
extern crate core;
extern crate gtk;
extern crate glib;
extern crate serial;

use core::num;
use std::boxed::Box;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::io;
use std::path::PathBuf;
use std::process;
use std::string::String;
use std::sync::mpsc;
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use argparse::{ArgumentParser, Store};
use gtk::prelude::*;
use glib::{signal_stop_emission_by_name, signal_handler_block, signal_handler_unblock};
use serial::BaudRate;
use serial::prelude::*;

// make moving clones into closures more convenient
// Taken from: https://github.com/gtk-rs/examples/blob/pending/src/cairo_threads.rs#L17
macro_rules! clone {
    (@param _) => ( _ );
    (@param $x:ident) => ( $x );
    ($($n:ident),+ => move || $body:expr) => (
        {
            $( let $n = $n.clone(); )+
            move || $body
        }
    );
    ($($n:ident),+ => move |$($p:tt),+| $body:expr) => (
        {
            $( let $n = $n.clone(); )+
            move |$(clone!(@param $p),)+| $body
        }
    );
}

#[derive(Debug)]
enum ExitCode {
    ArgumentError = 1,
    BadPort,
    ConfigurationError
}

#[derive(Debug)]
pub struct Error {
    code: ExitCode,
    description: String
}

enum SerialCommand {
    ConnectToPort { name: String, baud: usize },
    ChangeBaud(usize),
    ChangePort(String),
    Disconnect,
    SendData(Vec<u8>),
    SendFile(PathBuf)
}

enum SerialResponse {
    Data(Vec<u8>),
    SendingFileComplete,
    SendingFileError(String)
}

enum GeneralError {
    Io(io::Error),
    Parse(num::ParseIntError),
    Send(mpsc::SendError<SerialCommand>)
}

// declare a new thread local storage key
thread_local!(
    static GLOBAL: RefCell<Option<(gtk::Window, gtk::TextView, gtk::TextBuffer, gtk::ToolButton, Sender<SerialCommand>, Receiver<SerialResponse>, u64)>> = RefCell::new(None)
);

fn send_port_open_cmd(tx: &Sender<SerialCommand>, port_name: String, baud_rate: String) -> Result<(), GeneralError> {
    let baud_rate : usize = try!(baud_rate.parse().map_err(GeneralError::Parse));
    try!(tx.send(SerialCommand::ConnectToPort { name: port_name, baud: baud_rate }).map_err(GeneralError::Send)); // TODO: Remove in favor of impl From for GeneralError
    Ok(())
}

fn send_port_close_cmd(tx: &Sender<SerialCommand>) -> Result<(), GeneralError> {
    try!(tx.send(SerialCommand::Disconnect).map_err(GeneralError::Send)); // TODO: Remove in favor of impl From for GeneralError
    Ok(())
}

fn send_port_change_baud_cmd(tx: &Sender<SerialCommand>, baud_rate: String) -> Result<(), GeneralError> {
    let baud_rate : usize = try!(baud_rate.parse().map_err(GeneralError::Parse));
    try!(tx.send(SerialCommand::ChangeBaud(baud_rate)).map_err(GeneralError::Send)); // TODO: Remove in favor of impl From for GeneralError
    Ok(())
}

fn send_port_change_port_cmd(tx: &Sender<SerialCommand>, port_name: String) -> Result<(), GeneralError> {
    try!(tx.send(SerialCommand::ChangePort(port_name)).map_err(GeneralError::Send)); // TODO: Remove in favor of impl From for GeneralError
    Ok(())
}

fn send_port_data_cmd(tx: &Sender<SerialCommand>, data: Vec<u8>) -> Result<(), GeneralError> {
    try!(tx.send(SerialCommand::SendData(data)).map_err(GeneralError::Send)); // TODO: Remove in favor of impl From for GeneralError
    Ok(())
}

fn send_port_file_cmd(tx: &Sender<SerialCommand>, path: PathBuf) -> Result<(), GeneralError> {
    try!(tx.send(SerialCommand::SendFile(path)).map_err(GeneralError::Send)); // TODO: Remove in favor of impl From for GeneralError
    Ok(())
}

fn open_port(port_name: String, baud_rate: usize) -> serial::Result<Box<serial::SystemPort>> {
    // Open the specified serial port
    let mut port = try!(serial::open(&port_name));

    // Configure the port settings
    try!(port.reconfigure(&|settings| {
        try!(settings.set_baud_rate(BaudRate::from_speed(baud_rate)));
        settings.set_char_size(serial::Bits8);
        settings.set_parity(serial::ParityNone);
        settings.set_stop_bits(serial::Stop1);
        settings.set_flow_control(serial::FlowNone);
        Ok(())
    }));

    Ok(Box::new(port))
}

fn main() {
    // Store command-line arguments
    let mut serial_port_name = "".to_string();
    let mut serial_baud = "".to_string();

    // Parse command-line arguments
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("A serial terminal.");
        ap.refer(&mut serial_port_name)
            .add_option(&["-p", "--port"], Store, "The serial port name (COM3, /dev/ttyUSB0, etc.)");
        ap.refer(&mut serial_baud)
            .add_option(&["-b", "--baud"], Store, "The serial port baud rate (default 115200)");
        ap.parse_args_or_exit();
    }

    if gtk::init().is_err() {
        println!("Failed to initialize GTK.");
        return;
    }

    // Create the main window
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Gattii - Your Serial Terminal Interface");
    window.set_position(gtk::WindowPosition::Center);
    window.set_default_size(400, 300);

    // Create a toolbar with the basic options in it
    let toolbar = gtk::Toolbar::new();
    let ports_selector = gtk::ComboBoxText::new();
    let mut ports_selector_map = HashMap::new();
    if let Ok(ports) = serial::list_ports() {
        let mut i : i32 = 0;
        for p in ports {
            ports_selector.append(None, &p.port_name);
            ports_selector_map.insert(p.port_name, i);
            i += 1;
        }
        ports_selector.set_active(0);
    } else {
        ports_selector.append(None, "No ports found");
        ports_selector.set_active(0);
        ports_selector.set_sensitive(false);
    }
    let ports_selector_container = gtk::ToolItem::new();
    ports_selector_container.add(&ports_selector);

    let mut baud_selector_map = HashMap::new();
    baud_selector_map.insert("921600".to_string(), 0i32);
    baud_selector_map.insert("115200".to_string(), 1i32);
    baud_selector_map.insert("57600".to_string(), 2i32);
    baud_selector_map.insert("38400".to_string(), 3i32);
    baud_selector_map.insert("19200".to_string(), 4i32);
    baud_selector_map.insert("9600".to_string(), 5i32);
    toolbar.add(&ports_selector_container);
    let baud_selector = gtk::ComboBoxText::new();
    baud_selector.append(None, "921600");
    baud_selector.append(None, "115200");
    baud_selector.append(None, "57600");
    baud_selector.append(None, "38400");
    baud_selector.append(None, "19200");
    baud_selector.append(None, "9600");
    baud_selector.set_active(1);
    let baud_selector_container = gtk::ToolItem::new();
    baud_selector_container.add(&baud_selector);
    toolbar.add(&baud_selector_container);

    let open_button = gtk::ToggleToolButton::new();
    open_button.set_icon_name(Some("media-playback-start"));
    open_button.set_is_important(true);
    toolbar.add(&open_button);

    // Set up an auto-scrolling text view
    let text_view = gtk::TextView::new();
    text_view.set_wrap_mode(gtk::WrapMode::Char);
    text_view.set_cursor_visible(false);
    let scroll = gtk::ScrolledWindow::new(None, None);
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.add(&text_view);

    let css_style_provider = gtk::CssProvider::new();
    let style = "GtkTextView { font: Monospace 11 }";
    css_style_provider.load_from_data(style).unwrap();
    let text_view_style_context = text_view.get_style_context().unwrap();
    text_view_style_context.add_provider(&css_style_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

    // Add some actions on the right
    let separator = gtk::SeparatorToolItem::new();
    separator.set_draw(false);
    separator.set_expand(true);
    toolbar.add(&separator);
    let folder_image = gtk::Image::new_from_icon_name("folder", gtk::IconSize::Button.into());
    let send_file_button = gtk::ToolButton::new(Some(&folder_image), None);
    send_file_button.set_icon_name(Some("folder"));
    toolbar.add(&send_file_button);

    // Pack everything vertically
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.pack_start(&toolbar, false, false, 0);
    vbox.pack_start(&scroll, true, true, 0);
    window.add(&vbox);

    // Set up channels for communicating with the port thread.
    let (from_port_chan_tx, from_port_chan_rx) = channel();
    let (to_port_chan_tx, to_port_chan_rx) = channel();
    let buffer = text_view.get_buffer().unwrap();
    GLOBAL.with(move |global| {
        *global.borrow_mut() = Some((window, text_view, buffer, send_file_button, to_port_chan_tx, from_port_chan_rx, 0))
    });

    // Open a thread to monitor the active serial channel. This thread is always-running and listening
    // for various port-related commands, but is not necessarily always connected to the port.
    thread::spawn(move || {
        let mut port : Option<Box<serial::SystemPort>> = None;
        let mut port_settings : serial::PortSettings = serial::PortSettings{
            baud_rate: serial::Baud115200,
            char_size: serial::Bits8,
            parity: serial::ParityNone,
            stop_bits: serial::Stop1,
            flow_control: serial::FlowNone
        };
        let mut read_file : Option<Box<File>> = None;

        // With a 1ms time between serial port reads, this allows up to 921600 baud connections to
        // be saturated and still work.
        let mut serial_buf: Vec<u8> = vec![0; 100];
        let mut serial_buf_rx = [0; 100];
        loop {
            // First check if we have any incoming commands
            match to_port_chan_rx.try_recv() {
                Ok(SerialCommand::ConnectToPort { name, baud }) => {
                    println!("Connecting to {} at {}", name, baud);
                    if let Ok(p) = open_port(name, baud) {
                        port = Some(p);
                    }
                },
                Ok(SerialCommand::ChangeBaud(baud)) => {
                    if let Some(ref mut p) = port {
                        println!("Changing baud to {}", baud);
                        let baud_rate = BaudRate::from_speed(baud);
                        p.reconfigure(&|s| {
                            s.set_baud_rate(baud_rate).unwrap();
                            Ok(())
                        }).unwrap();
                        port_settings.set_baud_rate(baud_rate).unwrap();
                    }
                },
                Ok(SerialCommand::ChangePort(name)) => {
                    println!("Changing port to {}", name);
                    let mut p = Box::new(serial::open(&name).unwrap());
                    p.configure(&port_settings).unwrap();
                    port = Some(p);
                },
                Ok(SerialCommand::Disconnect) => { println!("Disconnecting"); port = None },
                Ok(SerialCommand::SendData(d)) => {
                    if let Some(ref mut p) = port {
                        match p.write(d.as_ref()) {
                            Ok(_) => (),
                            Err(e) => println!("Error in SendData: {:?}", e)
                        }
                    }
                },
                Ok(SerialCommand::SendFile(f)) => {
                    if let Some(_) = port {
                        println!("Sending file {:?}", f);
                        if let Ok(new_file) = File::open(f) {
                            read_file = Some(Box::new(new_file));
                        }
                    } else {
                        from_port_chan_tx.send(SerialResponse::SendingFileError(String::from("No open port to send file!"))).unwrap();
                        glib::idle_add(receive);
                    }
                },
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => ()
            }

            if let Some(ref mut p) = port {
                let rx_data_len = match p.read(serial_buf.as_mut_slice()) {
                    Ok(t) => t,
                    Err(_) => 0
                };
                if rx_data_len > 0 {
                    let send_data = SerialResponse::Data(serial_buf[..rx_data_len].to_vec());
                    from_port_chan_tx.send(send_data).unwrap();
                    glib::idle_add(receive);
                }

                // If a file has been opened, read the next 1ms of data from it as
                // determined by the current baud rate.
                let mut read_len : usize = 0;
                if let Some(ref mut file) = read_file {
                    let mut byte_as_serial_bits = 1 + 8;
                    if port_settings.parity != serial::ParityNone {
                        byte_as_serial_bits += 1;
                    }
                    if port_settings.stop_bits == serial::Stop1 {
                        byte_as_serial_bits += 1;
                    } else if port_settings.stop_bits == serial::Stop2 {
                        byte_as_serial_bits += 2;
                    }
                    let tx_data_len = port_settings.baud_rate.speed() / byte_as_serial_bits / 1000;
                    if let Ok(len) = file.read(&mut serial_buf_rx[..tx_data_len]) {
                        read_len = len;
                    } else {
                        println!("Failed to read {} bytes", tx_data_len);
                    }
                }
                if read_len > 0 {
                    if let Err(_) = p.write(&serial_buf_rx[..read_len]) {
                        println!("Failed to send {} bytes", read_len);
                        read_file = None;
                    }
                } else if read_file.is_some() {
                    read_file = None;
                    from_port_chan_tx.send(SerialResponse::SendingFileComplete).unwrap();
                    glib::idle_add(receive);
                }
            }

            thread::sleep(Duration::from_millis(1));
        }
    });

    baud_selector.connect_changed(move |s| {
        if let Some(baud_rate) = s.get_active_text() {
            GLOBAL.with(|global| {
                if let Some((_, _, _, _, ref tx, _, _)) = *global.borrow() {
                    match send_port_change_baud_cmd(tx, baud_rate.clone()) {
                        Err(GeneralError::Parse(_)) => println!("Invalid baud rate '{}' specified.", &baud_rate),
                        Err(GeneralError::Send(_)) => println!("Error sending port_open command to child thread. Aborting."),
                        Err(_) | Ok(_) => ()
                    }
                }
            });
        }
    });

    ports_selector.connect_changed(move |s| {
        if let Some(port_name) = s.get_active_text() {
            GLOBAL.with(|global| {
                if let Some((_, _, _, _, ref tx, _, _)) = *global.borrow() {
                    match send_port_change_port_cmd(tx, port_name.clone()) {
                        Err(GeneralError::Parse(_)) => println!("Invalid port name '{}' specified.", &port_name),
                        Err(GeneralError::Send(_)) => println!("Error sending port_open command to child thread. Aborting."),
                        Err(_) | Ok(_) => ()
                    }
                }
            });
        }
    });

    open_button.connect_clicked(clone!(ports_selector, baud_selector => move |s| {
        if s.get_active() {
            if let Some(port_name) = ports_selector.get_active_text() {
                if let Some(baud_rate) = baud_selector.get_active_text() {
                    GLOBAL.with(|global| {
                        if let Some((_, _, _, _, ref tx, _, _)) = *global.borrow() {
                            match send_port_open_cmd(tx, port_name, baud_rate.clone()) {
                                Err(GeneralError::Parse(_)) => println!("Invalid baud rate '{}' specified.", &baud_rate),
                                Err(GeneralError::Send(_)) => println!("Error sending port_open command to child thread. Aborting."),
                                Err(_) | Ok(_) => ()
                            }
                        }
                    });
                }
            }
        } else {
            GLOBAL.with(|global| {
                if let Some((_, _, _, _, ref tx, _, _)) = *global.borrow() {
                    match send_port_close_cmd(tx) {
                        Err(GeneralError::Send(_)) => println!("Error sending port_close command to child thread. Aborting."),
                        Err(_) | Ok(_) => ()
                    }
                }
            });
        }
    }));

    GLOBAL.with(|global| {
        if let Some((ref window, _, _, ref b, _, _, _)) = *global.borrow() {
            b.connect_clicked(clone!(window => move |s| {
                let dialog = gtk::FileChooserDialog::new(Some("Send File"), Some(&window), gtk::FileChooserAction::Open);
                dialog.add_buttons(&[
                    ("Send", gtk::ResponseType::Ok.into()),
                    ("Cancel", gtk::ResponseType::Cancel.into()),
                ]);
                let result = dialog.run();
                if result == gtk::ResponseType::Ok.into() {
                    let filename = dialog.get_filename().unwrap();
                    GLOBAL.with(|global| {
                        if let Some((_, _, _, _, ref tx, _, _)) = *global.borrow() {
                            match send_port_file_cmd(tx, filename) {
                                Err(GeneralError::Send(_)) => println!("Error sending port_file command to child thread. Aborting."),
                                Err(_) => (),
                                Ok(_) => {
                                    s.set_sensitive(false);
                                }
                            }
                        }
                    });
                }

                dialog.destroy();
            }));
        }
    });

    GLOBAL.with(|global| {
        if let Some((_, _, ref b, _, _, _, ref mut s)) = *global.borrow_mut() {
            *s = b.connect_insert_text(|b, _, text| {
                GLOBAL.with(|global| {
                    if let Some((_, _, _, _, ref tx, _, _)) = *global.borrow() {
                        let v = Vec::from(text);
                        match send_port_data_cmd(tx, v) {
                            Err(GeneralError::Send(_)) => println!("Error sending data command to child thread. Aborting."),
                            Err(_) | Ok(_) => ()
                        }
                    }
                });
                signal_stop_emission_by_name(b, "insert-text");
            });
        }
    });

    // Disable deletion of characters within the textview
    GLOBAL.with(|global| {
        if let Some((_, _, ref b, _, _, _, _)) = *global.borrow() {
            b.connect_delete_range(move |b, _, _| {
                signal_stop_emission_by_name(b, "delete-range");
            });
        }
    });

    // Process any command line arguments that were passed
    if !serial_port_name.is_empty() && !serial_baud.is_empty() {
        if let Some(ports_selector_index) = ports_selector_map.get(&serial_port_name) {
            ports_selector.set_active(*ports_selector_index);
        } else {
            println!("ERROR: Invalid port name '{}' specified.", serial_port_name);
            process::exit(ExitCode::ArgumentError as i32);
        }
        if let Some(baud_selector_index) = baud_selector_map.get(&serial_baud) {
            baud_selector.set_active(*baud_selector_index);
        } else {
            println!("ERROR: Invalid baud rate '{}' specified.", serial_baud);
            process::exit(ExitCode::ArgumentError as i32);
        }
        open_button.set_active(true);
    } else if !serial_port_name.is_empty() {
        println!("ERROR: A baud rate must be specified.");
        process::exit(ExitCode::ArgumentError as i32);
    } else if !serial_baud.is_empty() {
        println!("ERROR: A port name must be specified.");
        process::exit(ExitCode::ArgumentError as i32);
    }

    GLOBAL.with(|global| {
        if let Some((ref window, _, _, _, _, _, _)) = *global.borrow() {
            window.connect_delete_event(|_, _| {
                gtk::main_quit();
                Inhibit(false)
            });

            window.show_all();
        }
    });

    gtk::main();
}

fn receive() -> glib::Continue {
    GLOBAL.with(|global| {
        if let Some((ref window, ref view, ref buf, ref f_button, _, ref rx, s)) = *global.borrow() {
            match rx.try_recv() {
                Ok(SerialResponse::Data(data)) => {

                    // Don't know why this needs to be this complicated, but found
                    // the answer on the gtk+ forums:
                    // http://www.gtkforums.com/viewtopic.php?t=1307

                    // Get the position of the special "insert" mark
                    let mark = buf.get_insert().unwrap();
                    let mut iter = buf.get_iter_at_mark(&mark);

                    // Inserts buffer at the end
                    signal_handler_block(buf, s);
                    buf.insert(&mut iter, &String::from_utf8_lossy(&data));
                    signal_handler_unblock(buf, s);

                    // Scroll to the "insert" mark
                    view.scroll_mark_onscreen(&mark);
                },
                Ok(SerialResponse::SendingFileComplete) => f_button.set_sensitive(true),
                Ok(SerialResponse::SendingFileError(s)) => {
                    println!("{}", s);
                    let dialog = gtk::MessageDialog::new(Some(window),
                        gtk::DIALOG_DESTROY_WITH_PARENT,
                        gtk::MessageType::Error,
                        gtk::ButtonsType::Ok,
                        "Error sending file");
                    dialog.run();
                    dialog.destroy();
                    f_button.set_sensitive(true);
                },
                Err(_) => ()
            }
        }
    });
    glib::Continue(false)
}
