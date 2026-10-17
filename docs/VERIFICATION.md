# Prototype verification · 17 October 2026

Scope: offline fictional messages and local mock sends only. No real accounts,
provider requests, desktop configuration changes or user services were used.

## Environment and dependencies

Verified on Linux with Rust 1.97.1 and Cargo 1.97.1. Cargo.lock records the exact
dependency graph. Ratatui 0.30.2 and Crossterm 0.29.0 are compatible according to
the [Ratatui installation guide](https://ratatui.rs/installation/); `cargo tree -i
crossterm` confirms one Crossterm version. The manifest declares Rust 1.88 as the
minimum, but that compiler has not been independently tested.

Terminal image support was investigated through the ratatui-image crate metadata;
it is not included in this slice. Attachments currently show fixture metadata and
text controls. Real Kitty graphics and clipboard/file staging are pending.

## Automated checks

- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked`: 36 tests passed.
- `cargo build --release --locked`: passed; release `--help` and `doctor --json`
  reported the offline command surface and pending capabilities correctly.
- `python3 tests/terminal_smoke.py`: passed against the real debug executable in a
  Linux pseudo-terminal, using only temporary fixture profiles.
- `cargo run --locked -- demo --snapshot`: rendered a mixed-source frame without
  credentials or an interactive terminal.

Composer coverage includes C01–C07 and C09 using fake focus/arrival events,
attachment-only locking, clearing all content without releasing the lock,
selection/filter interleavings, frozen send targets and revisions, wrong/stale
send results, failed sends, uncertainty after interrupted submission, duplicate
submission prevention, saved draft restoration, historical/outgoing exclusion
and no-eligible-target handling. C08 is covered for failed sends and source
disappearance; real account disconnection/recovery is not implemented. C10–C14
have local model coverage where applicable; daemon multi-client delivery and
provider reconciliation are still pending.

Input/render checks cover literal navigation characters in insert mode,
grapheme-based deletion and cursor movement, CJK/emoji/combining marks, ANSI/OSC
control stripping, both palettes, reader isolation and resizing down to a 1×1
fallback. The prototype's minimum interactive layout is 28×16; smaller sizes show
a resize message and retain Ctrl+q. Narrow layouts retain the fixture recipient
and account. These establish headless rendering behaviour, not physical terminal
glyph or pixel fidelity.

Persistence checks cover private 0600 atomic snapshots, one writer per demo
profile, malformed-state rejection without overwriting, unsupported schema
rejection and draft restoration after reopening. Settings tests exercise the
shared TUI/CLI registry, valid round trips, atomic rejection of invalid edits,
reset/export and config/default origin reporting. No SQLite migrations exist yet.

Outer padding defaults to zero. Checks cover loading older profiles without the
new fields, CLI round trips and rejection of invalid padding, rendering the
configured inset on both axes, the small-content fallback, visibility of padding
controls in settings, and preservation of a locked draft during layout changes.

The PTY smoke test additionally drives actual keyboard routing: bracketed Unicode
paste, fixture attachment, selecting another message, arrival and filter changes,
reader opening/scrolling/closing, resize, quit/reopen, failed and uncertain mock
sends, retry blocking, explicit mock reconciliation, standalone fixture reader
and terminal mode restoration on exit.

## Desktop and later milestones still pending

No actual Ghostty/Omarchy desktop was available for validation. Ghostty spawning
uses an argv array and falls back to the in-process fixture reader if spawning
fails; successful second-window presentation, focus/visibility signals, modified
Enter keys, graphics and terminal cleanup on that desktop remain unverified.
The PTY tests use F5 as the unambiguous mock-send shortcut.

Daemon IPC, SQLite/outbox migration and recovery, shared reader snapshots,
Wayland rich paste, attachment staging/upload, general HTML-to-terminal
conversion, live providers, AI, desktop notifications/sounds/DND and the Omarchy
companion plugin are not implemented. Acceptance scenarios for those capabilities
are not passed by the prototype. The fixture reader never takes a draft lease or
changes the drawer's target, but it is not yet a shared-backend reader.
