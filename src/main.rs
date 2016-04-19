#![crate_type = "bin"]

extern crate argparse;
extern crate gtk;
extern crate serial;

use std::io::prelude::*;
use std::fs::File;
use std::io::BufReader;
use std::process;

use argparse::{ArgumentParser, Store};
use gtk::prelude::*;
use serial::BaudRate;
use serial::prelude::*;

enum ExitCode {
    ArgumentError = 1,
    BadPort,
    ConfigurationError
}

fn main() {
    // Store command-line arguments
    let mut serial_port_name = "".to_string();
    let mut serial_baud = "115200".to_string();

    // Parse command-line arguments
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("A serial terminal.");
        ap.refer(&mut serial_port_name)
            .add_option(&["-p", "--port"], Store, "The serial port name (COM3, /dev/ttyUSB0, etc.)")
            .required();
        ap.refer(&mut serial_baud)
            .add_option(&["-b", "--baud"], Store, "The serial port baud rate (default 115200)");
        ap.parse_args_or_exit();
    }

    // Convert arguments to numbers
    let baud : usize = match serial_baud.parse() {
        Ok(t) => t,
        Err(_) => { println!("Improper serial baud rate specified ({})", serial_baud); process::exit(ExitCode::ArgumentError as i32)}
    };

    // Open the specified serial port
    let mut port = match serial::open(&serial_port_name) {
        Ok(m) => { m }
        Err(e) => { println!("Failed to open {}: {}", serial_port_name, e.to_string()); process::exit(ExitCode::BadPort as i32)}
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
        Err(e) => { println!("Failed to configure {}: {}", serial_port_name, e.to_string()); process::exit(ExitCode::ConfigurationError as i32)}
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
    ports_selector.append(None, "/dev/ttyUSB0");
    ports_selector.set_active(0);
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
    let open_icon = gtk::Image::new_from_icon_name("media-playback-start",
                                                   gtk::IconSize::SmallToolbar as i32);
    let open_button = gtk::ToolButton::new::<gtk::Image>(Some(&open_icon), None);
    open_button.set_is_important(true);
    toolbar.add(&open_button);

    // Set up an auto-scrolling text view
    let text_view = gtk::TextView::new();
    text_view.set_editable(false);
    let scroll = gtk::ScrolledWindow::new(None, None);
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.add(&text_view);

    // Pack everything vertically
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.pack_start(&toolbar, false, true, 0);
    vbox.pack_start(&scroll, true, true, 0);
    window.add(&vbox);

    let window1 = window.clone();
    open_button.connect_clicked(move |_| {
        // TODO move this to a impl?
        let file_chooser = gtk::FileChooserDialog::new(
            Some("Open File"), Some(&window1), gtk::FileChooserAction::Open);
        file_chooser.add_buttons(&[
            ("Open", gtk::ResponseType::Ok as i32),
            ("Cancel", gtk::ResponseType::Cancel as i32),
        ]);
        if file_chooser.run() == gtk::ResponseType::Ok as i32 {
            let filename = file_chooser.get_filename().unwrap();
            let file = File::open(&filename).unwrap();

            let mut reader = BufReader::new(file);
            let mut contents = String::new();
            let _ = reader.read_to_string(&mut contents);

            text_view.get_buffer().unwrap().set_text(&contents);
        }

        file_chooser.destroy();
    });

    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        Inhibit(false)
    });

    window.show_all();
    gtk::main();
}
