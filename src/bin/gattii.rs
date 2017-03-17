extern crate argparse;
extern crate cairo;
extern crate chrono;
extern crate core;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate gdk;
extern crate glib;
extern crate gtk;

extern crate gattii;

use std::cell::RefCell;
use std::collections::HashMap;
use std::process;
use std::string::String;

use argparse::{ArgumentParser, Store};
use cairo::Context;
use chrono::prelude::*;
use gdk::prelude::*;
use glib::{signal_stop_emission_by_name, signal_handler_block, signal_handler_unblock};
use gtk::prelude::*;

use gattii::*;

#[derive(Debug)]
enum ExitCode {
    ArgumentError = 1,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum StatusContext {
    PortOperation,
    FileOperation,
}

struct Ui {
    window: gtk::Window,
    text_view: gtk::TextView,
    hex_view: gtk::TextView,
    scrolled_text_view: gtk::ScrolledWindow,
    scrolled_hex_view: gtk::ScrolledWindow,
    text_buffer: gtk::TextBuffer,
    hex_buffer: gtk::TextBuffer,
    file_button: gtk::ToggleButton,
    open_button: gtk::ToggleButton,
    save_button: gtk::ToggleButton,
    status_bar: gtk::Statusbar,
    status_bar_contexts: HashMap<StatusContext, u32>,
    data_bits_scale: gtk::Scale,
    stop_bits_scale: gtk::Scale,
    parity_dropdown: gtk::ComboBoxText,
    flow_control_dropdown: gtk::ComboBoxText,
    baud_dropdown: gtk::ComboBoxText,
    baud_map: HashMap<String, i32>,
    ports_dropdown: gtk::ComboBoxText,
    ports_map: HashMap<String, i32>,
    text_buffer_insert_signal: u64,
    hex_buffer_insert_signal: u64,
    text_buffer_delete_signal: u64,
    hex_buffer_delete_signal: u64,
    open_button_clicked_signal: u64,
    file_button_toggled_signal: u64,
    save_button_toggled_signal: u64,
    file_button_progress_icon: gtk::DrawingArea,
    file_button_static_icon: gtk::Image,
}

struct State {
    /// True if a serial port is currently connected
    connected: bool,
    /// The line ending that is sent when ENTER is pressed
    line_ending: String,
    /// The percentage completion of sending a file [0, 100]
    send_file_percentage: u8,
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
        ap.refer(&mut serial_baud).add_option(&["-b", "--baud"],
                                              Store,
                                              "The serial port baud rate (default 115200)");
        ap.parse_args_or_exit();
    }

    if gtk::init().is_err() {
        error!("Failed to initialize GTK.");
        return;
    }

    ui_init();

    ui_connect();

    GLOBAL.with(|global| {
        if let Some((ref ui, _, _)) = *global.borrow() {

            // Process any command line arguments that were passed
            if !serial_port_name.is_empty() && !serial_baud.is_empty() {
                if let Some(ports_dropdown_index) = ui.ports_map.get(&serial_port_name) {
                    ui.ports_dropdown.set_active(*ports_dropdown_index as i32);
                } else {
                    error!("Invalid port name '{}' specified.", serial_port_name);
                    process::exit(ExitCode::ArgumentError as i32);
                }
                if let Some(baud_dropdown_index) = ui.baud_map.get(&serial_baud) {
                    ui.baud_dropdown.set_active(*baud_dropdown_index);
                } else {
                    error!("Invalid baud rate '{}' specified.", serial_baud);
                    process::exit(ExitCode::ArgumentError as i32);
                }
                ui.open_button.set_active(true);
            } else if !serial_port_name.is_empty() {
                error!("A baud rate must be specified.");
                process::exit(ExitCode::ArgumentError as i32);
            } else if !serial_baud.is_empty() {
                error!("A port name must be specified.");
                process::exit(ExitCode::ArgumentError as i32);
            }

            // Set deleting the window to close the entire application
            ui.window.connect_delete_event(|_, _| {
                                               gtk::main_quit();
                                               Inhibit(false)
                                           });
        }
    });

    // Start our GUI main loop
    gtk::main();
}

fn ui_init() {
    // Create the main window
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Gattii - Your Serial Terminal Interface");
    window.set_position(gtk::WindowPosition::Center);
    window.set_default_size(400, 300);

    // Create the top toolbar
    let toolbar = gtk::Toolbar::new();
    toolbar.set_show_arrow(false);

    // Add a port selector
    let ports_dropdown = gtk::ComboBoxText::new();
    let mut ports_dropdown_map = HashMap::new();
    if let Ok(mut ports) = list_ports() {
        ports.sort();
        if !ports.is_empty() {
            for (i, p) in (0i32..).zip(ports.into_iter()) {
                ports_dropdown.append(None, &p);
                ports_dropdown_map.insert(p, i);
            }
            ports_dropdown.set_active(0);
        } else {
            ports_dropdown.append(None, "No ports found");
            ports_dropdown.set_active(0);
            ports_dropdown.set_sensitive(false);
        }
    } else {
        ports_dropdown.append(None, "No ports found");
        ports_dropdown.set_active(0);
        ports_dropdown.set_sensitive(false);
    }
    let ports_dropdown_container = gtk::ToolItem::new();
    ports_dropdown_container.add(&ports_dropdown);

    // Add a baud rate selector
    let mut baud_dropdown_map = HashMap::new();
    baud_dropdown_map.insert("921600".to_string(), 0i32);
    baud_dropdown_map.insert("115200".to_string(), 1i32);
    baud_dropdown_map.insert("57600".to_string(), 2i32);
    baud_dropdown_map.insert("38400".to_string(), 3i32);
    baud_dropdown_map.insert("19200".to_string(), 4i32);
    baud_dropdown_map.insert("9600".to_string(), 5i32);
    toolbar.add(&ports_dropdown_container);
    let baud_dropdown = gtk::ComboBoxText::new();
    baud_dropdown.append(None, "921600");
    baud_dropdown.append(None, "115200");
    baud_dropdown.append(None, "57600");
    baud_dropdown.append(None, "38400");
    baud_dropdown.append(None, "19200");
    baud_dropdown.append(None, "9600");
    baud_dropdown.set_active(5);
    let baud_dropdown_container = gtk::ToolItem::new();
    baud_dropdown_container.add(&baud_dropdown);
    toolbar.add(&baud_dropdown_container);

    // Add the port settings button
    let port_settings_button = gtk::MenuButton::new();
    port_settings_button.set_direction(gtk::ArrowType::None);
    let port_settings_popover = gtk::Popover::new(Some(&port_settings_button));
    port_settings_popover.set_position(gtk::PositionType::Bottom);
    // Enable the following once upgrading to GTK+3.20+
    // port_settings_popover.set_constrain_to(gtk::PopoverConstraint::None);
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

    // Add the open button, disabling it if no ports are available
    let open_button_container = gtk::ToolItem::new();
    let open_button = gtk::ToggleButton::new_with_label("Open");
    if ports_dropdown_map.is_empty() {
        open_button.set_sensitive(false);
    }
    open_button_container.add(&open_button);
    toolbar.add(&open_button_container);

    // Add some horizontally-expanding space
    let separator = gtk::SeparatorToolItem::new();
    separator.set_draw(false);
    separator.set_expand(true);
    toolbar.add(&separator);

    // This drawing area draws a pie-chart showing progress as stored in the
    // `GLOBAL::state.send_file_percentage` variable.
    // See src/nautilus-toolbar.c, line 688, from GTK's nautilus program.
    let operations_icon = gtk::DrawingArea::new();
    operations_icon.show();
    operations_icon.set_size_request(16, 16);
    // FIXME: Simplify the following line once gtk-rs#468 is resolved
    //        (https://github.com/gtk-rs/gtk/issues/468)
    operations_icon.add_events((gdk::BUTTON_PRESS_MASK | gdk::BUTTON_RELEASE_MASK).bits() as i32);
    operations_icon.connect_draw(|w, c| {
        GLOBAL.with(|global| {
            if let Some((.., ref state)) = *global.borrow() {
                let style_context = w.get_style_context().unwrap();
                let foreground = style_context.get_color(w.get_state_flags());
                let mut background = foreground.clone();
                background.alpha *= 0.3;
                let background = background;
                let width = w.get_allocated_width() as f64;
                let height = w.get_allocated_height() as f64;
                let two_pi = 2.0 * std::f64::consts::PI;
                <Context as ContextExt>::set_source_rgba(&c, &background);
                c.arc(width / 2.0,
                      height / 2.0,
                      width.min(height) / 2.0,
                      0.0,
                      two_pi);
                c.fill();
                c.move_to(width / 2.0, height / 2.0);
                <Context as ContextExt>::set_source_rgba(&c, &foreground);
                let arc_start = -std::f64::consts::FRAC_PI_2;
                let arc_end = arc_start + state.send_file_percentage as f64 / 100.0 * two_pi;
                c.arc(width / 2.0,
                      height / 2.0,
                      width.min(height) / 2.0,
                      arc_start,
                      arc_end);
                c.fill();
            }
        });
        Inhibit(false)
    });

    // Add send file button
    let send_file_button = gtk::ToggleButton::new();
    send_file_button.set_tooltip_text("Send file");
    let send_file_image = gtk::Image::new_from_file("resources/upload.svg");
    send_file_button.set_image(&send_file_image);
    send_file_button.set_sensitive(false);
    let send_file_button_container = gtk::ToolItem::new();
    send_file_button_container.add(&send_file_button);
    toolbar.add(&send_file_button_container);

    // Add save file button
    let save_file_button = gtk::ToggleButton::new();
    save_file_button.set_tooltip_text("Log to file");
    let save_file_image = gtk::Image::new_from_file("resources/download.svg");
    save_file_button.set_image(&save_file_image);
    save_file_button.set_sensitive(false);
    let save_file_button_container = gtk::ToolItem::new();
    save_file_button_container.add(&save_file_button);
    toolbar.add(&save_file_button_container);

    // Create dual text buffers, one with ASCII text and the other with the hex equivalent. We also
    // Create an "end" text mark within the buffers that we can use to insert new text. This has
    // a left-gravity so that inserting text at this mark will keep the mark at the end of it.
    // This is necessary because the "insert" mark gets moved when users select text.
    let text_buffer = gtk::TextBuffer::new(None);
    let mark = text_buffer.get_insert().unwrap();
    let iter = text_buffer.get_iter_at_mark(&mark);
    text_buffer.create_mark("end", &iter, false);
    let hex_buffer = gtk::TextBuffer::new(None);
    let mark = hex_buffer.get_insert().unwrap();
    let iter = hex_buffer.get_iter_at_mark(&mark);
    hex_buffer.create_mark("end", &iter, false);

    // Create two text views, one for the text and hex data
    let text_view = gtk::TextView::new_with_buffer(&text_buffer);
    text_view.set_wrap_mode(gtk::WrapMode::Char);
    text_view.set_cursor_visible(false);
    let hex_view = gtk::TextView::new_with_buffer(&hex_buffer);
    hex_view.set_wrap_mode(gtk::WrapMode::Char);
    hex_view.set_cursor_visible(false);

    // Set up an auto-scrolling text view for each text view, hiding the hex one. Only one of these
    // should ever be shown at a time.
    let scrolled_text_view = gtk::ScrolledWindow::new(None, None);
    scrolled_text_view.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scrolled_text_view.add(&text_view);
    let scrolled_hex_view = gtk::ScrolledWindow::new(None, None);
    scrolled_hex_view.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scrolled_hex_view.add(&hex_view);

    // Add a status bar
    let status_bar = gtk::Statusbar::new();
    // A context id for port operations (open, close, change port, change settings, etc.)
    let context_id_port_ops = status_bar.get_context_id("port operations");
    // A context id for file operations (log file start & end, send file start & end)
    let context_id_file_ops = status_bar.get_context_id("file operations");
    let context_map: HashMap<StatusContext, u32> =
        [(StatusContext::PortOperation, context_id_port_ops),
         (StatusContext::FileOperation, context_id_file_ops)]
                .iter()
                .cloned()
                .collect();

    // Pack everything vertically
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.pack_start(&toolbar, false, false, 0);
    vbox.pack_start(&scrolled_text_view, true, true, 0);
    vbox.pack_start(&scrolled_hex_view, true, true, 0);
    vbox.pack_start(&status_bar, false, false, 0);
    window.add(&vbox);

    // Make sure all desired widgets are visible.
    window.show_all();
    scrolled_hex_view.hide();

    // Set CSS styles for the entire application.
    let css_provider = gtk::CssProvider::new();
    let display = gdk::Display::get_default().expect("Couldn't open default GDK display");
    let screen = display.get_default_screen();
    gtk::StyleContext::add_provider_for_screen(&screen,
                                               &css_provider,
                                               gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    css_provider.load_from_path("resources/style.css").expect("Failed to load CSS stylesheet");

    // Set up channels for communicating with the port thread.
    let ui = Ui {
        window: window.clone(),
        text_view: text_view.clone(),
        hex_view: hex_view.clone(),
        scrolled_text_view: scrolled_text_view.clone(),
        scrolled_hex_view: scrolled_hex_view.clone(),
        text_buffer: text_buffer.clone(),
        hex_buffer: hex_buffer.clone(),
        file_button: send_file_button.clone(),
        open_button: open_button.clone(),
        save_button: save_file_button.clone(),
        status_bar: status_bar.clone(),
        status_bar_contexts: context_map,
        data_bits_scale: data_bits_scale.clone(),
        stop_bits_scale: stop_bits_scale.clone(),
        parity_dropdown: parity_dropdown.clone(),
        flow_control_dropdown: flow_control_dropdown.clone(),
        baud_dropdown: baud_dropdown.clone(),
        baud_map: baud_dropdown_map,
        ports_dropdown: ports_dropdown.clone(),
        ports_map: ports_dropdown_map,
        text_buffer_insert_signal: 0,
        hex_buffer_insert_signal: 0,
        text_buffer_delete_signal: 0,
        hex_buffer_delete_signal: 0,
        open_button_clicked_signal: 0,
        file_button_toggled_signal: 0,
        save_button_toggled_signal: 0,
        file_button_progress_icon: operations_icon,
        file_button_static_icon: send_file_image,
    };
    let state = State {
        connected: false,
        line_ending: "\n".to_string(),
        send_file_percentage: 0,
    };
    GLOBAL.with(move |global| {
                    *global.borrow_mut() =
                        Some((ui, SerialThread::new(|| { glib::idle_add(receive); }), state));
                });
}

fn ui_connect() {

    GLOBAL.with(|global| {
        if let Some((ref mut ui, _, _)) = *global.borrow_mut() {
            ui.baud_dropdown.connect_changed(move |s| if let Some(baud_rate) = s.get_active_text() {
                                                 GLOBAL.with(|global| {
                    if let Some((_, ref serial_thread, _)) = *global.borrow() {
                        match serial_thread.send_port_change_baud_cmd(baud_rate.clone()) {
                            Err(GeneralError::Parse(_)) => {
                                error!("Invalid baud rate '{}' specified.", &baud_rate)
                            }
                            Err(GeneralError::Send(_)) => {
                                error!("Error sending port_open command to child thread. \
                                            Aborting.")
                            }
                            Ok(_) => (),
                        }
                    }
                });
            });

            ui.ports_dropdown.connect_changed(move |s| if let Some(port_name) =
                s.get_active_text() {
                    GLOBAL.with(|global| {
                        if let Some((_, ref serial_thread, _)) = *global.borrow() {
                            match serial_thread.send_port_change_port_cmd(port_name.clone()) {
                                Err(GeneralError::Parse(_)) => {
                                    error!("Invalid port name '{}' specified.", &port_name)
                                }
                                Err(GeneralError::Send(_)) => {
                                    error!("Error sending change_port command to child thread. \
                                                Aborting.")
                                }
                                Ok(_) => (),
                            }
                        }
                    });
            });

            ui.open_button_clicked_signal = ui.open_button.connect_clicked(move |s| {
                if s.get_active() {
                    GLOBAL.with(|global| {
                        if let Some((ref ui, ref serial_thread, _)) = *global.borrow() {
                            if let Some(port_name) = ui.ports_dropdown.get_active_text() {
                                if let Some(baud_rate) = ui.baud_dropdown.get_active_text() {
                                    match serial_thread.send_port_open_cmd(port_name,
                                                                           baud_rate.clone()) {
                                        Err(GeneralError::Parse(_)) => {
                                            error!("Invalid baud rate '{}' specified.", &baud_rate)
                                        }
                                        Err(GeneralError::Send(_)) => {
                                            error!("Error sending port_open command to child \
                                                    thread. Aborting.")
                                        }
                                        // After opening the port has succeeded, set the focus on
                                        // the text view so the user can start sending data
                                        // immediately (this also prevents ENTER from closing the
                                        // port, likely not what the user intends or expects).
                                        Ok(_) => ui.text_view.grab_focus(),
                                    }
                                }
                            }
                        }
                    });
                } else {
                    GLOBAL.with(|global| if let Some((ref ui, ref sthread, _)) = *global.borrow() {
                            match sthread.send_port_close_cmd() {
                                Err(GeneralError::Send(_)) => {
                                    error!("Error sending port_close command to child thread. \
                                            Aborting.")
                                }
                                Err(_) | Ok(_) => (),
                            }
                            ui.file_button.set_image(&ui.file_button_static_icon);
                        });
                }
            });

            // Connect send file selector button to callback. This is left as a
            // separate function to reduce rightward drift.
            ui.file_button_toggled_signal = ui.file_button
                .connect_toggled(file_button_connect_toggled);

            // Connect log file selector button to callback. This is left as a
            // separate function to reduce rightward drift.
            ui.save_button_toggled_signal = ui.save_button
                .connect_toggled(save_button_connect_toggled);

            // Configure the data bits callback
            ui.data_bits_scale.connect_value_changed(|s| {
                let data_bits = match s.get_value() {
                    5.0 => DataBits::Five,
                    6.0 => DataBits::Six,
                    7.0 => DataBits::Seven,
                    8.0 => DataBits::Eight,
                    _ => unreachable!(),
                };
                GLOBAL.with(|global| if let Some((_, ref serial_thread, _)) = *global.borrow() {
                    match serial_thread.send_port_change_data_bits_cmd(data_bits) {
                        Err(GeneralError::Parse(_)) => {
                            unreachable!();
                        }
                        Err(GeneralError::Send(_)) => {
                            error!("Error sending data bits change command to child thread. \
                                    Aborting.")
                        }
                        Ok(_) => (),
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
                GLOBAL.with(|global| if let Some((_, ref serial_thread, _)) = *global.borrow() {
                    match serial_thread.send_port_change_stop_bits_cmd(stop_bits) {
                        Err(GeneralError::Parse(_)) => {
                            unreachable!();
                        }
                        Err(GeneralError::Send(_)) => {
                            error!("Error sending stop bits change command to child thread. \
                                    Aborting.")
                        }
                        Ok(_) => (),
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
                    GLOBAL.with(|global| if let Some((_, ref serial_thread, _)) = *global.borrow() {
                        match serial_thread.send_port_change_parity_cmd(parity) {
                            Err(GeneralError::Parse(_)) => unreachable!(),
                            Err(GeneralError::Send(_)) => {
                                error!("Error sending parity change command \
                                  to child thread. Aborting.")
                            }
                            Ok(_) => (),
                        }
                    });
                }
            });

            // Configure the flow control dropdown callback
            ui.flow_control_dropdown.connect_changed(|s| {
                let flow_control = match s.get_active_text() {
                    Some(ref x) if x == "None" => Some(FlowControl::None),
                    Some(ref x) if x == "Software" => Some(FlowControl::Software),
                    Some(ref x) if x == "Hardware" => Some(FlowControl::Hardware),
                    Some(_) | None => unreachable!(),
                };
                if let Some(flow_control) = flow_control {
                    GLOBAL.with(|global| if let Some((_, ref serial_thread, _)) = *global.borrow() {
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
                        });
                }
            });

            // Configure the right-click menu for the both the text and hex view widgets
            ui.text_view.connect_populate_popup(view_populate_popup);
            ui.hex_view.connect_populate_popup(view_populate_popup);

            ui.text_view.connect_key_press_event(|_, k| {
                GLOBAL.with(|global| {
                    if let Some((_, ref serial_thread, ref state)) = *global.borrow() {
                        if state.connected {
                            let mut cmd: Option<(u8, char)> = None;
                            // Check for a backspace with no modifier keys
                            if k.get_state().is_empty() &&
                               k.get_keyval() == gdk::enums::key::BackSpace {
                                cmd = Some((8, 'H'));
                            }
                            // Check for @, A-Z, [, \, ], ^, and _ with CTRL pressed
                            else if k.get_state().contains(gdk::CONTROL_MASK) {
                                if let Some(key) = gdk::keyval_to_unicode(k.get_keyval()) {
                                    cmd = match key {
                                        '@' => Some((0, key)),
                                        'A'...'Z' => Some((1 + key as u8 - b'A', key)),
                                        '[' => Some((27, key)),
                                        '\\' => Some((28, key)),
                                        ']' => Some((29, key)),
                                        '^' => Some((30, key)),
                                        '_' => Some((31, key)),
                                        _ => None,
                                    };
                                }
                            }
                            if let Some((cmd, debug_char)) = cmd {
                                info!("Sending Ctrl-{}", debug_char);
                                match serial_thread.send_port_data_cmd(&[cmd as u8]) {
                                    Err(GeneralError::Send(_)) => {
                                        error!("Error sending data command to child thread. \
                                                Aborting.")
                                    }
                                    Err(e) => error!("{:?}", e),
                                    Ok(_) => (),
                                }
                            }
                        }
                    }
                });
                Inhibit(false)
            });

            // Allow the user to send data by typing/pasting it in either buffer
            ui.text_buffer_insert_signal = ui.text_buffer.connect_insert_text(buffer_insert);
            ui.hex_buffer_insert_signal = ui.hex_buffer.connect_insert_text(buffer_insert);

            // Disable deletion of characters within the textview
            ui.text_buffer_delete_signal = ui.text_buffer.connect_delete_range(move |b, _, _| {
                signal_stop_emission_by_name(b, "delete-range");
            });
            ui.hex_buffer_delete_signal =
                ui.hex_buffer.connect_delete_range(move |b, _, _| {
                                                       signal_stop_emission_by_name(b,
                                                                                    "delete-range");
                                                   });
        }
    });
}

fn view_populate_popup(text_view: &gtk::TextView, popup: &gtk::Widget) {
    if let Ok(popup) = popup.clone().downcast::<gtk::Menu>() {

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

        let separator = gtk::SeparatorMenuItem::new();
        popup.prepend(&separator);

        // Add a submenu for selecting the newline to use
        let newline_submenu = gtk::Menu::new();
        let newline_n = gtk::RadioMenuItem::new_with_label(&[], "\\n");
        let group = newline_n.get_group();
        newline_submenu.append(&newline_n);
        let newline_r = gtk::RadioMenuItem::new_with_label(&group, "\\r");
        newline_submenu.append(&newline_r);
        let newline_rn = gtk::RadioMenuItem::new_with_label(&group, "\\r\\n");
        newline_submenu.append(&newline_rn);
        GLOBAL.with(|global| if let Some((.., ref state)) = *global.borrow() {
                        match state.line_ending.as_ref() {
                            "\n" => newline_n.activate(),
                            "\r" => newline_r.activate(),
                            "\r\n" => newline_rn.activate(),
                            _ => unreachable!(),
                        };
                    });
        newline_n.connect_toggled(|w| {
            GLOBAL.with(|global| {
                if let Some((.., ref mut state)) = *global.borrow_mut() {
                    // The toggle signal triggers on activation and deactivation, so only respond
                    // to activations here.
                    if w.get_active() {
                        state.line_ending = "\n".to_string();
                    }
                }
            });
        });
        newline_r.connect_toggled(|w| {
            GLOBAL.with(|global| {
                if let Some((.., ref mut state)) = *global.borrow_mut() {
                    // The toggle signal triggers on activation and deactivation, so only respond
                    // to activations here.
                    if w.get_active() {
                        state.line_ending = "\r".to_string();
                    }
                }
            });
        });
        newline_rn.connect_toggled(|w| {
            GLOBAL.with(|global| {
                if let Some((.., ref mut state)) = *global.borrow_mut() {
                    // The toggle signal triggers on activation and deactivation, so only respond
                    // to activations here.
                    if w.get_active() {
                        state.line_ending = "\r\n".to_string();
                    }
                }
            });
        });

        let newline = gtk::MenuItem::new_with_label("Enter sends");
        newline.set_submenu(Some(&newline_submenu));
        popup.prepend(&newline);

        // Add the text or Hex view selectors
        // Note: These are in reverse order because they use `prepend()`.
        let separator = gtk::SeparatorMenuItem::new();
        popup.prepend(&separator);
        let view_hex = gtk::RadioMenuItem::new_with_label(&[], "Hex");
        popup.prepend(&view_hex);
        let group = view_hex.get_group();
        let view_text = gtk::RadioMenuItem::new_with_label(&group, "Text");
        popup.prepend(&view_text);
        GLOBAL.with(|global| if let Some((ref ui, ..)) = *global.borrow() {
                        if ui.scrolled_hex_view.get_visible() {
                            view_hex.activate();
                        } else if ui.scrolled_text_view.get_visible() {
                view_text.activate();
            }
                    });
        view_hex.connect_toggled(|w| {
            GLOBAL.with(|global| {
                if let Some((ref ui, ..)) = *global.borrow() {
                    // The toggle signal triggers on activation and deactivation, so only respond
                    // to activations here.
                    if w.get_active() {
                        // Toggle the shown text view
                        ui.scrolled_text_view.hide();
                        ui.scrolled_hex_view.show();

                        // Calculate the relative position of the scroll within the available view.
                        // Adjustment objects have a range of: [lower, upper-page_size]
                        let text_vadj = ui.text_view.get_vadjustment().unwrap();
                        let rel_pos = match text_vadj.get_upper() - text_vadj.get_page_size() {
                            x if x > 0.0 => text_vadj.get_value() / x,
                            _ => 0.0,
                        };

                        // Use this relative position from the text view to generate the new
                        // relative position for the hex view.
                        let vadj = ui.hex_view.get_vadjustment().unwrap();
                        let new_value = vadj.get_lower() +
                                        rel_pos * (vadj.get_upper() - vadj.get_page_size());
                        vadj.set_value(new_value);
                    }
                }
            });
        });
        view_text.connect_toggled(|w| {
            GLOBAL.with(|global| {
                if let Some((ref ui, ..)) = *global.borrow() {
                    // The toggle signal triggers on activation and deactivation, so only respond
                    // to activations here.
                    if w.get_active() {
                        // Toggle the shown text view
                        ui.scrolled_hex_view.hide();
                        ui.scrolled_text_view.show();

                        // Calculate the relative position of the scroll within the available view.
                        // Adjustment objects have a range of: [lower, upper-page_size]
                        let hex_vadj = ui.hex_view.get_vadjustment().unwrap();
                        let rel_pos = match hex_vadj.get_upper() - hex_vadj.get_page_size() {
                            x if x > 0.0 => hex_vadj.get_value() / x,
                            _ => 0.0,
                        };

                        // Use this relative position from the text view to generate the new
                        // relative position for the hex view.
                        let vadj = ui.text_view.get_vadjustment().unwrap();
                        let new_value = vadj.get_lower() +
                                        rel_pos * (vadj.get_upper() - vadj.get_page_size());
                        vadj.set_value(new_value);
                    }
                }
            });
        });

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
        if let Some(b) = text_view.get_buffer() {
            if b.get_char_count() == 0 {
                clear_all.set_sensitive(false);
            } else {
                clear_all.connect_activate(|_| {
                    GLOBAL.with(|global| {
                        if let Some((ref ui, _, _)) = *global.borrow() {
                            // In order to clear the buffer we need to
                            // disable the insert-text and delete-range
                            // signal handlers.
                            signal_handler_block(&ui.text_buffer, ui.text_buffer_insert_signal);
                            signal_handler_block(&ui.text_buffer, ui.text_buffer_delete_signal);
                            ui.text_buffer.set_text("");
                            signal_handler_unblock(&ui.text_buffer, ui.text_buffer_delete_signal);
                            signal_handler_unblock(&ui.text_buffer, ui.text_buffer_insert_signal);
                            signal_handler_block(&ui.hex_buffer, ui.hex_buffer_insert_signal);
                            signal_handler_block(&ui.hex_buffer, ui.hex_buffer_delete_signal);
                            ui.hex_buffer.set_text("");
                            signal_handler_unblock(&ui.hex_buffer, ui.hex_buffer_delete_signal);
                            signal_handler_unblock(&ui.hex_buffer, ui.hex_buffer_insert_signal);
                        }
                    });
                });
            }
        }
        popup.append(&clear_all);

        popup.show_all();
    }
}

fn buffer_insert(textbuffer: &gtk::TextBuffer, _: &gtk::TextIter, text: &str) {
    GLOBAL.with(|global| if let Some((_, ref serial_thread, ref state)) = *global.borrow() {
        let text = text.replace("\n", &state.line_ending);
        debug!("Sending {:?}", &text);
        let text = text.as_bytes();
        match serial_thread.send_port_data_cmd(text) {
            Err(GeneralError::Send(_)) => {
                error!("Error sending data command to child thread. Aborting.")
            }
            Err(_) | Ok(_) => (),
        }
    });
    signal_stop_emission_by_name(textbuffer, "insert-text");
}

fn receive() -> glib::Continue {
    GLOBAL.with(|global| {
        if let Some((ref ui, ref serial_thread, ref mut state)) = *global.borrow_mut() {
            let window = &ui.window;
            let view = &ui.text_view;
            let ascii_buf = &ui.text_buffer;
            let hex_buf = &ui.hex_buffer;
            let f_button = &ui.file_button;
            let s_button = &ui.save_button;
            let o_button = &ui.open_button;
            match serial_thread.from_port_chan_rx.try_recv() {
                Ok(SerialResponse::Data(data)) => {
                    debug!("Received '{:?}'", data);

                    // Don't know why this needs to be this complicated, but found
                    // the answer on the gtk+ forums:
                    // http://www.gtkforums.com/viewtopic.php?t=1307

                    // Add the text to the Hex buffer first
                    // Get the position of our special "end" mark, which will always stay at the end
                    // of the buffer.
                    let mark = hex_buf.get_mark("end").unwrap();
                    let mut iter = hex_buf.get_iter_at_mark(&mark);

                    let mut hex_data = Vec::new();
                    for c in &data {
                        let upper_half = (c & 0xF0) >> 4;
                        if upper_half >= 10 {
                            hex_data.push(b'A' + upper_half - 10)
                        } else {
                            hex_data.push(b'0' + upper_half);
                        }
                        let lower_half = c & 0x0F;
                        if lower_half >= 10 {
                            hex_data.push(b'A' + lower_half - 10)
                        } else {
                            hex_data.push(b'0' + lower_half);
                        }
                        hex_data.push(b' ');
                    }

                    // Inserts data at the end
                    signal_handler_block(hex_buf, ui.hex_buffer_insert_signal);
                    hex_buf.insert(&mut iter, &String::from_utf8_lossy(&hex_data));
                    signal_handler_unblock(hex_buf, ui.hex_buffer_insert_signal);

                    // Add the text to the ASCII buffer
                    let mark = ascii_buf.get_mark("end").unwrap();
                    let mut iter = ascii_buf.get_iter_at_mark(&mark);
                    signal_handler_block(ascii_buf, ui.text_buffer_insert_signal);
                    ascii_buf.insert(&mut iter, &String::from_utf8_lossy(&data));
                    signal_handler_unblock(ascii_buf, ui.text_buffer_insert_signal);

                    // Keep the textview scrolled to the bottom. This is indepenent of which buffer
                    // is active, so we just need to do it once.
                    let mark = view.get_buffer()
                        .unwrap()
                        .get_mark("end")
                        .unwrap();
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
                    log_status(&ui, StatusContext::PortOperation, "Port closed");
                }
                Ok(SerialResponse::OpenPortSuccess) => {
                    f_button.set_sensitive(true);
                    s_button.set_sensitive(true);
                    o_button.set_active(true);
                    state.connected = true;
                    log_status(&ui, StatusContext::PortOperation, "Port opened");
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
                    log_status(&ui, StatusContext::PortOperation, "Error opening port");
                }
                Ok(SerialResponse::SendingFileComplete) => {
                    signal_handler_block(&ui.file_button, ui.file_button_toggled_signal);
                    f_button.set_active(false);
                    signal_handler_unblock(&ui.file_button, ui.file_button_toggled_signal);
                    view.set_editable(true);
                    log_status(&ui, StatusContext::FileOperation, "Sending file finished");
                    f_button.set_image(&ui.file_button_static_icon);
                }
                Ok(SerialResponse::SendingFileCanceled) => {
                    info!("Sending file complete");
                    signal_handler_block(&ui.file_button, ui.file_button_toggled_signal);
                    f_button.set_active(false);
                    signal_handler_unblock(&ui.file_button, ui.file_button_toggled_signal);
                    view.set_editable(true);
                    log_status(&ui, StatusContext::FileOperation, "Sending file canceled");
                    f_button.set_image(&ui.file_button_static_icon);
                }
                Ok(SerialResponse::SendingFileError(_)) => {
                    signal_handler_block(&ui.file_button, ui.file_button_toggled_signal);
                    f_button.set_active(false);
                    signal_handler_unblock(&ui.file_button, ui.file_button_toggled_signal);
                    view.set_editable(true);
                    f_button.set_image(&ui.file_button_static_icon);
                    let dialog = gtk::MessageDialog::new(Some(window),
                                                         gtk::DIALOG_DESTROY_WITH_PARENT,
                                                         gtk::MessageType::Error,
                                                         gtk::ButtonsType::Ok,
                                                         "Error sending file");
                    dialog.run();
                    dialog.destroy();
                    log_status(&ui,
                               StatusContext::FileOperation,
                               "Error while sending file");
                }
                Ok(SerialResponse::SendingFileStarted) => {
                    f_button.set_image(&ui.file_button_progress_icon);
                    state.send_file_percentage = 0;
                }
                Ok(SerialResponse::SendingFileProgress(i)) => {
                    info!("Sending file {}% complete", i);
                    state.send_file_percentage = i;
                    ui.file_button_progress_icon.queue_draw();
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
                    log_status(&ui,
                               StatusContext::FileOperation,
                               "Error while logging to file");
                }
                Ok(SerialResponse::LoggingFileCanceled) => {
                    info!("Logging file canceled");
                    s_button.set_active(false);
                    log_status(&ui, StatusContext::FileOperation, "Logging to file stopped");
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
                    match serial_thread.send_port_file_cmd(filename.clone()) {
                        Err(_) => {
                            error!("Error sending port_file command to child thread. Aborting.");
                            b.set_sensitive(true);
                            b.set_active(false);
                            log_status(&ui,
                                       StatusContext::FileOperation,
                                       "Error trying to send file");
                        }
                        Ok(_) => {
                            // TODO: Add a SerialResponse::SendingFileStarted and move this into
                            // receive()
                            view.set_editable(false);
                            log_status(&ui,
                                       StatusContext::FileOperation,
                                       &format!("Started sending file '{}'",
                                                filename.to_str().unwrap()));
                        }
                    }
                } else {
                    // Make the button look inactive if the user canceled the
                    // file open dialog
                    signal_handler_block(&ui.file_button, ui.file_button_toggled_signal);
                    b.set_active(false);
                    signal_handler_unblock(&ui.file_button, ui.file_button_toggled_signal);
                }

                dialog.destroy();
            } else {
                match serial_thread.send_cancel_file_cmd() {
                    Err(GeneralError::Send(_)) => {
                        error!("Error sending cancel_file command to child thread. Aborting.");
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
                    if serial_thread.send_log_to_file_cmd(filename.clone()).is_err() {
                        error!("Error sending log_to_file command to child thread. Aborting.");
                        b.set_sensitive(true);
                        b.set_active(false);
                    } else {
                        // TODO: Add a SerialResponse::LogToFileStarted and move this into receive()
                        log_status(&ui,
                                   StatusContext::FileOperation,
                                   &format!("Started logging to file '{}'",
                                            filename.to_str().unwrap()));
                    }
                } else {
                    // Make the button look inactive if the user canceled the
                    // file save dialog
                    signal_handler_block(&ui.save_button, ui.save_button_toggled_signal);
                    b.set_active(false);
                    signal_handler_unblock(&ui.save_button, ui.save_button_toggled_signal);
                }

                dialog.destroy();
            } else {
                match serial_thread.send_cancel_log_to_file_cmd() {
                    Err(GeneralError::Send(_)) => {
                        error!("Error sending cancel_log_to_file command to child thread. \
                                Aborting.");
                    }
                    Err(_) | Ok(_) => (),
                }
            }
        }
    });
}

/// Log messages to the status bar using the specific status context.
fn log_status(ui: &Ui, context: StatusContext, message: &str) {
    let context_id = ui.status_bar_contexts.get(&context).unwrap();
    let timestamp = UTC::now().format("%Y-%m-%d %H:%M:%S");
    let formatted_message = format!("[{}]: {}", timestamp, message);
    ui.status_bar.push(*context_id, &formatted_message);
}
