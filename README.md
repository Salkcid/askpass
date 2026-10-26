# askpass

`askpass` is a GTK 4 password review dialog for Wayland. It displays a structured
request and writes the entered password, followed by one newline, to stdout only
when Authorize is pressed. Cancellation and errors produce no password output.

Build with `nix develop -c cargo build --release` or from a shell with Rust,
GTK 4 development files, and `pkg-config` installed. The binary is
`target/release/askpass`.

Example:

```sh
askpass --review \
  --reason 'Install updates' \
  --command-arg /usr/bin/true \
  --working-directory /tmp \
  --requesting-user "$USER" --requesting-uid "$(id -u)" \
  --run-as-user root
```

Each `--command-arg` is one argument, including empty strings and strings that
begin with `-`. `--origin PID:EXECUTABLE` may be repeated, nearest ancestor
first. A request can contain at most 16 KiB of display text. All values must be
UTF-8; callers must escape non-UTF-8 bytes without losing their original bytes
for execution.

The dialog shows claims supplied by its caller. It cannot verify the claimed
identity, command, origin, or safety. It does not authenticate the password.
Those jobs belong to the caller and authentication system.

For niri, if the window does not float by default, use a rule for application ID
`org.askpass.Review`:

```kdl
window-rule {
    match app-id="org.askpass.Review"
    open-floating true
}
```
