use askpass::{parse_review, validate_stdout, write_password, Review};
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

fn main() -> std::process::ExitCode {
    let review = match parse_review(std::env::args_os().skip(1)) {
        Ok(review) => review,
        Err(error) => {
            eprintln!("askpass: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    if let Err(error) = validate_stdout() {
        eprintln!("askpass: {error}");
        return std::process::ExitCode::FAILURE;
    }

    gtk::gdk::set_allowed_backends("wayland");
    if gtk::init().is_err() || gtk::gdk::Display::default().is_none() {
        eprintln!("askpass: a Wayland display is required");
        return std::process::ExitCode::FAILURE;
    }

    let app = gtk::Application::new(
        Some("org.askpass.Review"),
        gtk::gio::ApplicationFlags::NON_UNIQUE,
    );
    let submitted = Rc::new(Cell::new(false));
    let submitted_for_window = submitted.clone();
    app.connect_activate(move |app| build_window(app, &review, submitted_for_window.clone()));
    app.run_with_args(&["askpass"]);

    if submitted.get() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

fn build_window(app: &gtk::Application, review: &Review, submitted: Rc<Cell<bool>>) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Administrator access requested")
        .default_width(560)
        .default_height(520)
        .build();

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);
    outer.set_margin_start(16);
    outer.set_margin_end(16);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_min_content_height(180);
    let sections = gtk::Box::new(gtk::Orientation::Vertical, 12);

    add_section(&sections, "Reason supplied by caller", &review.reason);
    let command = review
        .command
        .iter()
        .enumerate()
        .map(|(index, argument)| format!("{index}: {argument:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    add_section(&sections, "Command (argument boundaries)", &command);
    add_section(
        &sections,
        "Working directory",
        &format!("{:?}", review.working_directory),
    );
    add_section(
        &sections,
        "Requesting user",
        &format!(
            "{:?} (UID {})",
            review.requesting_user, review.requesting_uid
        ),
    );
    add_section(&sections, "Run as", &format!("{:?}", review.run_as_user));
    let origins = if review.origins.is_empty() {
        "Unavailable from caller".to_owned()
    } else {
        review
            .origins
            .iter()
            .map(|origin| format!("PID {}: {:?}", origin.pid, origin.executable))
            .collect::<Vec<_>>()
            .join("\n")
    };
    add_section(&sections, "Origin observed by caller", &origins);
    scroll.set_child(Some(&sections));
    outer.append(&scroll);

    let password = gtk::PasswordEntry::new();
    password.set_show_peek_icon(false);
    password.set_placeholder_text(Some("Password"));
    password.set_activates_default(false);
    outer.append(&password);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let authorize = gtk::Button::with_label("Authorize");
    authorize.set_sensitive(false);
    authorize.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&authorize);
    outer.append(&buttons);
    window.set_child(Some(&outer));

    let authorize_for_change = authorize.clone();
    password
        .connect_changed(move |entry| authorize_for_change.set_sensitive(!entry.text().is_empty()));

    let submit = {
        let password = password.clone();
        let app = app.clone();
        move || {
            let text = password.text();
            if text.is_empty() {
                return;
            }
            let result = write_password(&text);
            password.set_text("");
            match result {
                Ok(()) => submitted.set(true),
                Err(error) => eprintln!("askpass: could not write password to stdout: {error}"),
            }
            app.quit();
        }
    };
    let submit = Rc::new(submit);
    let submit_for_button = submit.clone();
    authorize.connect_clicked(move |_| submit_for_button());
    password.connect_activate(move |_| submit());

    let app_for_cancel = app.clone();
    cancel.connect_clicked(move |_| app_for_cancel.quit());
    let app_for_escape = app.clone();
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            app_for_escape.quit();
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);

    window.present();
    password.grab_focus();
}

fn add_section(container: &gtk::Box, title: &str, value: &str) {
    let frame = gtk::Frame::new(Some(title));
    let label = gtk::Label::new(None);
    label.set_text(value);
    label.set_selectable(true);
    label.set_wrap(true);
    label.set_max_width_chars(72);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    label.set_margin_top(8);
    label.set_margin_bottom(8);
    label.set_margin_start(10);
    label.set_margin_end(10);
    frame.set_child(Some(&label));
    container.append(&frame);
}
