# Test coverage

- [x] Submission and cancellation: Authorize, Enter, empty password, Cancel,
  Escape, window close, exit status, exact stdout, and failed submission.
- [x] Output failures and boundaries: non-pipe stdout, broken/full pipes,
  atomic write limits, and multibyte passwords.
- [x] Parser validation: required and duplicate fields, malformed arguments,
  non-UTF-8 input, numeric limits, and complete ordered results.
- [x] Request size: exactly 16 KiB, one byte over, and UTF-8 byte counting.
- [x] Startup and display: subprocess failure contracts, command escaping,
  literal GTK labels, and long-content usability.
- [x] Run formatting, tests, and lint checks; document any display prerequisites.

GTK coverage runs explicitly on Wayland; see README.md. Compositor key routing,
placement, and final keyboard focus remain manual desktop acceptance checks.
