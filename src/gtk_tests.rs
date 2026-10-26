//! Display tests run explicitly and isolate GTK and stdout in child processes.
use super::*;
use std::ffi::OsString;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PASSWORD: &str = "sëcret<&%_test";
const TEST_NAME: &str = "gtk_tests::dialog_contracts";

struct PasswordPipe {
    saved_stdout: OwnedFd,
    reader: std::fs::File,
}

impl PasswordPipe {
    fn new() -> Self {
        let mut fds = [-1; 2];
        // SAFETY: pipe2 writes two owned descriptors into fds; dup copies stdout.
        let (reader, writer, saved_stdout) = unsafe {
            assert_eq!(libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC), 0);
            let saved = libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 3);
            assert!(saved >= 0);
            (
                std::fs::File::from_raw_fd(fds[0]),
                OwnedFd::from_raw_fd(fds[1]),
                OwnedFd::from_raw_fd(saved),
            )
        };
        // SAFETY: both descriptors are valid; this occurs only in an isolated child.
        assert_eq!(
            unsafe { libc::dup2(writer.as_raw_fd(), libc::STDOUT_FILENO) },
            libc::STDOUT_FILENO
        );
        Self {
            saved_stdout,
            reader,
        }
    }

    fn restore(&self) {
        // SAFETY: saved_stdout remains owned and valid throughout this guard's life.
        assert_eq!(
            unsafe { libc::dup2(self.saved_stdout.as_raw_fd(), libc::STDOUT_FILENO) },
            libc::STDOUT_FILENO
        );
    }

    fn output(&mut self) -> Vec<u8> {
        self.restore();
        let mut output = Vec::new();
        self.reader.read_to_end(&mut output).unwrap();
        output
    }
}

impl Drop for PasswordPipe {
    fn drop(&mut self) {
        self.restore();
    }
}

fn request(long: bool) -> Vec<OsString> {
    let reason = if long {
        "<b>literal</b>\nRequested by: _é<&%\n".repeat(150)
    } else {
        "<b>literal</b>\nRequested by: _é<&%".into()
    };
    let argument = if long {
        "a b\\c\n<&%é".repeat(200)
    } else {
        "a b\\c\n<&%é".into()
    };
    let mut args: Vec<OsString> = [
        "--review",
        "--reason",
        &reason,
        "--command-arg",
        "/bin/true",
        "--command-arg",
        &argument,
        "--command-arg",
        "",
        "--working-directory",
        "/tmp/<b>_é&%",
        "--requesting-user",
        "alice_<&%",
        "--requesting-uid",
        "1000",
        "--run-as-user",
        "root_<&%",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    if long {
        args.extend(
            ["--origin", "42:/bin/a:b", "--origin", "1:/sbin/init"]
                .into_iter()
                .map(OsString::from),
        );
    }
    args
}

fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut result = vec![widget.clone()];
    let mut child = widget.first_child();
    while let Some(widget) = child {
        result.extend(descendants(&widget));
        child = widget.next_sibling();
    }
    // GTK can unparent collapsed expander contents from its widget hierarchy.
    if let Some(expander) = widget.downcast_ref::<gtk::Expander>() {
        if let Some(child) = expander.child() {
            if !result.contains(&child) {
                result.extend(descendants(&child));
            }
        }
    }
    result
}

fn button(widgets: &[gtk::Widget], text: &str) -> gtk::Button {
    widgets
        .iter()
        .filter_map(|widget| widget.clone().downcast::<gtk::Button>().ok())
        .find(|button| button.label().as_deref() == Some(text))
        .unwrap()
}

fn assert_layout(window: &gtk::Window, widgets: &[gtk::Widget], review: &Review) {
    assert_eq!(
        window.title().as_deref(),
        Some("Administrator access requested")
    );
    assert!(window.is_resizable());
    assert_eq!(
        window.application().unwrap().application_id().as_deref(),
        Some("org.askpass.Review")
    );
    assert!(gtk::Settings::default()
        .unwrap()
        .is_gtk_application_prefer_dark_theme());
    let labels: Vec<gtk::Label> = widgets
        .iter()
        .filter_map(|widget| widget.clone().downcast::<gtk::Label>().ok())
        .collect();
    for text in [
        &review.reason,
        &review.working_directory,
        &review.requesting_user,
        &review.run_as_user,
        &format_command(&review.command),
    ] {
        let label = labels
            .iter()
            .find(|label| label.text().as_str() == text)
            .unwrap();
        assert!(!label.uses_markup());
        assert!(!label.uses_underline());
        assert!(label.is_selectable());
        assert!(label.wraps());
        assert_eq!(label.ellipsize(), gtk::pango::EllipsizeMode::None);
    }
    let command = format_command(&review.command);
    assert!(labels
        .iter()
        .find(|label| label.text().as_str() == command)
        .unwrap()
        .has_css_class("monospace"));
    let expander = widgets
        .iter()
        .find_map(|widget| widget.clone().downcast::<gtk::Expander>().ok())
        .unwrap();
    assert!(!expander.is_expanded());
    assert!(labels
        .iter()
        .any(|label| label.text() == "Request details are supplied by the caller."));
    let origins = if review.origins.is_empty() {
        "Unavailable"
    } else {
        "PID 42: \"/bin/a:b\"\nPID 1: \"/sbin/init\""
    };
    assert!(labels.iter().any(|label| label.text() == origins));
    assert!(labels.iter().any(|label| label.text() == "1000"));
    let password = widgets
        .iter()
        .find_map(|widget| widget.clone().downcast::<gtk::PasswordEntry>().ok())
        .unwrap();
    assert!(!password.shows_peek_icon());
    let focus = gtk::prelude::GtkWindowExt::focus(window).unwrap();
    assert!(focus == password || focus.is_ancestor(&password));
    let mnemonic = labels
        .iter()
        .find(|label| label.label() == "_Password")
        .unwrap();
    assert_eq!(mnemonic.mnemonic_widget(), Some(password.clone().upcast()));
    let scroll = widgets
        .iter()
        .find_map(|widget| widget.clone().downcast::<gtk::ScrolledWindow>().ok())
        .unwrap();
    assert!(!password.is_ancestor(&scroll));
    assert!(!button(widgets, "Authorize").is_ancestor(&scroll));
    assert!(!button(widgets, "Cancel").is_ancestor(&scroll));
    for widget in [
        password.upcast::<gtk::Widget>(),
        button(widgets, "Authorize").upcast(),
        button(widgets, "Cancel").upcast(),
    ] {
        let bounds = widget.compute_bounds(window).unwrap();
        assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
        assert!(bounds.y() >= 0.0 && bounds.y() + bounds.height() <= window.height() as f32 + 1.0);
    }
    if review.reason.len() > 1000 {
        let adjustment = scroll.vadjustment();
        assert!(
            adjustment.upper() > adjustment.page_size(),
            "long content must scroll"
        );
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
        assert!(adjustment.value() > 0.0);
    }
}

fn run_scenario(scenario: &str) {
    let args = request(scenario.starts_with("layout"));
    let review = parse_review(args.clone()).unwrap();
    let mut pipe = PasswordPipe::new();
    // SAFETY: stdout is now a valid pipe descriptor.
    let limit = unsafe { libc::fpathconf(libc::STDOUT_FILENO, libc::_PC_PIPE_BUF) } as usize;
    assert!(limit > 0);
    let scenario_for_driver = scenario.to_owned();
    let exercised = Rc::new(Cell::new(false));
    let exercised_for_driver = exercised.clone();
    let mut stage = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut drive = move || {
        assert!(Instant::now() < deadline, "dialog did not become usable");
        let Some(window) = gtk::Window::list_toplevels()
            .into_iter()
            .find_map(|widget| widget.downcast::<gtk::Window>().ok())
        else {
            return gtk::glib::ControlFlow::Continue;
        };
        if !window.is_mapped() || window.height() == 0 {
            return gtk::glib::ControlFlow::Continue;
        }
        if scenario_for_driver == "layout-scaled" && stage == 0 {
            gtk::Settings::default()
                .unwrap()
                .set_gtk_font_name(Some("Sans 20"));
            stage = 1;
            return gtk::glib::ControlFlow::Continue;
        }
        let widgets = descendants(window.upcast_ref());
        let password = widgets
            .iter()
            .find_map(|widget| widget.clone().downcast::<gtk::PasswordEntry>().ok())
            .unwrap();
        let authorize = button(&widgets, "Authorize");
        assert_layout(&window, &widgets, &review);
        assert!(!authorize.is_sensitive());
        if scenario_for_driver == "empty" && stage == 0 {
            password.emit_activate();
            // Even a directly emitted click must be guarded against empty input.
            authorize.emit_clicked();
            stage = 1;
            return gtk::glib::ControlFlow::Continue;
        }
        password.set_text(PASSWORD);
        assert!(authorize.is_sensitive());
        password.set_text("");
        assert!(!authorize.is_sensitive());
        password.set_text(PASSWORD);
        match scenario_for_driver.as_str() {
            "authorize" => authorize.emit_clicked(),
            "enter" => password.emit_activate(),
            "oversized" => {
                password.set_text(&"x".repeat(limit));
                authorize.emit_clicked();
            }
            "broken-pipe" => authorize.emit_clicked(),
            "cancel" | "empty" | "layout" | "layout-scaled" => {
                button(&widgets, "Cancel").emit_clicked()
            }
            "escape" => {
                let controllers = window.observe_controllers();
                let keys = (0..controllers.n_items())
                    .find_map(|index| {
                        controllers
                            .item(index)
                            .unwrap()
                            .downcast::<gtk::EventControllerKey>()
                            .ok()
                    })
                    .unwrap();
                assert!(!keys.emit_by_name::<bool>(
                    "key-pressed",
                    &[&gtk::gdk::Key::a, &0u32, &gtk::gdk::ModifierType::empty()]
                ));
                assert!(keys.emit_by_name::<bool>(
                    "key-pressed",
                    &[
                        &gtk::gdk::Key::Escape,
                        &0u32,
                        &gtk::gdk::ModifierType::empty()
                    ]
                ));
            }
            "close" => window.close(),
            _ => panic!("unknown scenario"),
        }
        if matches!(
            scenario_for_driver.as_str(),
            "authorize" | "enter" | "oversized" | "broken-pipe"
        ) {
            assert!(password.text().is_empty());
        }
        exercised_for_driver.set(true);
        gtk::glib::ControlFlow::Break
    };
    // Report assertions through Rust's test harness, rather than unwinding across
    // GLib's C callback boundary (which aborts the process).
    let failure = Rc::new(std::cell::RefCell::new(None));
    let failure_for_driver = failure.clone();
    gtk::glib::timeout_add_local(
        Duration::from_millis(50),
        move || match std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut drive)) {
            Ok(flow) => flow,
            Err(error) => {
                *failure_for_driver.borrow_mut() = Some(error);
                for window in gtk::Window::list_toplevels() {
                    if let Ok(window) = window.downcast::<gtk::Window>() {
                        window.application().unwrap().quit();
                    }
                }
                gtk::glib::ControlFlow::Break
            }
        },
    );

    // Closing the sole reader exercises a failure after startup validation succeeds.
    // Retain a harmless replacement file so the stdout guard can still restore itself.
    if scenario == "broken-pipe" {
        pipe.reader = std::fs::File::open("/dev/null").unwrap();
    }
    let status = run(args);
    let output = pipe.output();
    if let Some(error) = failure.borrow_mut().take() {
        std::panic::resume_unwind(error);
    }
    assert!(exercised.get(), "dialog was never exercised");
    if matches!(scenario, "authorize" | "enter") {
        assert_eq!(status, std::process::ExitCode::SUCCESS);
        assert_eq!(output, format!("{PASSWORD}\n").as_bytes());
    } else {
        assert_eq!(status, std::process::ExitCode::FAILURE);
        assert!(
            output.is_empty(),
            "cancellation/failure leaked password bytes"
        );
    }
}

#[test]
#[ignore = "requires a Wayland display; see README testing instructions"]
fn dialog_contracts() {
    if let Ok(scenario) = std::env::var("ASKPASS_TEST_SCENARIO") {
        run_scenario(&scenario);
        return;
    }
    for scenario in [
        "authorize",
        "enter",
        "empty",
        "cancel",
        "escape",
        "close",
        "oversized",
        "broken-pipe",
        "layout",
        "layout-scaled",
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", TEST_NAME, "--nocapture"])
            .env("ASKPASS_TEST_SCENARIO", scenario)
            .env("GDK_BACKEND", "wayland")
            .env("GTK_A11Y", "none")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("{scenario} timed out: {output:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{scenario}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains(PASSWORD), "password appeared on stderr");
        if matches!(scenario, "oversized" | "broken-pipe") {
            assert!(stderr.contains("could not write password to stdout"));
        }
    }
}
