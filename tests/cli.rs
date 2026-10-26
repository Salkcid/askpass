use std::ffi::OsString;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn valid() -> Vec<OsString> {
    [
        "--review",
        "--reason",
        "Test request",
        "--command-arg",
        "/bin/true",
        "--working-directory",
        "/tmp",
        "--requesting-user",
        "alice",
        "--requesting-uid",
        "1000",
        "--run-as-user",
        "root",
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_askpass"));
    command
        .env("WAYLAND_DISPLAY", "/tmp/askpass-nonexistent-wayland-socket")
        .env("DISPLAY", ":2147483647")
        .env("GDK_BACKEND", "x11")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn finish(command: &mut Command) -> Output {
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "askpass did not exit: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_failure(output: &Output, diagnostic: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "unexpected stdout: {:?}",
        output.stdout
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(diagnostic), "{stderr}");
    assert!(
        !stderr.contains("Wayland display"),
        "validation must precede GTK: {stderr}"
    );
}

#[test]
fn invalid_requests_fail_before_display_initialization_with_empty_stdout() {
    let mut missing = valid();
    missing.truncate(missing.len() - 2);
    let mut duplicate = valid();
    duplicate.extend(["--reason", "again"].into_iter().map(OsString::from));
    let mut malformed = valid();
    malformed.extend(
        ["--origin", "not-a-pid:/bin/sh"]
            .into_iter()
            .map(OsString::from),
    );
    let mut oversized = valid();
    oversized.extend(["--command-arg".into(), "x".repeat(16 * 1024).into()]);
    for (args, diagnostic) in [
        (vec![], "expected --review"),
        (missing, "missing run-as user"),
        (duplicate, "duplicate --reason"),
        (malformed, "origin must be PID:EXECUTABLE"),
        (oversized, "review request exceeds 16 KiB"),
    ] {
        assert_failure(&finish(command().args(args)), diagnostic);
    }
}

#[test]
fn non_pipe_stdout_is_rejected_before_display_initialization() {
    let path = std::env::temp_dir().join(format!("askpass-stdout-test-{}", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let output = finish(command().args(valid()).stdout(file));
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_failure(&output, "stdout must be a pipe");
    assert!(bytes.is_empty());
}

#[test]
fn unavailable_wayland_fails_with_empty_stdout_even_with_x11_requested() {
    let output = finish(command().args(valid()));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("a Wayland display is required"));
}
