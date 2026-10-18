# Prototype verification · 18 October 2026

Scope: offline fictional messages, local mock sends, fake AI outputs and an isolated synthetic D-Bus notification stream. No real accounts, provider requests, owner desktop monitoring, desktop configuration changes or user services were used.

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
- `cargo test --locked`: 76 tests passed. One ignored child test is deliberately invoked by the private-bus wrapper, so it is also exercised without contacting the desktop.
- `cargo build --release --locked`: passed; release `--help` and `doctor --json`
  reported the explicit-setup command surface and pending capabilities correctly.
- `python3 tests/terminal_smoke.py`: passed against the real debug executable in a Linux pseudo-terminal, using only temporary fixture profiles.
- `python3 tests/service_terminal_smoke.py`: passed with a real offline daemon and two PTYs. It tests reply-first arrows, Alt+Left cursor movement, persisted per-message replies, shared reader isolation/seen state, mock completion after the drawer exits, and acknowledged service shutdown.
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
rejection and draft restoration after reopening. Settings tests exercise the shared TUI/CLI registry, valid round trips, atomic rejection of invalid edits, reset/export and config/default/service origin reporting. SQLite import/migration and service transaction checks are described below.

Outer padding defaults to zero. Checks cover loading older profiles without the
new fields, CLI round trips and rejection of invalid padding, rendering the
configured inset on both axes, the small-content fallback, visibility of padding
controls in settings, and preservation of a locked draft during layout changes.

Source filter checks cover navigation Left/Right wraparound, traditional insert-mode cursor movement, reply-first source switching with Alt cursor movement, combined Email sources, intersection with focus, empty results,
inactive follow eligibility, unchanged drafts/submitted revisions, filtered
preview restoration after reopening, and settings compatibility/persistence.
The PTY test also drives real arrow escape sequences and checks persisted source
changes while preserving a locked draft.

The PTY smoke test additionally drives actual keyboard routing: bracketed Unicode
paste, fixture attachment, selecting another message, arrival and filter changes,
reader opening/scrolling/closing, resize, quit/reopen, failed and uncertain mock
sends, retry blocking, explicit mock reconciliation, standalone fixture reader
and terminal mode restoration on exit.

## Feed policy, settings and provider contracts

Source rendering tests verify distinct source colours, Discord RGB (88,101,242), selection tint and filled unread/hollow read dots. Seen state and dismissal persist across snapshots. Accepted replies dismiss the frozen original incoming target even when another row is selected; failures, uncertainty and stale acceptance do not. Dismissed messages can be included and restored without redirecting a draft.

Read-state contract tests cover durable IMAP identity metadata, coalescing local flag changes, ignoring stale acknowledgements, protecting pending intent from incoming flag snapshots, failure retention and restart. This tests a fake transport and durable queue, **not** real iCloud/Graph read synchronisation. Older JSON snapshots lacking new fields and older flat TOML configs remain readable.

Profile/account creation and multiline prompt editing work through shared settings and CLI, including file input, strict rejection of credential fields in TOML, local term fallback, independent global spam exclusions, dynamic setting readback and atomic rejection of invalid thresholds. The TUI masks synthetic credential input and cancels it without touching a draft. Actual keyring storage and Microsoft device login were not exercised; `secret-tool` is absent on the development machine.

Decision tests use a fake transport for both Jev `noul` and OpenAI `predicate` request/response shapes, strict typed answers, global/per-account consent, prompt/model/policy cache fingerprints, stale-result rejection, pending-item release after failure/disconnection and draft preservation. HTTPS clients disable redirects and bound time/response size. No live provider latency, quality, API access or credential roundtrip has been verified.

The notification protocol test starts **its own** `dbus-run-session`, sends fictional Notify method calls and verifies the real monitor sees only the configured app/title. Event identity includes bus ID, sender connection and serial. Domain tests cover duplicate suppression, arrival ordering, sanitised previews, read-only capability and local state reopening. Capture uses an additional shared user ownership lock across data directories; a CLI test proves the second process is rejected before bus access. No official Discord session was accessed. The notification bridge is not evidence of personal history/reply support.

## Reply-first and persistent service checks · 18 October 2026

`reply_first` defaults true. Unit tests verify Up/Down saves/restores distinct replies and attachments while retaining insert mode; Left/Right changes sources and restores an eligible reply; no matches parks the draft with no recipient. Alt+arrows stays in the editor, and ordinary j/k remain text. Separate incoming messages in the same conversation have separate keys. Legacy saved keys normalize from their stored immutable target without losing content. Background arrivals/filter/focus changes retain locks; submitting/uncertain sends block explicit context switching. Traditional mode remains configurable through the shared registry.

Fourteen service integration tests cover atomic SQLite bootstrap and outbox recovery, retained legacy JSON, malformed/future schema rejection without replacement, bounded change-ledger resync, transactional rollback, stale revisions/composer/settings edits, reader write denial, second-editor refusal and lease release, seen-state isolation, service-owned mock completion after editor exit, forced crash/restart with no retry, queued optimistic edit/cursor preservation, CLI service settings/reader/shutdown, explicit draft restoration, conflicting/pending-send recovery refusal and private recovery files. A failed recovery write leaves unsaved text in the window and reports the failure rather than exiting and discarding it.

The process tests use temporary private profiles and local sockets, with fictional data and mock sends only. SQLite stores domain snapshots/settings, a mock outbox and bounded change metadata. They do not validate real provider normalized tables or attachment bytes. Socket and database permissions, same-user ownership checks, handshake versions, frame limits and isolation across live test processes are exercised; a cross-UID attacker test was not run. `rusqlite 0.40.2` uses bundled SQLite; `nix 0.31.3` supplies Linux peer credentials. The manifest’s Rust 1.88 minimum remains untested on that compiler.

## Desktop and later milestones still pending

No actual Ghostty/Omarchy desktop was available for validation. Ghostty spawning
uses an argv array and falls back to the in-process fixture reader if spawning
fails; successful second-window presentation, focus/visibility signals, modified
Enter keys, graphics and terminal cleanup on that desktop remain unverified.
The PTY tests use F5 as the unambiguous mock-send shortcut.

Automatic client reconnect, Wayland rich paste, attachment staging/upload, personal Discord history/replies, generated summaries, outgoing desktop notifications/sounds/DND and the Omarchy companion plugin are not implemented. Live provider ownership, normalized records, mail HTML conversion and core iCloud/Microsoft/Teams transports are now implemented as described below. Explicit account setup, typed decision adapters and opt-in read-only notification capture exist but still need owner-authorised live/desktop testing. Acceptance scenarios for those capabilities
are not passed by the prototype. The fixture reader remains independent; the new `preview --socket` reader uses the shared backend, never takes the editor lease and cannot redirect the drawer draft.

## Live-provider preview · 18 October 2026

The full suite passes **95 tests**, with one notification-bus child test intentionally ignored by the normal harness and exercised by its isolated-bus wrapper. Formatting, Clippy with warnings denied, both standalone/shared-service PTY smoke scripts and a locked optimized build pass. These checks use fictional data, private temporary profiles, loopback fake IMAP/SMTP/Graph servers and stub credentials. No real accounts, device login, keyring secrets, messages, recipients or desktop notifications were accessed.

New coverage: empty live/demo isolation and mode mismatch; schema-1 to schema-2 migration without replacing settings/drafts; durable scoped remote IDs, allocated-but-uncommitted replay and partial updates; background arrivals/read acknowledgements and idempotent restart; outbox persistence before a fake send; acceptance dismissing only the original incoming message; ambiguous/interrupted send locking, owner resolution and no automatic retry; account-identity changes rejecting old drafts/read work; live rejection of fixtures/metadata-only attachments. A real daemon process with zero enabled accounts verifies live status, shared account/profile settings, refresh/shutdown and rejection of a second live profile.

Fake IMAP verifies UID identities, PEEK, bounded metadata, oversized header-only fetch and UIDVALIDITY reset. MIME fixtures verify encoded Unicode headers, multipart alternatives, Reply-To, attachment names, terminal sanitization and reply threading. Fake SMTP distinguishes successful acceptance from disconnect after DATA; response parsing errors cannot be treated as definite rejection. Fake Graph verifies captured-recipient reply JSON, encoded immutable ID path segments, mail read PATCH, exact opaque pagination/checkpoints, foreign-origin continuation rejection, Retry-After, auth expiry, uncertain server failure, Teams chat paths/local read markers/self-message exclusion, refresh-token rotation and configured identity verification. Live TUI tests verify LIVE/SEND labels, disabled-send guarding and blocked mock outcome controls.

The daemon now owns AI work; connected readers/drawers do not schedule classification jobs. Inactive/closed drawers follow latest eligible content only while empty; a connected drawer begins conservatively focused. Per-account workers are bounded and serialized; failures cannot redirect the frozen composer.

The development machine has neither secret-tool nor Ghostty on PATH. Real iCloud TLS/login/read/SMTP/Sent behavior, Microsoft tenant consent/login/delivery, Teams chat access, AI endpoint access/quality and actual Omarchy/Ghostty acceptance remain owner setup steps. SMTP Sent APPEND, provider attachments, archive UI/retention, automatic client reconnect, alerts/DND, rich clipboard/graphics, personal Discord and the companion plugin remain open. The full plan is not complete.

Dependency inspection used current primary crate metadata/docs. Locked additions: imap 2.4.1 (latest stable; imap-proto emits an upstream future-incompatibility notice), native-tls 0.2.18, lettre 0.11.23, mailparse 0.18.0, html2text 0.17.3 and chrono 0.4.45. Lettre/mailparse/html2text declare Rust 1.85; the project's minimum 1.88 compiler has not been separately run. Production mail TLS validates certificates; cleartext loopback endpoints exist only inside protocol tests.
