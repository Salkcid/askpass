use std::ffi::OsString;
use std::io;
use std::os::fd::RawFd;

pub const MAX_DISPLAY_BYTES: usize = 16 * 1024;

pub fn write_password(password: &str) -> io::Result<()> {
    write_password_to_fd(libc::STDOUT_FILENO, password)
}

pub fn validate_stdout() -> io::Result<()> {
    pipe_atomic_limit(libc::STDOUT_FILENO).map(|_| ())
}

fn pipe_atomic_limit(fd: RawFd) -> io::Result<usize> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: stat points to writable storage, and fd is checked by fstat.
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstat initialized stat on success.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "stdout must be a pipe",
        ));
    }

    // SAFETY: fd is a valid pipe descriptor after fstat.
    let pipe_buf = unsafe { libc::fpathconf(fd, libc::_PC_PIPE_BUF) };
    if pipe_buf <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "pipe atomic write limit is unavailable",
        ));
    }
    Ok(pipe_buf as usize)
}

fn write_password_to_fd(fd: RawFd, password: &str) -> io::Result<()> {
    let pipe_buf = pipe_atomic_limit(fd)?;
    let length = password
        .len()
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "password is too long"))?;
    if length > pipe_buf {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "password exceeds the pipe's atomic write limit",
        ));
    }

    let bytes = password.as_bytes();
    let newline = b"\n";
    let parts = [
        libc::iovec {
            iov_base: bytes.as_ptr() as *mut libc::c_void,
            iov_len: bytes.len(),
        },
        libc::iovec {
            iov_base: newline.as_ptr() as *mut libc::c_void,
            iov_len: newline.len(),
        },
    ];
    // SAFETY: both buffers remain alive for the call, and writev only reads them.
    let written = unsafe { libc::writev(fd, parts.as_ptr(), parts.len() as libc::c_int) };
    if written < 0 {
        return Err(io::Error::last_os_error());
    }
    if written as usize != length {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "incomplete password write",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub pid: u32,
    pub executable: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub reason: String,
    pub command: Vec<String>,
    pub working_directory: String,
    pub requesting_user: String,
    pub requesting_uid: u32,
    pub run_as_user: String,
    pub origins: Vec<Origin>,
}

pub fn parse_review(args: impl IntoIterator<Item = OsString>) -> Result<Review, String> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--review")) {
        return Err("expected --review".into());
    }

    let mut reason = None;
    let mut command = Vec::new();
    let mut working_directory = None;
    let mut requesting_user = None;
    let mut requesting_uid = None;
    let mut run_as_user = None;
    let mut origins = Vec::new();
    let mut request_bytes = "--review".len();

    while let Some(option) = args.next() {
        let option = option
            .into_string()
            .map_err(|_| "option name is not UTF-8")?;
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {option}"))?
            .into_string()
            .map_err(|_| format!("value for {option} is not UTF-8"))?;
        request_bytes = request_bytes
            .checked_add(option.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .ok_or("review request is too large")?;
        if request_bytes > MAX_DISPLAY_BYTES {
            return Err("review request exceeds 16 KiB".into());
        }

        match option.as_str() {
            "--reason" => set_once(&mut reason, value, &option)?,
            "--command-arg" => command.push(value),
            "--working-directory" => set_once(&mut working_directory, value, &option)?,
            "--requesting-user" => set_once(&mut requesting_user, value, &option)?,
            "--requesting-uid" => set_once(&mut requesting_uid, value, &option)?,
            "--run-as-user" => set_once(&mut run_as_user, value, &option)?,
            "--origin" => {
                let (pid, executable) = value
                    .split_once(':')
                    .ok_or("origin must be PID:EXECUTABLE")?;
                if executable.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("origin must be PID:EXECUTABLE".into());
                }
                let pid = pid.parse::<u32>().map_err(|_| "invalid origin PID")?;
                if pid == 0 {
                    return Err("origin PID must be positive".into());
                }
                origins.push(Origin {
                    pid,
                    executable: executable.into(),
                });
            }
            _ => return Err(format!("unsupported option: {option}")),
        }
    }

    let reason = required(reason, "reason")?;
    if reason.trim().is_empty() {
        return Err("reason must not be empty".into());
    }
    if command.is_empty() || command[0].is_empty() {
        return Err("command must include a nonempty executable".into());
    }
    let working_directory = required_nonempty(working_directory, "working directory")?;
    let requesting_user = required_nonempty(requesting_user, "requesting user")?;
    let uid_text = required_nonempty(requesting_uid, "requesting UID")?;
    if !uid_text.bytes().all(|b| b.is_ascii_digit()) {
        return Err("requesting UID must be an unsigned integer".into());
    }
    let requesting_uid = uid_text
        .parse::<u32>()
        .map_err(|_| "invalid requesting UID")?;
    let run_as_user = required_nonempty(run_as_user, "run-as user")?;

    Ok(Review {
        reason,
        command,
        working_directory,
        requesting_user,
        requesting_uid,
        run_as_user,
        origins,
    })
}

fn set_once(slot: &mut Option<String>, value: String, name: &str) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err(format!("duplicate {name}"));
    }
    Ok(())
}

fn required(value: Option<String>, name: &str) -> Result<String, String> {
    value.ok_or_else(|| format!("missing {name}"))
}

fn required_nonempty(value: Option<String>, name: &str) -> Result<String, String> {
    let value = required(value, name)?;
    if value.is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStringExt;

    fn replace(args: &mut [OsString], option: &str, value: impl Into<OsString>) {
        let index = args.iter().position(|arg| arg == option).unwrap();
        args[index + 1] = value.into();
    }

    fn valid() -> Vec<OsString> {
        [
            "--review",
            "--reason",
            "Install updates",
            "--command-arg",
            "/usr/bin/true",
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

    #[test]
    fn preserves_argument_boundaries() {
        let mut args = valid();
        args.extend(
            [
                "--command-arg",
                "",
                "--command-arg",
                "--flag",
                "--command-arg",
                "a\n<&%é",
            ]
            .into_iter()
            .map(OsString::from),
        );
        let review = parse_review(args).unwrap();
        assert_eq!(review.command, ["/usr/bin/true", "", "--flag", "a\n<&%é"]);
    }

    #[test]
    fn parses_every_field_and_preserves_origin_order() {
        let mut args = valid();
        replace(&mut args, "--reason", "  <b>Install</b>\n_é%&  ");
        args.extend(
            ["--origin", "42:/bin/a:b", "--origin", "4294967295:/bin/sh"]
                .into_iter()
                .map(OsString::from),
        );
        assert_eq!(
            parse_review(args).unwrap(),
            Review {
                reason: "  <b>Install</b>\n_é%&  ".into(),
                command: vec!["/usr/bin/true".into()],
                working_directory: "/tmp".into(),
                requesting_user: "alice".into(),
                requesting_uid: 1000,
                run_as_user: "root".into(),
                origins: vec![
                    Origin {
                        pid: 42,
                        executable: "/bin/a:b".into()
                    },
                    Origin {
                        pid: u32::MAX,
                        executable: "/bin/sh".into()
                    },
                ],
            }
        );
        assert!(parse_review(valid()).unwrap().origins.is_empty());
    }

    #[test]
    fn rejects_missing_and_empty_required_fields() {
        for (option, name) in [
            ("--reason", "reason"),
            ("--command-arg", "command"),
            ("--working-directory", "working directory"),
            ("--requesting-user", "requesting user"),
            ("--requesting-uid", "requesting UID"),
            ("--run-as-user", "run-as user"),
        ] {
            let mut args = valid();
            let index = args.iter().position(|arg| arg == option).unwrap();
            args.drain(index..index + 2);
            assert!(
                parse_review(args).unwrap_err().contains(name),
                "missing {option}"
            );
            let mut args = valid();
            replace(&mut args, option, "");
            assert!(
                parse_review(args).unwrap_err().contains(name),
                "empty {option}"
            );
        }
        let mut args = valid();
        replace(&mut args, "--reason", " \t\n\u{2003}");
        assert_eq!(parse_review(args).unwrap_err(), "reason must not be empty");
    }

    #[test]
    fn rejects_every_duplicate_singleton() {
        for option in [
            "--reason",
            "--working-directory",
            "--requesting-user",
            "--requesting-uid",
            "--run-as-user",
        ] {
            let mut args = valid();
            args.extend([option, "1"].into_iter().map(OsString::from));
            assert_eq!(
                parse_review(args).unwrap_err(),
                format!("duplicate {option}")
            );
        }
    }

    #[test]
    fn rejects_missing_mode_and_dangling_options() {
        assert_eq!(parse_review([]).unwrap_err(), "expected --review");
        let mut args = valid();
        args[0] = "--other".into();
        assert_eq!(parse_review(args).unwrap_err(), "expected --review");
        for option in [
            "--reason",
            "--command-arg",
            "--working-directory",
            "--requesting-user",
            "--requesting-uid",
            "--run-as-user",
            "--origin",
        ] {
            let mut args = valid();
            args.push(option.into());
            assert_eq!(
                parse_review(args).unwrap_err(),
                format!("missing value for {option}")
            );
        }
    }

    #[test]
    fn rejects_non_utf8_option_names_and_values() {
        let invalid = OsString::from_vec(vec![0xff]);
        let mut args = valid();
        args.extend([invalid.clone(), "value".into()]);
        assert_eq!(parse_review(args).unwrap_err(), "option name is not UTF-8");
        for option in [
            "--reason",
            "--command-arg",
            "--working-directory",
            "--requesting-user",
            "--requesting-uid",
            "--run-as-user",
        ] {
            let mut args = valid();
            replace(&mut args, option, invalid.clone());
            assert_eq!(
                parse_review(args).unwrap_err(),
                format!("value for {option} is not UTF-8")
            );
        }
        let mut args = valid();
        args.extend(["--origin".into(), invalid]);
        assert_eq!(
            parse_review(args).unwrap_err(),
            "value for --origin is not UTF-8"
        );
    }

    #[test]
    fn validates_uid_numeric_limits() {
        for uid in ["0", "4294967295", "0001"] {
            let mut args = valid();
            replace(&mut args, "--requesting-uid", uid);
            assert_eq!(
                parse_review(args).unwrap().requesting_uid,
                uid.parse::<u32>().unwrap()
            );
        }
        for uid in ["-1", "+1", " 1", "1 ", "1.0", "١", "4294967296"] {
            let mut args = valid();
            replace(&mut args, "--requesting-uid", uid);
            assert!(parse_review(args).is_err(), "accepted UID {uid:?}");
        }
    }

    #[test]
    fn rejects_malformed_origins() {
        for origin in [
            "",
            "42",
            ":/bin/sh",
            "42:",
            "0:/bin/sh",
            "-1:/bin/sh",
            "+1:/bin/sh",
            " 1:/bin/sh",
            "١:/bin/sh",
            "4294967296:/bin/sh",
        ] {
            let mut args = valid();
            args.extend(["--origin", origin].into_iter().map(OsString::from));
            assert!(parse_review(args).is_err(), "accepted origin {origin:?}");
        }
    }

    #[test]
    fn request_limit_counts_utf8_bytes_and_accepts_exact_boundary() {
        for multibyte in [false, true] {
            let mut args = valid();
            let used: usize = args.iter().map(|arg| arg.len()).sum();
            let remaining = MAX_DISPLAY_BYTES - used - "--command-arg".len();
            let value = if multibyte {
                "é".repeat(remaining / 2) + &"x".repeat(remaining % 2)
            } else {
                "x".repeat(remaining)
            };
            args.extend(["--command-arg".into(), value.clone().into()]);
            assert_eq!(parse_review(args.clone()).unwrap().command[1], value);
            args.last_mut().unwrap().push("x");
            assert_eq!(
                parse_review(args).unwrap_err(),
                "review request exceeds 16 KiB"
            );
        }
    }

    #[test]
    fn rejects_invalid_requests() {
        let mut missing = valid();
        missing.truncate(missing.len() - 2);
        assert!(parse_review(missing).unwrap_err().contains("run-as user"));
        let mut duplicate = valid();
        duplicate.extend(["--reason", "other"].into_iter().map(OsString::from));
        assert!(parse_review(duplicate).unwrap_err().contains("duplicate"));
        let mut bad_uid = valid();
        let index = bad_uid.iter().position(|x| x == "1000").unwrap();
        bad_uid[index] = "-1".into();
        assert!(parse_review(bad_uid).is_err());
        let mut huge = valid();
        huge.extend([
            OsString::from("--command-arg"),
            OsString::from("x".repeat(MAX_DISPLAY_BYTES)),
        ]);
        assert!(parse_review(huge).unwrap_err().contains("16 KiB"));
        let mut many_empty_args = valid();
        many_empty_args.extend(
            std::iter::repeat_with(|| [OsString::from("--command-arg"), OsString::new()])
                .take(MAX_DISPLAY_BYTES / "--command-arg".len() + 1)
                .flatten(),
        );
        assert!(parse_review(many_empty_args)
            .unwrap_err()
            .contains("16 KiB"));
        let mut origin = valid();
        origin.extend(["--origin", "0:/bin/sh"].into_iter().map(OsString::from));
        assert!(parse_review(origin).is_err());
        let mut unsupported = valid();
        unsupported.extend(["--other", "x"].into_iter().map(OsString::from));
        assert!(parse_review(unsupported)
            .unwrap_err()
            .contains("unsupported"));
    }

    #[test]
    fn password_output_is_exact() {
        let mut fds = [0; 2];
        // SAFETY: fds has space for the two descriptors created by pipe.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // SAFETY: pipe returned two owned descriptors.
        let mut reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        let writer = unsafe { std::fs::File::from_raw_fd(fds[1]) };
        write_password_to_fd(writer.as_raw_fd(), "sëcret").unwrap();
        drop(writer);
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        assert_eq!(output, "sëcret\n".as_bytes());
    }

    #[test]
    fn oversized_password_emits_nothing() {
        let mut fds = [0; 2];
        // SAFETY: fds has space for the two descriptors created by pipe.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // SAFETY: pipe returned two owned descriptors.
        let mut reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        let writer = unsafe { std::fs::File::from_raw_fd(fds[1]) };
        // SAFETY: writer is a valid pipe descriptor.
        let pipe_buf = unsafe { libc::fpathconf(writer.as_raw_fd(), libc::_PC_PIPE_BUF) };
        assert!(pipe_buf > 0);
        assert!(write_password_to_fd(writer.as_raw_fd(), &"x".repeat(pipe_buf as usize)).is_err());
        drop(writer);
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        assert!(output.is_empty());
    }

    fn pipe() -> (std::fs::File, std::fs::File) {
        let mut fds = [-1; 2];
        // SAFETY: fds has room for both descriptors; each is owned exactly once.
        assert_eq!(
            unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) },
            0
        );
        unsafe {
            (
                std::fs::File::from_raw_fd(fds[0]),
                std::fs::File::from_raw_fd(fds[1]),
            )
        }
    }

    #[test]
    fn password_atomic_boundary_uses_bytes_including_newline() {
        for multibyte in [false, true] {
            let (mut reader, writer) = pipe();
            let limit = pipe_atomic_limit(writer.as_raw_fd()).unwrap();
            let password = if multibyte {
                "é".repeat((limit - 1) / 2) + &"x".repeat((limit - 1) % 2)
            } else {
                "x".repeat(limit - 1)
            };
            write_password_to_fd(writer.as_raw_fd(), &password).unwrap();
            drop(writer);
            let mut output = Vec::new();
            reader.read_to_end(&mut output).unwrap();
            assert_eq!(output, [password.as_bytes(), b"\n"].concat());

            let (mut reader, writer) = pipe();
            let error = write_password_to_fd(writer.as_raw_fd(), &(password + "x")).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            drop(writer);
            let mut output = Vec::new();
            reader.read_to_end(&mut output).unwrap();
            assert!(output.is_empty());
        }
    }

    #[test]
    fn rejects_non_pipe_and_invalid_descriptors_without_writing() {
        let path = std::env::temp_dir().join(format!("askpass-non-pipe-{}", std::process::id()));
        let file = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            write_password_to_fd(file.as_raw_fd(), "secret")
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(file.metadata().unwrap().len(), 0);
        let file = std::fs::File::options()
            .write(true)
            .open("/dev/null")
            .unwrap();
        assert_eq!(
            write_password_to_fd(file.as_raw_fd(), "secret")
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            write_password_to_fd(-1, "secret")
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EBADF)
        );
    }

    #[test]
    fn broken_pipe_reports_failure() {
        let (reader, writer) = pipe();
        drop(reader);
        // Rust test executables ignore SIGPIPE, just like the application.
        assert_eq!(
            write_password_to_fd(writer.as_raw_fd(), "secret")
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EPIPE)
        );
    }

    #[test]
    fn full_nonblocking_pipe_emits_no_password_bytes() {
        use std::io::Write;
        let (mut reader, mut writer) = pipe();
        let fill = vec![b'x'; pipe_atomic_limit(writer.as_raw_fd()).unwrap()];
        let mut filled = 0;
        loop {
            match writer.write(&fill) {
                Ok(0) => panic!("pipe made no progress"),
                Ok(n) => filled += n,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("{error}"),
            }
        }
        assert_eq!(
            write_password_to_fd(writer.as_raw_fd(), "secret")
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(writer);
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        assert_eq!(output, vec![b'x'; filled]);
    }
}
