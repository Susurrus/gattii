#![crate_type = "bin"]

extern crate argparse;
extern crate gtk;
extern crate glib;
extern crate serial;

use std::cell::RefCell;
use std::io::prelude::*;
use std::process;
use std::string::String;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::time::Duration;

use argparse::{ArgumentParser, Store};
use gtk::prelude::*;
use serial::BaudRate;
use serial::prelude::*;

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

// declare a new thread local storage key
thread_local!(
    static GLOBAL: RefCell<Option<(gtk::TextBuffer, Receiver<Vec<u8>>)>> = RefCell::new(None)
);

fn open_port(tx: Sender<Vec<u8>>, port_name: String, baud_rate: String) {
    thread::spawn(move || {

        // Open the specified serial port
        let mut port = match serial::open(&port_name) {
            Ok(p) => { p }
            Err(e) => { println!("Failed to open {}: {}", port_name, e.to_string()); process::exit(ExitCode::BadPort as i32)}
        };

        // Parse the baud rate setting
        let baud : usize = match baud_rate.parse() {
            Ok(b) => b,
            Err(_) => {
                println!("Failed to parse baud rate, please specify a valid integer ({} was specified)",
                port_name); process::exit(ExitCode::ConfigurationError as i32)
            }
        };

        // Configure the port settings
        match port.reconfigure(&|settings| {
            try!(settings.set_baud_rate(BaudRate::from_speed(baud)));
            settings.set_char_size(serial::Bits8);
            settings.set_parity(serial::ParityNone);
            settings.set_stop_bits(serial::Stop1);
            settings.set_flow_control(serial::FlowNone);
            Ok(())
        }) {
            Ok(_) => (),
            Err(e) => { println!("Failed to configure {}: {}", port_name, e.to_string()); process::exit(ExitCode::ConfigurationError as i32)}
        }

        // With a 1ms time between serial port reads, this allows up to 921600 baud connections to
        // be saturated and still work.
        let mut serial_buf: Vec<u8> = vec![0; 100];
        loop {
            let rx_data_len = match port.read(serial_buf.as_mut_slice()) {
                Ok(t) => t,
                Err(_) => 0
            };
            if rx_data_len > 0 {
                tx.send(serial_buf[..rx_data_len].to_vec()).unwrap();
                glib::idle_add(receive);
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
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
    if let Ok(ports) = serial::list_ports() {
        for p in ports {
            ports_selector.append(None, &p.port_name);
        }
        ports_selector.set_active(0);
    } else {
        ports_selector.append(None, "No ports found");
        ports_selector.set_active(0);
        ports_selector.set_sensitive(false);
    }
    let ports_selector_container = gtk::ToolItem::new();
    ports_selector_container.add(&ports_selector);
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
    text_view.set_editable(false);
    let scroll = gtk::ScrolledWindow::new(None, None);
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.add(&text_view);

    let css_style_provider = gtk::CssProvider::new();
    let style = "GtkTextView { font: Monospace 11 }";
    css_style_provider.load_from_data(style).unwrap();
    let text_view_style_context = text_view.get_style_context().unwrap();
    text_view_style_context.add_provider(&css_style_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

    // Pack everything vertically
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.pack_start(&toolbar, false, true, 0);
    vbox.pack_start(&scroll, true, true, 0);
    window.add(&vbox);

    let (tx, rx) = channel();
    // put TextBuffer and receiver in thread local storage
    GLOBAL.with(move |global| {
        *global.borrow_mut() = Some((text_view.get_buffer().unwrap(), rx))
    });

    // Process any command line arguments that were passed
    let port: serial::SystemPort;
    if serial_port_name.len() > 0 && serial_baud.len() > 0 {
        open_port(tx, serial_port_name, serial_baud);
    } else if serial_port_name.len() > 0 {
        println!("A baud rate must be specified as well.");
        process::exit(ExitCode::ArgumentError as i32);
    } else if serial_baud.len() > 0 {
        println!("A port name must be specified as well.");
        process::exit(ExitCode::ArgumentError as i32);
    }

    open_button.connect_clicked(move |s| {
        if s.get_active() {
            if let Some(port_name) = ports_selector.get_active_text() {
                if let Some(baud_rate) = baud_selector.get_active_text() {
                    open_port(tx, port_name, baud_rate);
                }
            }
        }
    });

    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        Inhibit(false)
    });

    window.show_all();
    gtk::main();
}

fn receive() -> glib::Continue {
    GLOBAL.with(|global| {
        if let Some((ref buf, ref rx)) = *global.borrow() {
            if let Ok(text) = rx.try_recv() {
                let mut end = buf.get_end_iter();
                buf.insert(&mut end, &String::from_utf8_lossy(&text));
            }
        }
    });
    glib::Continue(false)
}
