#![crate_type = "bin"]

extern crate argparse;
extern crate core;
extern crate env_logger;
#[macro_use] extern crate log;
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
    file_button: gtk::ToggleButton,
    open_button: gtk::ToggleButton,
    save_button: gtk::ToggleButton,
    data_bits_scale: gtk::Scale,
    stop_bits_scale: gtk::Scale,
    parity_dropdown: gtk::ComboBoxText,
    flow_control_dropdown: gtk::ComboBoxText,
    text_view_insert_signal: u64,
    text_buffer_delete_signal: u64,
    open_button_clicked_signal: u64,
    file_button_toggled_signal: u64,
    save_button_toggled_signal: u64,
}

struct State {
    connected: bool,
}

// declare a new thread local storage key
thread_local!(
    static GLOBAL: RefCell<Option<(Ui, SerialThread, State)>> = RefCell::new(None)
);

fn main() {
    // Initialize logging
    env_logger::init().expect("Failed to initialize logging");

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
        error!("Failed to initialize GTK.");
        return;
    }

    // Create the main window
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Gattii - Your Serial Terminal Interface");
    window.set_position(gtk::WindowPosition::Center);
    window.set_default_size(400, 300);

    // Create the top toolbar
    let toolbar = gtk::Toolbar::new();
    toolbar.set_show_arrow(false);

    // Add a port selector
    let ports_selector = gtk::ComboBoxText::new();
    let mut ports_selector_map = HashMap::new();
    if let Ok(mut ports) = serial_thread::list_ports() {
        ports.sort();
        if !ports.is_empty() {
            for (i, p) in ports.into_iter().enumerate() {
                ports_selector.append(None, &p);
                ports_selector_map.insert(p, i);
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

    // Add a baud rate selector
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
    baud_selector.set_active(5);
    let baud_selector_container = gtk::ToolItem::new();
    baud_selector_container.add(&baud_selector);
    toolbar.add(&baud_selector_container);

    // Add the port settings button
    let port_settings_button = gtk::MenuButton::new();
    port_settings_button.set_direction(gtk::ArrowType::None);
    let port_settings_popover = gtk::Popover::new(Some(&port_settings_button));
    port_settings_popover.set_position(gtk::PositionType::Bottom);
    port_settings_popover.set_constrain_to(gtk::PopoverConstraint::None);
    port_settings_button.set_popover(Some(&port_settings_popover));
    let popover_container = gtk::Grid::new();
    popover_container.set_margin_top(10);
    popover_container.set_margin_right(10);
    popover_container.set_margin_bottom(10);
    popover_container.set_margin_left(10);
    popover_container.set_row_spacing(10);
    popover_container.set_column_spacing(10);
    let data_bits_label = gtk::Label::new("Data bits:");
    data_bits_label.set_halign(gtk::Align::End);
    popover_container.attach(&data_bits_label, 0, 0, 1, 1);
    let data_bits_scale = gtk::Scale::new_with_range(gtk::Orientation::Horizontal, 5.0, 8.0, 1.0);
    data_bits_scale.set_draw_value(false);
    // FIXME: Remove the following line of code once GTK+ bug 358970 is released
    data_bits_scale.set_round_digits(0);
    data_bits_scale.set_value(8.0);
    data_bits_scale.add_mark(5.0, gtk::PositionType::Bottom, "5");
    data_bits_scale.add_mark(6.0, gtk::PositionType::Bottom, "6");
    data_bits_scale.add_mark(7.0, gtk::PositionType::Bottom, "7");
    data_bits_scale.add_mark(8.0, gtk::PositionType::Bottom, "8");
    popover_container.attach(&data_bits_scale, 1, 0, 1, 1);
    let stop_bits_label = gtk::Label::new("Stop bits:");
    stop_bits_label.set_halign(gtk::Align::End);
    popover_container.attach(&stop_bits_label, 0, 1, 1, 1);
    let stop_bits_scale = gtk::Scale::new_with_range(gtk::Orientation::Horizontal, 1.0, 2.0, 1.0);
    stop_bits_scale.set_draw_value(false);
    // FIXME: Remove the following line of code once GTK+ bug 358970 is released
    stop_bits_scale.set_round_digits(0);
    stop_bits_scale.add_mark(1.0, gtk::PositionType::Bottom, "1");
    stop_bits_scale.add_mark(2.0, gtk::PositionType::Bottom, "2");
    popover_container.attach(&stop_bits_scale, 1, 1, 1, 1);
    let parity_label = gtk::Label::new("Parity:");
    parity_label.set_halign(gtk::Align::End);
    popover_container.attach(&parity_label, 0, 2, 1, 1);
    let parity_dropdown = gtk::ComboBoxText::new();
    parity_dropdown.append(None, "None");
    parity_dropdown.append(None, "Odd");
    parity_dropdown.append(None, "Even");
    parity_dropdown.set_active(0);
    popover_container.attach(&parity_dropdown, 1, 2, 1, 1);
    let flow_control_label = gtk::Label::new("Flow control:");
    flow_control_label.set_halign(gtk::Align::End);
    popover_container.attach(&flow_control_label, 0, 3, 1, 1);
    let flow_control_dropdown = gtk::ComboBoxText::new();
    flow_control_dropdown.append(None, "None");
    flow_control_dropdown.append(None, "Hardware");
    flow_control_dropdown.append(None, "Software");
    flow_control_dropdown.set_active(0);
    popover_container.attach(&flow_control_dropdown, 1, 3, 1, 1);
    popover_container.show_all();
    port_settings_popover.add(&popover_container);
    let port_settings_button_container = gtk::ToolItem::new();
    port_settings_button_container.add(&port_settings_button);
    toolbar.add(&port_settings_button_container);

    // Add the open button
    let open_button_container = gtk::ToolItem::new();
    let open_button = gtk::ToggleButton::new_with_label("Open");
    open_button_container.add(&open_button);
    toolbar.add(&open_button_container);

    // Set up an auto-scrolling text view
    let text_view = gtk::TextView::new();
    text_view.set_wrap_mode(gtk::WrapMode::Char);
    text_view.set_cursor_visible(false);
    let scroll = gtk::ScrolledWindow::new(None, None);
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.add(&text_view);

    let css_style_provider = gtk::CssProvider::new();
    let style = "GtkTextView { font: 11pt monospace }";
    css_style_provider.load_from_data(style).unwrap();
    let text_view_style_context = text_view.get_style_context().unwrap();
    text_view_style_context.add_provider(&css_style_provider,
                                         gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

    // Add send file button
    let separator = gtk::SeparatorToolItem::new();
    separator.set_draw(false);
    separator.set_expand(true);
    toolbar.add(&separator);
    let send_file_button = gtk::ToggleButton::new();
    send_file_button.set_tooltip_text("Transmit file");
    // FIXME: Use gtk::IconSize::SmallToolbar once https://github.com/gtk-rs/gtk/issues/439
    // is resolved
    let send_file_image = gtk::Image::new_from_icon_name("folder", 2);
    send_file_button.set_image(&send_file_image);
    send_file_button.set_sensitive(false);
    let send_file_button_container = gtk::ToolItem::new();
    send_file_button_container.add(&send_file_button);
    toolbar.add(&send_file_button_container);

    // Add save file button
    let save_file_button = gtk::ToggleButton::new();
    save_file_button.set_tooltip_text("Record to file");
    // FIXME: Use gtk::IconSize::SmallToolbar once https://github.com/gtk-rs/gtk/issues/439
    // is resolved
    let save_file_image = gtk::Image::new_from_icon_name("folder", 2);
    save_file_button.set_image(&save_file_image);
    save_file_button.set_sensitive(false);
    let save_file_button_container = gtk::ToolItem::new();
    save_file_button_container.add(&save_file_button);
    toolbar.add(&save_file_button_container);

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
        file_button: send_file_button.clone(),
        open_button: open_button.clone(),
        save_button: save_file_button.clone(),
        data_bits_scale: data_bits_scale.clone(),
        stop_bits_scale: stop_bits_scale.clone(),
        parity_dropdown: parity_dropdown.clone(),
        flow_control_dropdown: flow_control_dropdown.clone(),
        text_view_insert_signal: 0,
        text_buffer_delete_signal: 0,
        open_button_clicked_signal: 0,
        file_button_toggled_signal: 0,
        save_button_toggled_signal: 0,
    };
    let state = State {
        connected: false
    };
    GLOBAL.with(move |global| {
        *global.borrow_mut() = Some((ui,
                                     SerialThread::new(|| {
                                         glib::idle_add(receive);
                                     }),
                                     state));
    });

    baud_selector.connect_changed(move |s| {
        if let Some(baud_rate) = s.get_active_text() {
            GLOBAL.with(|global| {
                if let Some((_, ref serial_thread, _)) = *global.borrow() {
                    match serial_thread.send_port_change_baud_cmd(baud_rate.clone()) {
                        Err(GeneralError::Parse(_)) => {
                            error!("Invalid baud rate '{}' specified.", &baud_rate)
                        }
                        Err(GeneralError::Send(_)) => {
                            error!("Error sending port_open command to child \
                                      thread. Aborting.")
                        }
                        Ok(_) => (),
                    }
                }
            });
        }
    });

    ports_selector.connect_changed(move |s| {
        if let Some(port_name) = s.get_active_text() {
            GLOBAL.with(|global| {
                if let Some((_, ref serial_thread, _)) = *global.borrow() {
                    match serial_thread.send_port_change_port_cmd(port_name.clone()) {
                        Err(GeneralError::Parse(_)) => {
                            error!("Invalid port name '{}' specified.", &port_name)
                        }
                        Err(GeneralError::Send(_)) => {
                            error!("Error sending change_port command to child \
                                      thread. Aborting.")
                        }
                        Ok(_) => (),
                    }
                }
            });
        }
    });

    let open_button_clicked_signal = open_button.connect_clicked(clone!(ports_selector, baud_selector => move |s| {
        if s.get_active() {
            if let Some(port_name) = ports_selector.get_active_text() {
                if let Some(baud_rate) = baud_selector.get_active_text() {
                    GLOBAL.with(|global| {
                        if let Some((_, ref serial_thread, _)) = *global.borrow() {
                            match serial_thread.send_port_open_cmd(port_name, baud_rate.clone()) {
                                Err(GeneralError::Parse(_)) =>
                                    error!("Invalid baud rate '{}' specified.", &baud_rate),
                                Err(GeneralError::Send(_)) =>
                                    error!("Error sending port_open command to \
                                              child thread. Aborting."),
                                Ok(_) => ()
                            }
                        }
                    });
                }
            }
        } else {
            GLOBAL.with(|global| {
                if let Some((_, ref serial_thread, _)) = *global.borrow() {
                    match serial_thread.send_port_close_cmd() {
                        Err(GeneralError::Send(_)) => error!("Error sending port_close command to child thread. Aborting."),
                        Err(_) | Ok(_) => ()
                    }
                }
            });
        }
    }));

    GLOBAL.with(|global| {
        if let Some((ref mut ui, _, _)) = *global.borrow_mut() {
            // Connect send file selector button to callback. This is left as a
            // separate function to reduce rightward drift.
            ui.file_button_toggled_signal = ui.file_button.connect_toggled(file_button_connect_toggled);

            // Connect log file selector button to callback. This is left as a
            // separate function to reduce rightward drift.
            ui.save_button_toggled_signal = ui.save_button.connect_toggled(save_button_connect_toggled);

            // Configure the data bits callback
            ui.data_bits_scale.connect_value_changed(|s| {
                let data_bits = match s.get_value() {
                    5.0 => DataBits::Five,
                    6.0 => DataBits::Six,
                    7.0 => DataBits::Seven,
                    8.0 => DataBits::Eight,
                    _ => unreachable!(),
                };
                GLOBAL.with(|global| {
                    if let Some((_, ref serial_thread, _)) = *global.borrow() {
                        match serial_thread.send_port_change_data_bits_cmd(data_bits) {
                            Err(GeneralError::Parse(_)) => {
                                unreachable!();
                            }
                            Err(GeneralError::Send(_)) => {
                                error!("Error sending data bits change command \
                                          to child thread. Aborting.")
                            }
                            Ok(_) => (),
                        }
                    }
                });
            });

            // Configure the data bits callback
            ui.stop_bits_scale.connect_value_changed(|s| {
                let stop_bits = match s.get_value() {
                    1.0 => StopBits::One,
                    2.0 => StopBits::Two,
                    _ => unreachable!(),
                };
                GLOBAL.with(|global| {
                    if let Some((_, ref serial_thread, _)) = *global.borrow() {
                        match serial_thread.send_port_change_stop_bits_cmd(stop_bits) {
                            Err(GeneralError::Parse(_)) => {
                                unreachable!();
                            }
                            Err(GeneralError::Send(_)) => {
                                error!("Error sending stop bits change command \
                                          to child thread. Aborting.")
                            }
                            Ok(_) => (),
                        }
                    }
                });
            });

            // Configure the parity dropdown callback
            ui.parity_dropdown.connect_changed(|s| {
                let parity = match s.get_active_text() {
                    Some(ref x) if x == "None" => Some(Parity::None),
                    Some(ref x) if x == "Odd" => Some(Parity::Odd),
                    Some(ref x) if x == "Even" => Some(Parity::Even),
                    Some(_) | None => unreachable!(),
                };
                if let Some(parity) = parity {
                    GLOBAL.with(|global| {
                        if let Some((_, ref serial_thread,  _)) = *global.borrow() {
                            match serial_thread.send_port_change_parity_cmd(parity) {
                                Err(GeneralError::Parse(_)) => {
                                    unreachable!();
                                }
                                Err(GeneralError::Send(_)) => {
                                    error!("Error sending parity change command \
                                              to child thread. Aborting.")
                                }
                                Ok(_) => (),
                            }
                        }
                    });
                }
            });

            // Configure the data bits callback
            ui.flow_control_dropdown.connect_changed(|s| {
                let flow_control = match s.get_active_text() {
                    Some(ref x) if x == "None" => Some(FlowControl::None),
                    Some(ref x) if x == "Software" => Some(FlowControl::Software),
                    Some(ref x) if x == "Hardware" => Some(FlowControl::Hardware),
                    Some(_) | None => unreachable!(),
                };
                if let Some(flow_control) = flow_control {
                    GLOBAL.with(|global| {
                        if let Some((_, ref serial_thread, _)) = *global.borrow() {
                            match serial_thread.send_port_change_flow_control_cmd(flow_control) {
                                Err(GeneralError::Parse(_)) => {
                                    unreachable!();
                                }
                                Err(GeneralError::Send(_)) => {
                                    error!("Error sending flow control change \
                                              command to child thread. Aborting.")
                                }
                                Ok(_) => (),
                            }
                        }
                    });
                }
            });

            // Configure the right-click menu for the text view widget
            ui.text_view.connect_populate_popup(|t, p| {
                if let Ok(popup) = p.clone().downcast::<gtk::Menu>() {

                    // Remove the "delete" menu option as it doesn't even work
                    // because the "delete-range" signal is disabled.
                    for c in popup.get_children() {
                        // Workaround for Bug 778162:
                        // https://bugzilla.gnome.org/show_bug.cgi?id=778162
                        if c.is::<gtk::SeparatorMenuItem>() {
                            continue;
                        }
                        if let Ok(child) = c.clone().downcast::<gtk::MenuItem>() {
                            if let Some(l) = child.get_label() {
                                if l == "_Delete" {
                                    popup.remove(&c);
                                }
                            }
                        }
                    }

                    // Only enable the Paste option if a port is open
                    GLOBAL.with(|global| {
                        if let Some((_, _, ref state)) = *global.borrow() {
                            if !state.connected {
                                for c in popup.get_children() {
                                    // Workaround for Bug 778162:
                                    // https://bugzilla.gnome.org/show_bug.cgi?id=778162
                                    if c.is::<gtk::SeparatorMenuItem>() {
                                        continue;
                                    }
                                    if let Ok(child) = c.downcast::<gtk::MenuItem>() {
                                        if let Some(l) = child.get_label() {
                                            if l == "_Paste" {
                                                child.set_sensitive(false);
                                            }
                                        }
                                    }
                                }

                            }
                        }
                    });

                    // Add a "Clear All" button that's only active if there's
                    // data in the buffer.
                    let clear_all = gtk::MenuItem::new_with_label("Clear All");
                    if let Some(b) = t.get_buffer() {
                        if b.get_char_count() == 0 {
                            clear_all.set_sensitive(false);
                        } else {
                            clear_all.connect_activate(|_| {
                                GLOBAL.with(|global| {
                                    if let Some((ref ui, _, _)) = *global.borrow() {
                                        // In order to clear the buffer we need to
                                        // disable the insert-text and delete-range
                                        // signal handlers.
                                        signal_handler_block(&ui.text_buffer,
                                                             ui.text_view_insert_signal);
                                        signal_handler_block(&ui.text_buffer,
                                                             ui.text_buffer_delete_signal);
                                        ui.text_buffer.set_text("");
                                        signal_handler_unblock(&ui.text_buffer,
                                                               ui.text_buffer_delete_signal);
                                        signal_handler_unblock(&ui.text_buffer,
                                                               ui.text_view_insert_signal);
                                    }
                                });
                            });
                        }
                    }
                    popup.append(&clear_all);
                    popup.show_all();
                }
            });
        }
    });

    GLOBAL.with(|global| {
        if let Some((ref mut ui, _, _)) = *global.borrow_mut() {
            let b = &ui.text_buffer;
            ui.text_view_insert_signal = b.connect_insert_text(|b, _, text| {
                GLOBAL.with(|global| {
                    if let Some((_, ref serial_thread, _)) = *global.borrow() {
                        let v = Vec::from(text);
                        match serial_thread.send_port_data_cmd(v) {
                            Err(GeneralError::Send(_)) => {
                                error!("Error sending data command to child \
                                          thread. Aborting.")
                            }
                            Err(_) | Ok(_) => (),
                        }
                    }
                });
                signal_stop_emission_by_name(b, "insert-text");
            });
            ui.open_button_clicked_signal = open_button_clicked_signal;
        }
    });

    // Disable deletion of characters within the textview
    GLOBAL.with(|global| {
        if let Some((ref mut ui, _, _)) = *global.borrow_mut() {
            let b = &ui.text_buffer;
            ui.text_buffer_delete_signal = b.connect_delete_range(move |b, _, _| {
                signal_stop_emission_by_name(b, "delete-range");
            });
        }
    });

    // Process any command line arguments that were passed
    if !serial_port_name.is_empty() && !serial_baud.is_empty() {
        if let Some(ports_selector_index) = ports_selector_map.get(&serial_port_name) {
            ports_selector.set_active(*ports_selector_index as i32);
        } else {
            error!("Invalid port name '{}' specified.", serial_port_name);
            process::exit(ExitCode::ArgumentError as i32);
        }
        if let Some(baud_selector_index) = baud_selector_map.get(&serial_baud) {
            baud_selector.set_active(*baud_selector_index);
        } else {
            error!("Invalid baud rate '{}' specified.", serial_baud);
            process::exit(ExitCode::ArgumentError as i32);
        }
        open_button.set_active(true);
    } else if !serial_port_name.is_empty() {
        error!("A baud rate must be specified.");
        process::exit(ExitCode::ArgumentError as i32);
    } else if !serial_baud.is_empty() {
        error!("A port name must be specified.");
        process::exit(ExitCode::ArgumentError as i32);
    }

    GLOBAL.with(|global| {
        if let Some((ref ui, _, _)) = *global.borrow() {
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
        if let Some((ref ui, ref serial_thread, ref mut state)) = *global.borrow_mut() {
            let window = &ui.window;
            let view = &ui.text_view;
            let buf = &ui.text_buffer;
            let f_button = &ui.file_button;
            let s_button = &ui.save_button;
            let o_button = &ui.open_button;
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
                    signal_handler_block(f_button, ui.file_button_toggled_signal);
                    f_button.set_active(false);
                    signal_handler_unblock(f_button, ui.file_button_toggled_signal);
                    s_button.set_sensitive(false);
                    signal_handler_block(s_button, ui.save_button_toggled_signal);
                    s_button.set_active(false);
                    signal_handler_unblock(s_button, ui.save_button_toggled_signal);
                    state.connected = false;
                }
                Ok(SerialResponse::OpenPortSuccess) => {
                    f_button.set_sensitive(true);
                    s_button.set_sensitive(true);
                    o_button.set_active(true);
                    state.connected = true;
                }
                Ok(SerialResponse::OpenPortError(s)) => {
                    debug!("OpenPortError: {}", s);
                    let dialog = gtk::MessageDialog::new(Some(window),
                                                         gtk::DIALOG_DESTROY_WITH_PARENT,
                                                         gtk::MessageType::Error,
                                                         gtk::ButtonsType::Ok,
                                                         &s);
                    dialog.run();
                    dialog.destroy();
                    f_button.set_sensitive(false);
                    s_button.set_sensitive(false);
                    signal_handler_block(o_button, ui.open_button_clicked_signal);
                    o_button.set_active(false);
                    signal_handler_unblock(o_button, ui.open_button_clicked_signal);
                    state.connected = false;
                }
                Ok(SerialResponse::SendingFileComplete) |
                Ok(SerialResponse::SendingFileCanceled) => {
                    info!("Sending file complete");
                    f_button.set_active(false);
                    view.set_editable(true);
                }
                Ok(SerialResponse::SendingFileError(_)) => {
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
                Ok(SerialResponse::LogToFileError(_)) => {
                    s_button.set_active(false);
                    let dialog = gtk::MessageDialog::new(Some(window),
                                                         gtk::DIALOG_DESTROY_WITH_PARENT,
                                                         gtk::MessageType::Error,
                                                         gtk::ButtonsType::Ok,
                                                         "Error logging to file");
                    dialog.run();
                    dialog.destroy();
                }
                Ok(SerialResponse::LoggingFileCanceled) => {
                    info!("Logging file canceled");
                    s_button.set_active(false);
                }
                Err(_) => (),
            }
        }
    });
    glib::Continue(false)
}

fn file_button_connect_toggled(b: &gtk::ToggleButton) {
    GLOBAL.with(|global| {
        if let Some((ref ui, ref serial_thread, _)) = *global.borrow() {
            let window = &ui.window;
            let view = &ui.text_view;
            if b.get_active() {
                let dialog = gtk::FileChooserDialog::new(Some("Send File"),
                                                         Some(window),
                                                         gtk::FileChooserAction::Open);
                dialog.add_buttons(&[("Send", gtk::ResponseType::Ok.into()),
                                     ("Cancel", gtk::ResponseType::Cancel.into())]);
                let result = dialog.run();
                if result == gtk::ResponseType::Ok.into() {
                    let filename = dialog.get_filename().unwrap();
                    match serial_thread.send_port_file_cmd(filename) {
                        Err(_) => {
                            error!("Error sending port_file command to child \
                                    thread. Aborting.");
                            b.set_sensitive(true);
                            b.set_active(false);
                        },
                        Ok(_) => view.set_editable(false)
                    }
                } else {
                    // Make the button look inactive if the user canceled the
                    // file open dialog
                    signal_handler_block(&ui.file_button,
                                         ui.file_button_toggled_signal);
                    b.set_active(false);
                    signal_handler_unblock(&ui.file_button,
                                           ui.file_button_toggled_signal);
                }

                dialog.destroy();
            } else {
                match serial_thread.send_cancel_file_cmd() {
                    Err(GeneralError::Send(_)) => {
                        error!("Error sending cancel_file command to child \
                                thread. Aborting.");
                    }
                    Err(_) | Ok(_) => (),
                }
            }
        }
    });
}

fn save_button_connect_toggled(b: &gtk::ToggleButton) {
    GLOBAL.with(|global| {
        if let Some((ref ui, ref serial_thread, _)) = *global.borrow() {
            let window = &ui.window;
            if b.get_active() {
                let dialog = gtk::FileChooserDialog::new(Some("Log to File"),
                                                         Some(window),
                                                         gtk::FileChooserAction::Save);
                dialog.add_buttons(&[("Log", gtk::ResponseType::Ok.into()),
                                     ("Cancel", gtk::ResponseType::Cancel.into())]);
                let result = dialog.run();
                if result == gtk::ResponseType::Ok.into() {
                    let filename = dialog.get_filename().unwrap();
                    if serial_thread.send_log_to_file_cmd(filename).is_err() {
                        error!("Error sending log_to_file command to child \
                                thread. Aborting.");
                        b.set_sensitive(true);
                        b.set_active(false);
                    }
                } else {
                    // Make the button look inactive if the user canceled the
                    // file save dialog
                    signal_handler_block(&ui.save_button,
                                         ui.save_button_toggled_signal);
                    b.set_active(false);
                    signal_handler_unblock(&ui.save_button,
                                           ui.save_button_toggled_signal);
                }

                dialog.destroy();
            } else {
                match serial_thread.send_cancel_log_to_file_cmd() {
                    Err(GeneralError::Send(_)) => {
                        error!("Error sending cancel_log_to_file command to \
                                child thread. Aborting.");
                    }
                    Err(_) | Ok(_) => (),
                }
            }
        }
    });
}
