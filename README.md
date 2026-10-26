# askpass

`askpass` is a GTK 4 password review dialog for Wayland. It displays a structured
request and writes the entered password, followed by one newline, to stdout only
when Authorize is pressed. Cancellation and errors produce no password output.
Standard output must be a pipe. A password whose UTF-8 bytes plus newline exceed
that pipe's atomic write limit is rejected without output.

Build with `nix develop -c cargo build --release` or from a shell with Rust,
GTK 4 development files, and `pkg-config` installed. The binary is
`target/release/askpass`.

To preview the dialog from the development shell:

```sh
nix develop -c cargo run -- --review \
  --reason 'Rebuild the laptop configuration' \
  --command-arg nixos-rebuild \
  --command-arg switch \
  --command-arg --flake \
  --command-arg '/home/qy1n/nixos#laptop' \
  --working-directory "$PWD" \
  --requesting-user "$USER" \
  --requesting-uid "$(id -u)" \
  --run-as-user root | cat >/dev/null
```

The pipe allows the dialog to open and discards any submitted password. This
preview only displays the request; it does not execute the displayed command.

Each `--command-arg` is one argument, including empty strings and strings that
begin with `-`. `--origin PID:EXECUTABLE` may be repeated, nearest ancestor
first. A request can contain at most 16 KiB of option and value text. All
values must be UTF-8; callers must escape non-UTF-8 bytes without losing their
original bytes for execution.

The dialog opens with “Administrator access requested from:” and the working
directory on the next line, followed by the command in a wrapping, selectable
monospace block and then the caller's reason. The target user and requesting
user stay visible; expand Request details to inspect the UID and request origin.
The password field has a persistent label and a native Caps Lock indicator.

The dialog shows claims supplied by its caller. It cannot verify the claimed
identity, command, origin, or safety. It does not authenticate the password.
Those jobs belong to the caller and authentication system.

Niri automatically floats parented dialogs and fixed-size windows. This app
opens a standalone, resizable window, so add this rule for application ID
`org.askpass.Review` to open it floating. The `open-maximized false` setting
also clears any global maximize rule for this window:

```kdl
window-rule {
    match app-id="org.askpass.Review"
    open-maximized false
    open-floating true
}
```

## Testing

Run the parser, password-output, command-formatting, and CLI startup tests with:

```sh
nix develop -c cargo test --all-targets
```

The GTK suite requires a reachable Wayland display and is ignored by the default
test run. From a Wayland desktop session, run:

```sh
nix develop -c cargo test --bin askpass gtk_tests::dialog_contracts -- --ignored --exact --nocapture
```

It opens and closes ten dialogs in isolated child processes using synthetic
passwords. It checks Authorize and Enter, empty input, Cancel, Escape, window
close, oversized passwords, broken output pipes, literal field contents, and
scrolling with long requests at normal and increased text size. Password output
is captured in private test pipes. Keyboard actions are exercised through GTK
signals; compositor key routing, final focus, and placement still need a manual
check in niri.

For formatting and lint checks:

```sh
nix develop -c cargo fmt --check
nix develop -c cargo clippy --all-targets -- -D warnings
```

## License

MIT. See [LICENSE](LICENSE).
