#![crate_type = "bin"]

extern crate argparse;
extern crate core;
extern crate gtk;
extern crate glib;

use std::cell::RefCell;
use std::collections::HashMap;
use std::process;
use std::string::String;

use argparse::{ArgumentParser, Store};
use gtk::prelude::*;
use glib::{signal_stop_emission_by_name, signal_handler_block, signal_handler_unblock};

mod serial_thread;
use serial_thread::*;

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
    ConfigurationError,
}

#[derive(Debug)]
pub struct Error {
    code: ExitCode,
    description: String,
}

struct Ui {
    window: gtk::Window,
    text_view: gtk::TextView,
    text_buffer: gtk::TextBuffer,
    open_button: gtk::ToggleToolButton,
    file_button: gtk::ToggleToolButton,
    text_view_insert_signal: u64,
}

// declare a new thread local storage key
thread_local!(
    static GLOBAL: RefCell<Option<(Ui, SerialThread)>> = RefCell::new(None)
);

fn main() {
    // Store command-line arguments
    let mut serial_port_name = "".to_string();
    let mut serial_baud = "".to_string();

    // Parse command-line arguments
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("A serial terminal.");
        ap.refer(&mut serial_port_name)
            .add_option(&["-p", "--port"],
                        Store,
                        "The serial port name (COM3, /dev/ttyUSB0, etc.)");
        ap.refer(&mut serial_baud)
            .add_option(&["-b", "--baud"],
                        Store,
                        "The serial port baud rate (default 115200)");
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
    if let Ok(ports) = serial_thread::list_ports() {
        if ports.len() > 0 {
            let mut i: i32 = 0;
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
    text_view_style_context.add_provider(&css_style_provider,
                                         gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

    // Add send file button
    let separator = gtk::SeparatorToolItem::new();
    separator.set_draw(false);
    separator.set_expand(true);
    toolbar.add(&separator);
    let send_file_button = gtk::ToggleToolButton::new();
    send_file_button.set_icon_name(Some("folder"));
    send_file_button.set_sensitive(false);
    toolbar.add(&send_file_button);

    // Pack everything vertically
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.pack_start(&toolbar, false, false, 0);
    vbox.pack_start(&scroll, true, true, 0);
    window.add(&vbox);

    // Set up channels for communicating with the port thread.
    let buffer = text_view.get_buffer().unwrap();
    let ui = Ui {
        window: window.clone(),
        text_view: text_view.clone(),
        text_buffer: buffer.clone(),
        open_button: open_button.clone(),
        file_button: send_file_button.clone(),
        text_view_insert_signal: 0,
    };
    GLOBAL.with(move |global| {
        *global.borrow_mut() = Some((ui,
                                     SerialThread::new(|| {
            glib::idle_add(receive);
        })))
    });

    baud_selector.connect_changed(move |s| {
        if let Some(baud_rate) = s.get_active_text() {
            GLOBAL.with(|global| {
                if let Some((_, ref serial_thread)) = *global.borrow() {
                    match serial_thread.send_port_change_baud_cmd(baud_rate.clone()) {
                        Err(GeneralError::Parse(_)) => {
                            println!("Invalid baud rate '{}' specified.", &baud_rate)
                        }
                        Err(GeneralError::Send(_)) => {
                            println!("Error sending port_open command to child thread. Aborting.")
                        }
                        Err(_) | Ok(_) => (),
                    }
                }
            });
        }
    });

    ports_selector.connect_changed(move |s| {
        if let Some(port_name) = s.get_active_text() {
            GLOBAL.with(|global| {
                if let Some((_, ref serial_thread)) = *global.borrow() {
                    match serial_thread.send_port_change_port_cmd(port_name.clone()) {
                        Err(GeneralError::Parse(_)) => {
                            println!("Invalid port name '{}' specified.", &port_name)
                        }
                        Err(GeneralError::Send(_)) => {
                            println!("Error sending port_open command to child thread. Aborting.")
                        }
                        Err(_) | Ok(_) => (),
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
                        if let Some((_, ref serial_thread)) = *global.borrow() {
                            match serial_thread.send_port_open_cmd(port_name, baud_rate.clone()) {
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
                if let Some((_, ref serial_thread)) = *global.borrow() {
                    match serial_thread.send_port_close_cmd() {
                        Err(GeneralError::Send(_)) => println!("Error sending port_close command to child thread. Aborting."),
                        Err(_) | Ok(_) => ()
                    }
                }
            });
        }
    }));

    GLOBAL.with(|global| {
        if let Some((ref ui, _)) = *global.borrow() {
            ui.file_button.connect_toggled(|s| {
                GLOBAL.with(|global| {
                    if let Some((ref ui, ref serial_thread)) = *global.borrow() {
                        let window = &ui.window;
                        let view = &ui.text_view;
                        if s.get_active() {
                            let dialog = gtk::FileChooserDialog::new(Some("Send File"), Some(window), gtk::FileChooserAction::Open);
                            dialog.add_buttons(&[
                                ("Send", gtk::ResponseType::Ok.into()),
                                ("Cancel", gtk::ResponseType::Cancel.into()),
                            ]);
                            let result = dialog.run();
                            if result == gtk::ResponseType::Ok.into() {
                                let filename = dialog.get_filename().unwrap();
                                GLOBAL.with(|global| {
                                    if let Some((_, ref serial_thread)) = *global.borrow() {
                                        match serial_thread.send_port_file_cmd(filename) {
                                            Err(_) => {
                                                println!("Error sending port_file command to child thread. Aborting.");
                                                s.set_sensitive(true);
                                                s.set_active(false);
                                            },
                                            Ok(_) => view.set_editable(false)
                                        }
                                    }
                                });
                            }

                            dialog.destroy();
                        } else {
                            match serial_thread.send_cancel_file_cmd() {
                                Err(GeneralError::Send(_)) => {
                                    println!("Error sending cancel_file command to child thread. Aborting.");
                                },
                                Err(_) | Ok(_) => ()
                            }
                        }
                    }
                });
            });
        }
    });

    GLOBAL.with(|global| {
        if let Some((ref mut ui, _)) = *global.borrow_mut() {
            let b = &ui.text_buffer;
            ui.text_view_insert_signal = b.connect_insert_text(|b, _, text| {
                GLOBAL.with(|global| {
                    if let Some((_, ref serial_thread)) = *global.borrow() {
                        let v = Vec::from(text);
                        match serial_thread.send_port_data_cmd(v) {
                            Err(GeneralError::Send(_)) => {
                                println!("Error sending data command to child thread. Aborting.")
                            }
                            Err(_) | Ok(_) => (),
                        }
                    }
                });
                signal_stop_emission_by_name(b, "insert-text");
            });
        }
    });

    // Disable deletion of characters within the textview
    GLOBAL.with(|global| {
        if let Some((ref ui, _)) = *global.borrow() {
            let b = &ui.text_buffer;
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
        if let Some((ref ui, _)) = *global.borrow() {
            let window = &ui.window;
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
        if let Some((ref ui, ref serial_thread)) = *global.borrow() {
            let window = &ui.window;
            let view = &ui.text_view;
            let buf = &ui.text_buffer;
            let f_button = &ui.file_button;
            match serial_thread.from_port_chan_rx.try_recv() {
                Ok(SerialResponse::Data(data)) => {

                    // Don't know why this needs to be this complicated, but found
                    // the answer on the gtk+ forums:
                    // http://www.gtkforums.com/viewtopic.php?t=1307

                    // Get the position of the special "insert" mark
                    let mark = buf.get_insert().unwrap();
                    let mut iter = buf.get_iter_at_mark(&mark);

                    // Inserts buffer at the end
                    signal_handler_block(buf, ui.text_view_insert_signal);
                    buf.insert(&mut iter, &String::from_utf8_lossy(&data));
                    signal_handler_unblock(buf, ui.text_view_insert_signal);

                    // Scroll to the "insert" mark
                    view.scroll_mark_onscreen(&mark);
                }
                Ok(SerialResponse::DisconnectSuccess) => {
                    f_button.set_sensitive(false);
                    f_button.set_active(false);
                }
                Ok(SerialResponse::OpenPortSuccess) => {
                    f_button.set_sensitive(true);
                }
                Ok(SerialResponse::OpenPortError(s)) => {
                    println!("OpenPortError: {}", s);
                    let dialog = gtk::MessageDialog::new(Some(window),
                                                         gtk::DIALOG_DESTROY_WITH_PARENT,
                                                         gtk::MessageType::Error,
                                                         gtk::ButtonsType::Ok,
                                                         "Error opening port");
                    dialog.run();
                    dialog.destroy();
                    f_button.set_sensitive(false);
                }
                Ok(SerialResponse::SendingFileComplete) |
                Ok(SerialResponse::SendingFileCanceled) => {
                    println!("Sending file complete");
                    f_button.set_active(false);
                    view.set_editable(true);
                }
                Ok(SerialResponse::SendingFileError(s)) => {
                    f_button.set_active(false);
                    view.set_editable(true);
                    let dialog = gtk::MessageDialog::new(Some(window),
                                                         gtk::DIALOG_DESTROY_WITH_PARENT,
                                                         gtk::MessageType::Error,
                                                         gtk::ButtonsType::Ok,
                                                         "Error sending file");
                    dialog.run();
                    dialog.destroy();
                }
                Err(_) => (),
            }
        }
    });
    glib::Continue(false)
}
