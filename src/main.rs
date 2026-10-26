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
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(true);
        settings.set_gtk_label_select_on_focus(false);
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
    scroll.set_max_content_height(420);
    scroll.set_propagate_natural_height(true);
    let sections = gtk::Box::new(gtk::Orientation::Vertical, 12);

    add_command_section(&sections, &format_command(&review.command));
    add_section(&sections, None, &review.reason);
    add_details_section(&sections, review);
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
    add_section(&sections, Some("Origin observed by caller"), &origins);
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

fn format_command(command: &[String]) -> String {
    let mut display = String::from("sudo");
    for argument in command {
        display.push(' ');
        if argument.is_empty() {
            display.push_str("\\0");
            continue;
        }
        for character in argument.chars() {
            match character {
                '\\' => display.push_str("\\\\"),
                ' ' => display.push_str("\\ "),
                '\t' => display.push_str("\\t"),
                '\n' => display.push_str("\\n"),
                '\r' => display.push_str("\\r"),
                character if character.is_whitespace() || character.is_control() => {
                    display.push_str(&format!("\\u{{{:x}}}", character as u32));
                }
                character => display.push(character),
            }
        }
    }
    display
}

fn add_command_section(container: &gtk::Box, command: &str) {
    let frame = gtk::Frame::new(None);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    scroll.set_hexpand(true);

    let label = gtk::Label::new(Some(command));
    label.set_selectable(true);
    label.set_xalign(0.0);
    label.set_margin_top(8);
    label.set_margin_bottom(8);
    label.set_margin_start(10);
    label.set_margin_end(10);
    scroll.set_child(Some(&label));
    frame.set_child(Some(&scroll));
    container.append(&frame);
}

fn add_section(container: &gtk::Box, title: Option<&str>, value: &str) {
    let frame = gtk::Frame::new(title);
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

fn add_details_section(container: &gtk::Box, review: &Review) {
    let frame = gtk::Frame::new(None);
    let details = gtk::Box::new(gtk::Orientation::Vertical, 8);
    details.set_margin_top(8);
    details.set_margin_bottom(8);
    details.set_margin_start(10);
    details.set_margin_end(10);

    add_detail_row(&details, "Working directory:", &review.working_directory);
    add_detail_row(
        &details,
        "Requesting user:",
        &format!("{} ({})", review.requesting_user, review.requesting_uid),
    );
    add_detail_row(&details, "Run as:", &review.run_as_user);

    frame.set_child(Some(&details));
    container.append(&frame);
}

fn add_detail_row(container: &gtk::Box, title: &str, value: &str) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);

    let title_label = gtk::Label::new(Some(title));
    row.append(&title_label);

    let value_label = gtk::Label::new(Some(value));
    value_label.set_selectable(true);
    value_label.set_wrap(true);
    value_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value_label.set_xalign(0.0);
    value_label.set_hexpand(true);
    row.append(&value_label);

    container.append(&row);
}

#[cfg(test)]
mod tests {
    use super::format_command;

    #[test]
    fn command_is_one_line_without_added_quotes() {
        let command = [
            "nixos-rebuild".into(),
            "switch".into(),
            "--flake".into(),
            "/home/qy1n/nixos#laptop".into(),
        ];
        assert_eq!(
            format_command(&command),
            "sudo nixos-rebuild switch --flake /home/qy1n/nixos#laptop"
        );
    }

    #[test]
    fn command_preserves_unusual_argument_boundaries() {
        let command = ["a b".into(), "".into(), "a\\b".into(), "a\nb".into()];
        assert_eq!(format_command(&command), "sudo a\\ b \\0 a\\\\b a\\nb");
    }
}
