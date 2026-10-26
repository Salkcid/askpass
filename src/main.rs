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

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 20);
    outer.set_margin_top(24);
    outer.set_margin_bottom(24);
    outer.set_margin_start(24);
    outer.set_margin_end(24);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_max_content_height(420);
    scroll.set_propagate_natural_height(true);
    let sections = gtk::Box::new(gtk::Orientation::Vertical, 20);

    let heading = text_label("Administrator access requested");
    let heading_style = gtk::pango::AttrList::new();
    heading_style.insert(gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Bold));
    heading_style.insert(gtk::pango::AttrFloat::new_scale(1.3));
    heading.set_attributes(Some(&heading_style));
    sections.append(&heading);

    let explanation = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let reason_label = text_label("Reason provided by caller");
    reason_label.add_css_class("dim-label");
    explanation.append(&reason_label);
    let reason = text_label(&review.reason);
    reason.set_selectable(true);
    explanation.append(&reason);
    sections.append(&explanation);

    add_command_section(&sections, &format_command(&review.command));
    let identity = gtk::Box::new(gtk::Orientation::Vertical, 6);
    add_detail_row(&identity, "Working directory:", &review.working_directory);
    add_detail_row(&identity, "Run as:", &review.run_as_user);
    add_detail_row(&identity, "Requested by:", &review.requesting_user);
    sections.append(&identity);
    add_details_section(&sections, review);
    scroll.set_child(Some(&sections));
    outer.append(&scroll);

    outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let password_section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let password_label = gtk::Label::new(Some("_Password"));
    password_label.set_use_underline(true);
    password_label.set_xalign(0.0);
    let password = gtk::PasswordEntry::new();
    password.set_show_peek_icon(false);
    password.set_activates_default(false);
    password_label.set_mnemonic_widget(Some(&password));
    password_section.append(&password_label);
    password_section.append(&password);
    outer.append(&password_section);

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
    let mut display = String::new();
    for (index, argument) in command.iter().enumerate() {
        if index > 0 {
            display.push(' ');
        }
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

fn text_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_max_width_chars(60);
    label.set_xalign(0.0);
    label
}

fn add_command_section(container: &gtk::Box, command: &str) {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    section.append(&text_label("Command"));
    let frame = gtk::Frame::new(None);
    let label = text_label(command);
    label.add_css_class("monospace");
    label.set_selectable(true);
    label.set_margin_top(12);
    label.set_margin_bottom(12);
    label.set_margin_start(12);
    label.set_margin_end(12);
    frame.set_child(Some(&label));
    section.append(&frame);
    container.append(&section);
}

fn add_details_section(container: &gtk::Box, review: &Review) {
    let expander = gtk::Expander::new(Some("Request details"));
    let details = gtk::Box::new(gtk::Orientation::Vertical, 10);
    details.set_margin_top(12);
    details.set_margin_start(16);

    add_detail_row(
        &details,
        "Requesting UID:",
        &review.requesting_uid.to_string(),
    );
    let origins = if review.origins.is_empty() {
        "Unavailable".to_owned()
    } else {
        review
            .origins
            .iter()
            .map(|origin| format!("PID {}: {:?}", origin.pid, origin.executable))
            .collect::<Vec<_>>()
            .join("\n")
    };
    add_detail_row(&details, "Request origin:", &origins);
    let provenance = text_label("Request details are supplied by the caller.");
    provenance.add_css_class("dim-label");
    details.append(&provenance);
    expander.set_child(Some(&details));
    container.append(&expander);
}

fn add_detail_row(container: &gtk::Box, title: &str, value: &str) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);

    let title_label = gtk::Label::new(Some(title));
    title_label.set_valign(gtk::Align::Start);
    title_label.add_css_class("dim-label");
    row.append(&title_label);

    let value_label = text_label(value);
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
            "nixos-rebuild switch --flake /home/qy1n/nixos#laptop"
        );
    }

    #[test]
    fn command_preserves_unusual_argument_boundaries() {
        let command = ["a b".into(), "".into(), "a\\b".into(), "a\nb".into()];
        assert_eq!(format_command(&command), "a\\ b \\0 a\\\\b a\\nb");
    }
}
