use std::ffi::OsString;
use std::io::{self, Write};

pub const MAX_DISPLAY_BYTES: usize = 16 * 1024;

pub fn write_password(writer: &mut impl Write, password: &str) -> io::Result<()> {
    writer.write_all(password.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
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
    let mut display_bytes = 0usize;

    while let Some(option) = args.next() {
        let option = option
            .into_string()
            .map_err(|_| "option name is not UTF-8")?;
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {option}"))?
            .into_string()
            .map_err(|_| format!("value for {option} is not UTF-8"))?;
        display_bytes = display_bytes
            .checked_add(value.len())
            .ok_or("review request is too large")?;
        if display_bytes > MAX_DISPLAY_BYTES {
            return Err("review request exceeds 16 KiB of display text".into());
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
        let mut output = Vec::new();
        write_password(&mut output, "sëcret").unwrap();
        assert_eq!(output, "sëcret\n".as_bytes());
    }
}
