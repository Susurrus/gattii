//! # Basic Sample
//!
//! This sample demonstrates how to create a toplevel `window`, set its title, size and position, how to add a `button` to this `window` and how to connect signals with actions.

#![crate_type = "bin"]

extern crate argparse;
extern crate gtk;
extern crate serial;

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

    let window = gtk::Window::new(gtk::WindowType::Toplevel);

    window.set_title("First GTK+ Program");
    window.set_border_width(10);
    window.set_position(gtk::WindowPosition::Center);
    window.set_default_size(350, 70);

    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        Inhibit(false)
    });

    let button = gtk::Button::new_with_label("Click me!");

    button.connect_clicked(|_| {
        println!("Clicked!");
    });
    window.add(&button);

    window.show_all();
    gtk::main();
}
