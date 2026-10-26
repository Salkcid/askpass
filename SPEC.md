# askpass specification

## Purpose

Build a GTK 4 password dialog for a Wayland desktop, initially tested with niri.
The dialog presents a request as distinct fields so the user can review what is
being authorized. It collects one password and writes it to standard output for
the caller to verify. It does not authenticate users, run privileged commands,
or decide whether a request is safe.

This is a standalone project in `~/projects/askpass`. `approve` is its first
planned caller, but the UI project must not depend on `approve`.

## Structured command-line interface

Install a binary named `askpass`. Its primary interface is a `--review` mode
with separate arguments. The following names describe the intended contract;
implementations may refine spelling before integration, but must preserve the
field boundaries and required/optional distinctions.

```text
askpass --review
  --reason TEXT
  --command-arg ARG [--command-arg ARG ...]
  --working-directory PATH
  --requesting-user NAME --requesting-uid UID
  --run-as-user NAME
  [--origin PID:EXECUTABLE ...]
```

- Require a nonempty reason, at least one command argument, a working directory,
  requesting user and UID, and target user. `--origin` is repeatable, ordered
  nearest ancestor first, and optional because process inspection can fail.
- Accept option values as separate argv elements, including empty command
  arguments and values beginning with `-`. Never join command arguments into a
  shell command for execution or parse them with a shell.
- The reason is caller-provided. The command and other facts are passed by the
  invoking program; `askpass` cannot independently verify them. Preserve this
  distinction in the labels. In `approve` integration, `approve` must collect
  the working directory, user identity, and ancestry independently of the
  reason supplied by its caller.
- Accept UTF-8 display values. The integrating caller must render any non-UTF-8
  path or argument bytes into an unambiguous escaped display form before passing
  them, while retaining the original bytes for command execution.
- Reject duplicate singleton fields, missing values, malformed UIDs/origins,
  unsupported options, and oversized total input before opening a window.
  Bound the complete request to 16 KiB of display text, and show a clear error
  on standard error without exposing a password.

A future generic single-prompt mode may be added for other askpass callers.
The structured `--review` mode is required for the `approve` integration.

## Dialog

- Use GTK 4 through the Rust `gtk4` bindings. Use its Wayland backend and fail
  clearly if no Wayland display is available; do not silently use X11.
- Open one compact, resizable normal Wayland toplevel window with a stable
  application ID so niri can match it in a window rule. Test a floating window
  before considering layer shell.
- Title the window `Administrator access requested`. Show the fields in a
  readable hierarchy:
  1. The command in one unlabeled, horizontally scrollable line, prefixed with
     `sudo`. Show ordinary arguments without quotes; escape whitespace,
     backslashes, and control characters within arguments, and display empty
     arguments as `\0`.
  2. The reason supplied by the caller in a separate unlabeled block.
  3. One block with `Working directory: PATH`, `Requesting user: NAME (UID)`,
     and `Run as: NAME` on separate lines.
  4. `Origin observed by caller` with ordered PID/executable entries, or an
     explicit unavailable message.
- Display all values literally, except for the command's documented display
  escaping. Do not interpret them as Pango markup, HTML, shell syntax, or
  format strings. Preserve line breaks in the reason. Visually
  separate labels from values so an argument cannot masquerade as a field label.
- Size the initial window to its content, up to a bounded review area that
  scrolls for long requests. Keep the password field and actions visible. Never
  silently truncate the command, reason, or required facts. Make content
  selectable for inspection.
- Use `GtkPasswordEntry`, focused when the window opens. Provide Cancel and
  Authorize buttons. Enter submits only when the field is nonempty; Escape and
  window close cancel. The final button press means submit this password to the
  caller; authentication still occurs outside the dialog.
- Respect system text scaling and use GTK's dark theme variant for the dialog,
  including its text, controls, and focus indicators. Any project CSS must
  preserve contrast and visible keyboard focus.
- The compositor controls final placement and focus. Document a niri
  `open-floating true` rule for the app ID if testing shows it is needed.

## Password output and failures

- On Authorize, write only the entered password followed by one newline
  directly to standard output, then exit successfully. Never print review data
  or status text to standard output.
- Require stdout to be a pipe. Reject a password whose UTF-8 bytes plus newline
  exceed the pipe's atomic write limit, then emit both with one write.
- On Cancel, Escape, window close, validation failure, or UI startup failure,
  write no password bytes and exit nonzero. Diagnostics may go to standard
  error, but must never contain the password.
- Treat stdout write failure as failure. Do not claim successful submission if
  the password did not reach the caller.
- Keep the password in process memory only as long as needed. Do not put it in
  files, logs, command arguments, environment variables, clipboard, or
  telemetry. Avoid unnecessary copies where GTK permits. Mask the field and
  disable any reveal control by default.

## sudo and approve integration

Sudo's `SUDO_ASKPASS` interface launches the configured executable with **one
prompt argument**; it cannot pass `--reason`, `--command-arg`, and the other
fields directly. See the [sudo askpass interface](https://man.archlinux.org/man/sudo.conf.5.en).

Keep `approve-askpass` as a small adapter beside `approve`:

1. `approve` collects the structured request and observed facts before invoking
   sudo. It serializes a versioned, bounded payload containing the separate
   fields, then passes that payload through `sudo -p`. Use an encoding safe from
   sudo's `%` prompt substitutions, such as base64url over a versioned JSON
   object. No password is included.
2. Sudo invokes `approve-askpass` with that one prompt argument. The adapter
   validates the version and payload, rejects malformed or excessive data,
   and starts `askpass --review` with separate argv values. It must not use a
   shell or re-derive facts from a lossy formatted prompt.
3. The adapter replaces itself with `askpass` or directly pipes its stdout to
   sudo without inspecting, modifying, storing, or logging the password.
   Cancellation and failure must propagate as nonzero exits.

The payload and displayed fields are information from `approve`, not proof of
what sudo will ultimately execute. Keep the same argument array for `approve`'s
command display and sudo execution. The integration must document how it finds
the installed `askpass` executable and fail before launching sudo if it is
missing or nonexecutable. It must keep `SUDO_ASKPASS` scoped to this sudo
invocation and retain `sudo -k -A` so cached credentials do not skip the dialog.

## Trust boundary

Any program able to run `askpass` can supply its own review fields. The dialog
must not claim to have verified the caller's identity, command safety, or
ancestry. Only `approve`'s own collection step distinguishes its observed facts
from the caller's explanation. The README must state this limitation plainly.

## Acceptance checks

- Each field appears separately; the command and caller's reason appear first,
  without headings, and the remaining facts retain labels. An argument
  containing newlines, quotes, ampersands, angle brackets, underscores, percent
  signs, and non-ASCII text cannot change the layout or impersonate another field.
- Command argument boundaries survive from `approve` through the adapter to
  the GTK display, including empty strings and arguments beginning with `-`.
- Authorize emits exactly the entered password and one newline on stdout.
  Cancel, Escape, and window close emit no stdout and exit nonzero.
- Missing, duplicate, malformed, and oversized fields fail before any window
  opens. An unavailable Wayland display fails cleanly.
- In niri, the window opens visibly, receives keyboard focus, and remains
  usable with a long request at normal and increased display scaling.
- With integration, cancellation prevents a harmless test command from
  running; successful authentication runs it. Check manually with a
  password-requiring sudo policy, including cached credentials.
