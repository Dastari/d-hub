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

## Personal QR setup and automatic launch · 18 October 2026

The owner explicitly selected personal Discord QR sign-in like Concord, superseding the earlier supported-bot-only project constraint. The implementation is independent; no Concord code/dependency or license change was introduced. Personal DM events, reply delivery and history remain pending and are not reported as connected after sign-in.

The suite now passes **110 tests** with the isolated-bus child convention unchanged. New fake-auth coverage verifies RSA-OAEP-SHA256/SPKI interoperability, nonce proof, strict event order, short/mismatched fingerprints, wrong keys, safe user names, user-ID pinning, profile verification, encrypted ticket exchange, confirmation-before-save, cancellation, remote cancellation, missing heartbeat ACK, CAPTCHA/rate-limit errors and keyring failure without plaintext fallback or credential echo. Personal credential references cannot collide with bot/mail account IDs. All tokens, identities and crypto exchanges in tests are fictional.

Terminal tests decode the actual black/white half-block QR rendering through quircs, check its four-module quiet zone, refuse cropped QR output on small terminals, clear expired codes and preserve the complete existing composer/session across setup/cancellation. UI QR state is absent from serialized Session/recovery data. The worker exposes identities and status, never the session token, through its bounded UI channel.

The startup PTY smoke runs the actual executable with empty isolated XDG configuration/state/runtime directories. Bare launch starts the live service, first-launch account metadata is entered through the TUI, subsequent launches reuse the socket, closing the drawer leaves the owner alive, shutdown followed by launch restarts it, and connect-only/noninteractive invocations do not start hidden services. Existing standalone/shared-service PTY checks still pass. CI includes all three scripts.

Formatting, warnings-denied Clippy, the complete Rust suite and optimized locked build pass. Live Discord WebSocket/TLS compatibility, phone-camera scanning/approval, real Secret Service storage and actual Ghostty remain owner setup checks; the development machine has no secret-tool/Ghostty. No live QR session was generated and no account credential/message was read or sent. The protocol can change or require additional verification. New dependencies: tungstenite 0.30.0 (Rust 1.85), qrcode 0.14.1 (1.67.1), explicit openssl 0.10.81 (1.80) and base64 0.22; quircs 0.10.3 (1.68) is test-only.

## Production installer and update handover · 18 October 2026

Version 0.2.1 passes **115 Rust tests**, formatting and warnings-denied Clippy. Existing standalone, shared-service and automatic-start PTY scripts pass. The new update PTY builds a second executable with a newer package version and an additional settings field unknown to the old frontend, atomically replaces an isolated managed executable, invokes :update and verifies service/frontend exec, unchanged Unicode draft/recipient, continued typing and zero send replay. Its automatic network checks are disabled; installed-version detection precedes any release HTTP request. A minimal build-info probe is independent of IPC version/snapshot decoding. The protocol/frame test verifies it with a deliberately incompatible protocol number.

The update-safe test holds a fictional submission until its failure is durably recorded, preserving the locked target. Demo update commands remain offline and cannot alter drafts. Unit checks reject unstable versions, foreign/missing release assets, corrupt/truncated binaries and unowned/writable/unmarked installations. The shell installer smoke substitutes a local fake curl release and verifies checksum/version, private managed directory, paths with spaces, symlink, reinstall backup, corrupt-download rejection and existing-executable preservation.

All five Python scripts (four PTY plus installer) are in CI and native production release builds. Release CI builds Linux x86_64 on Ubuntu 22.04 and aarch64 on Ubuntu 24.04, and only publishes a complete draft after both jobs succeed. No real credential/login/send, desktop configuration or user-system installation was used. GitHub artifact publication and isolated download smoke are recorded separately after release completion. Unplanned disconnect recovery remains conservative; automatic reconnect is specifically implemented for acknowledged update handovers.

The initial v0.2.0 release attempt passed Rust tests but its Ubuntu 22.04 job lacked Python's tomllib. Nothing was published. Release and main CI now explicitly select Python 3.12 through setup-python v7; v0.2.1 is the fresh production tag.

Production v0.2.1 is published at https://github.com/dastari/d-hub/releases/tag/v0.2.1. Both native jobs and draft-first publishing passed in [release CI](https://github.com/Dastari/d-hub/actions/runs/39097610331); [main CI](https://github.com/Dastari/d-hub/actions/runs/39097610309) also passed. Published binary sizes: x86_64 16,665,304 bytes and aarch64 14,639,024 bytes; GitHub REST reports SHA-256 digests for every asset.

Post-publication smoke used the actual public installer in a temporary HOME/XDG_DATA_HOME and ran its installed x86_64 binary (`d-hub 0.2.1`). A separately compiled older-version fixture then ran a zero-account private live profile with automatic checks disabled. A manual update downloaded the real GitHub production asset, verified GitHub's digest/size and binary version, retained the prior executable, atomically replaced it and execed the new service with the same PID. Its empty messages/accounts and private database survived. The fixture was stopped and all temporary installation/profile data removed. This exercised GitHub release HTTP only; no credentials, real messages, provider access or owner installation occurred. ARM64 build/tests passed natively in CI; the post-publication executable smoke ran on x86_64.

## Existing-launcher installer migration · v0.2.3

An owner install attempt hit v0.2.1's guard for ~/.local/bin/d-hub. The development environment did not contain that owner launcher, so no direct migration was attempted. The corrected installer supports explicit --replace, preserves a unique private backup after checksum/version validation and atomically replaces the launcher. Shell syntax/help and the expanded offline installer smoke passed: user-owned regular file, relative/dangling/directory symlinks with unchanged targets, managed reinstall, corrupt download/version mismatch without backup or launcher changes, directory refusal and argument validation. Rust application behavior is unchanged; Cargo's package version advances for a fresh immutable release.

The v0.2.2 main CI passed, but native ARM64 release CI hit an existing startup-test race: Stop replies before socket-guard cleanup, so the immediate connect-only diagnostic was not always the missing-socket error. The startup smoke now waits for socket removal after acknowledged stops before asserting restart/missing-service behavior. No runtime stop behavior changed. v0.2.2 was not published; the corrected release tag is v0.2.3.

[v0.2.3](https://github.com/dastari/d-hub/releases/tag/v0.2.3) published successfully after both [native release jobs](https://github.com/Dastari/d-hub/actions/runs/39100174929) passed all 115 Rust tests, four PTY scripts, expanded installer smoke and optimized builds. Main CI also passed. Post-publication verification fetched the actual public install.sh and ran sh -s -- --replace with a pre-existing executable in a temporary HOME containing spaces. It verified the original bytes and executable permissions were retained in a private backup, the backup path was printed, the managed symlink was installed and the real downloaded binary reported d-hub 0.2.3. Temporary data was removed; no owner installation, service or account was accessed.

## Responsive settings · v0.2.4

The Rust suite now has 119 passing tests. Added checks cover full-height row use, selected-field visibility across narrow/tall/wide/minimum-size resizes, four distinct heading/label/value/note colours in both themes, full-width selection shading, Unicode grapheme wrapping, bounded long-value previews, page/Home/End dispatch and unchanged draft/recipient state. All four existing PTY scripts and installer smoke pass with fictional profiles; formatting and warnings-denied Clippy pass. TestBackend captures were inspected at 38×65 and 110×44. Actual Ghostty/Omarchy acceptance remains pending; no owner accounts/settings were accessed. The optional headless HTML preview is unavailable because host browser libraries are missing; raster captures were generated from actual terminal cells in an isolated temporary Python environment.

[v0.2.4](https://github.com/dastari/d-hub/releases/tag/v0.2.4) published after both [native release jobs](https://github.com/Dastari/d-hub/actions/runs/39101147876) passed all 119 tests and release checks; main CI also passed. Post-publication verification downloaded the real installer/binary into a temporary HOME and opened standalone settings with an isolated configuration in a PTY. At 110×44 and 38×65 it showed settings beyond the old three-entry limit, emitted RGB escapes and handled Home/End/page selection before restoring terminal modes on exit. The development harness's NO_COLOR environment flag was cleared only inside this test process for the explicit colour check. No user account, running service, desktop configuration or owner settings were accessed.

## Account command repair · v0.2.5

Reproduced the production-source failure in a private standalone settings PTY before changing code: a bracketed paste of :account add discord personal did not open/display command input, and Tab after account changed the input to unrelated :update. The parser accepts typed account commands; missing completion, ignored settings pastes, colon handling and narrow input rendering made setup unreliable.

The Rust suite passes 123 tests. Added coverage checks provider completion/cycling in both directions, dynamic account/profile IDs, preservation of unknown commands and supplied IDs, pasted commands requiring Enter, duplicate/invalid/incomplete setup without settings mutation, cancellation back to settings, selected new-account fields, narrow command scrolling and unchanged locked draft/session. Insert-mode colon pastes remain literal reply text. Formatting and warnings-denied Clippy pass.

All four PTY scripts and the offline installer smoke pass. The startup regression now uses a 32-column window, completes a partial Discord provider, creates all four provider entries through typed/direct-settings/command-entry pastes, checks that paste waits for Enter, confirms accounts/connections/sends remain disabled and verifies cancellation, service reuse/restart and private state. No authentication or provider connection is performed. The update smoke still verifies managed service/frontend handover and retained Unicode draft/recipient without HTTP or send replay. Native production publication and public downloaded-binary checks follow below.

[v0.2.5](https://github.com/dastari/d-hub/releases/tag/v0.2.5) published after both [native release jobs and publication](https://github.com/Dastari/d-hub/actions/runs/39102508257) succeeded. [Main CI](https://github.com/Dastari/d-hub/actions/runs/39102506015) also passed. The optimized build passed locally. Post-publication verification downloaded the actual public installer into a temporary HOME, verified and installed the real x86_64 asset (d-hub 0.2.5), then ran the expanded startup PTY against that installed binary. Partial-provider Tab completion, typed/pasted setup for Discord/iCloud/Teams/Office365, Enter-before-execution, cancellation, service reuse/restart and terminal restoration all passed in the narrow drawer. Account metadata remained disabled with sends off; the empty profile had no real messages. Temporary service/profile/install data was cleaned up. ARM64 was verified natively in release CI; no owner account, authentication, keyring or desktop configuration was accessed.

## Initial personal Discord DM transport · v0.3.0

The owner reported successful account setup but no arrival. Inspection confirmed v0.2.5's Network::sync explicitly rejected Discord: authentication existed without the requested transport. v0.3.0 implements the initial new-DM/text-reply slice, with separate receive/send opt-ins and explicit local-read/history limits. Existing sign-ins are reused; no account is enabled by an update. No real owner credential, QR session, account or Discord message was accessed by development tools.

All 134 Rust tests pass, with formatting and warnings-denied Clippy. New loopback WebSocket/HTTP tests verify personal Identify and pinned READY identity, REST identity before Gateway start, own/guild exclusion, Unicode/control sanitization, reconnect Resume sequence, replay identity, missing heartbeat ACK, bounded queue overflow replay, foreign resume URL rejection and credential/error privacy. Reply tests check fixed channel/message/recipient routing, current group membership, disabled mention parsing, stable bounded nonce, known 403/429 rejection, ambiguous 500/disconnect acceptance and no automatic POST retry. Edit tests fetch one message; deleted/missing edits cannot block a later incoming DM. The live owner fake-transport test verifies durable outbox before send, arrival ordering/deduplication, unchanged time/read state across edits/replays, deleted-message non-resurrection, retained locked/saved drafts, targeted accepted-reply dismissal and unchanged legacy mail cache keys.

All four PTY scripts and the offline installer smoke passed. The startup regression covers account enable/disable completion, persistence and an unsigned Discord account reporting required QR setup before keyring/network access. A transient test failure immediately after PTY resize was not reproducible in three diagnostic reruns; the test now waits for the resized frame before typing and for the settings footer after Esc instead of relying on input timing. The corrected regression passes. Update handover still preserves the Unicode draft/recipient and never replays sends. An optimized locked build passes; native release and public download verification follow below.

Protocol facts were checked against the primary Discord Gateway, message/channel and close-code documentation linked in DISCORD_SETUP.md, plus the already pinned Concord Gateway reference for personal-client v9 facts. No Concord source/dependency was copied. The WebSocket/REST endpoints and resume hosts are restricted, TLS validation remains enabled and loopback URLs exist only in tests. Automated tests establish protocol translation and recovery behavior, not actual Discord personal-client acceptance. Full-stop/expired-session catch-up, remote read markers, attachment transfers, real account delivery/replies and actual Ghostty remain pending owner checks/future slices.

[v0.3.0](https://github.com/dastari/d-hub/releases/tag/v0.3.0) is published. Both [native release jobs and publication](https://github.com/Dastari/d-hub/actions/runs/39105104749) passed all 134 tests, terminal/installer checks and optimized builds; [main CI](https://github.com/Dastari/d-hub/actions/runs/39105102871) is also green. Post-publication verification fetched the actual public installer and x86_64 asset into a temporary HOME, verified its checksum/version (d-hub 0.3.0), and ran the expanded startup PTY against that installed binary. Account creation/completion/pastes, enable/disable persistence, unsigned-account QR-required status, cancellation, private state, owner reuse/restart and terminal restoration all passed. Temporary services/profiles/installations were cleaned up. This exercises GitHub HTTP and synthetic account metadata only; Discord protocol tests use loopback servers. Actual personal-account DM receipt/reply confirmation remains the owner's next setup check: update, enable the signed-in account, wait for connected, then request a new DM. Earlier unseen DMs are not backfilled by this slice.

## Settings pages and multiple accounts

All 140 Rust tests pass, with formatting and warnings-denied Clippy. New tests cover overview/section navigation and Tab cycling, Esc returning to the drawer from the entry page, the add-account form (nothing saved before Create, free-ID suggestions for repeated providers, typed IDs surviving provider changes, duplicate rejection, disconnected/sends-off creation), display names in the feed while notification items stay read-only, rename leaving connection equality/scope/AI fingerprint unchanged and not serializing empty names, Connect/Disconnect, Left never running actions, confirmation-only removal blocked by an uncertain send, selected-row visibility on every page at narrow/tall/wide/minimum sizes, clickable wide navigation and distinct title/label/value/note colours in both themes. A CLI test covers `account add` without an ID, `--name`, `account rename`, atomic rejection and `account list` names. All four PTY scripts pass; the update smoke caught Esc needing to leave first-launch setup in one step, now fixed. TestBackend captures were inspected at 110×34, 120×30, 44×24 and 40×40. No real account, keyring or Ghostty desktop was used.

## Grouped feed previews and readable reply context

The owner's narrow live drawer showed "Direct message" placeholders instead of message text, separate rows for consecutive DMs from one person, a demo label on live Discord and a Discord user ID as the reply recipient. All 145 Rust tests pass, with formatting and warnings-denied Clippy. New feed tests check grouping by current view order (an interleaved provider splits groups in All and merges them under a source filter; a group DM stays separate), bounded three-line previews with ellipses, mail subjects, removal of quoted history, no chat placeholder subjects, no live demo label, click mapping into variable-height rows, and revealing a newly selected message without undoing wheel scrolling. A drawer test checks that the recipient shows the sender's name (with `+ N more` or a mail address) and never the snowflake. All four PTY scripts pass. A 44-column TestBackend capture was inspected. No real account or Ghostty desktop was used.

## Dismiss without replying, stack previews and local dates

The owner asked to dismiss messages without replying, to see every message of a selected chat stack with a time each, and for full dates to appear somewhere, especially for email. Inspection also found that live provider times were formatted in UTC; feed, preview and account statistics now format timestamps in local time. All 151 Rust tests pass, with formatting and warnings-denied Clippy. New tests cover compact/full local time formats against a fixed UTC+11 zone and fixture fallbacks. They also cover stack previews: every message is shown oldest first with its own time and line breaks, other sources are excluded, and the selected message stays visible when the pane overflows, with markers pointing to the reader. Email preview and reader tests check the sender address and full date. Ctrl+E is tested in insert mode: it dismisses a whole chat stack without typing and moves to the next message, and the empty composer follows. Mail dismisses singly, a stack restores as one, and a reply in progress blocks dismissal of its message while another message can still be dismissed without moving the draft. All four PTY scripts and the installer smoke pass. A 46×50 TestBackend capture was inspected. No real account or Ghostty desktop was used.

Follow-up at the owner's request: viewing a chat stack (selection, reply switch, reader, standalone reader) now marks the whole stack read, and an accepted reply dismisses its message plus the older messages stacked with it. All 153 Rust tests pass, with formatting and warnings-denied Clippy. New tests check that selection marks only the stack read and respects the setting. They also check that replying to a middle message leaves the newer one, a same-sender message arriving while a reply is in flight stays unread and visible, and other conversations are untouched. All four PTY scripts and the installer smoke pass. No real account was used.

## v0.4.0 publication

[v0.4.0](https://github.com/dastari/d-hub/releases/tag/v0.4.0) is published (settings pages, multiple accounts, stacked feed previews, Ctrl+E dismissal, local times). The [native release jobs and publication](https://github.com/Dastari/d-hub/actions/runs/39119547020) and [main CI](https://github.com/Dastari/d-hub/actions/runs/39119547172) passed. Locally, all 153 Rust tests, the four PTY scripts and the installer smoke passed before tagging. Post-publication verification downloaded the public installer into a temporary HOME. It installed the x86_64 asset, which reported d-hub 0.4.0, and the startup/account-setup PTY passed against that installed binary. Temporary data was removed. No owner account, keyring or desktop configuration was accessed.

## Provider hardening and read-only connection checks

An audit of iCloud, Office 365 and Teams against their documented contracts found defects likely to break first contact with real accounts. None of these providers has yet been run against a real account. Fixed and covered by offline tests:
- iCloud replies lacked MIME-Version and Content-Type, so Unicode replies could be misread as US-ASCII. They are now text/plain UTF-8; a test parses the wire message back to "Reply 東京".
- The first Office 365 delta filter used a nanosecond RFC 3339 date. It now uses whole seconds with Z, asserted on the wire.
- An expired cursor on a later mail folder restarted every folder and dropped items already fetched. Now only that folder restarts, earlier items and cursors are kept, and Outlook's 400 SyncStateNotFound is handled like 410.
- Device login, token refresh and Graph errors discarded Microsoft's explanation. AADSTS and Graph error codes now reach the status, first line only, bounded and without terminal controls; IMAP login and SMTP refusal text and folder names are included. Service status shows the whole error chain.
- Sends that never connect (DNS, refused, TLS handshake) now fail with the draft retained instead of becoming uncertain. Tested with closed ports for SMTP and Graph.
- Without --socket, `config`/`account`/`profile` edited a seed TOML file the running service never reads. They now use the running live service and refuse while a live profile exists but its service is stopped; an explicit `--config` still edits a seed file. A live CLI test covers all three cases.
- Teams chat IDs pasted percent-encoded from web addresses are decoded.

New `d-hub account test ID` and the account page's Test connection run a read-only check. For iCloud: IMAP sign-in, LIST, EXAMINE of synced folders and SMTP sign-in without sending. For Microsoft: sign-in with granted scopes, folders or the 50 most recent chats with people and IDs, and the exact first sync query. Fake-server tests verify the results, the absence of SELECT/FETCH/STORE or non-GET requests, Graph error text and chat listing; the demo refuses to connect. All 163 Rust tests, formatting, warnings-denied Clippy, the four PTY scripts and the installer smoke pass. Real-account acceptance remains the owner's step; see LIVE_SETUP.md.

## Built-in Microsoft registration and v0.4.1

On the owner's direction, new Office 365 and Teams accounts start with the work app registration used by gema-2026: client ID 67441acc-d050-453d-bfe3-cb6234d1efb1 in tenant afa5c75a-9ba4-40d3-849e-b36efb2fc71f. Both are public identifiers, not credentials; the tenant ID is what Microsoft's public OpenID metadata for firstassist.com.au reports. Other providers keep `organizations` and an empty client ID, existing accounts keep their saved values, and both fields stay editable. A CLI test covers the defaults and an override. Whether that registration allows public client flows and grants the delegated Graph scopes has not been checked; device login will report AADSTS7000218 or AADSTS65001 if not. All 164 Rust tests, formatting, warnings-denied Clippy and the four PTY scripts pass before tagging v0.4.1.

[v0.4.1](https://github.com/dastari/d-hub/releases/tag/v0.4.1) is published. The [release jobs and publication](https://github.com/Dastari/d-hub/actions/runs/39126429637) and [main CI](https://github.com/Dastari/d-hub/actions/runs/39126427568) passed. The public installer, run in a temporary HOME, installed the x86_64 asset reporting d-hub 0.4.1, and the startup/account-setup PTY passed against that binary. Temporary data was removed; no owner account or keyring was accessed.

## Settings values edited in place

At the owner's request, settings no longer replace the page with a full-screen editor for one value. Enter turns the selected row into an input with the terminal cursor in it; the breadcrumb, page list and other rows stay visible. Single values are typed into the value column (or under the label in narrow windows) and save with Enter. Multiline prompts and terms wrap at words inside the row and save with Ctrl+s. Credentials started from an account's Sign in or Store API key row are typed into that row as dots. `:spam prompt` and `:profile prompt ID` open the page holding the row, already editing. Saving still goes through the shared typed settings path, and a rejected value keeps the row editing with the reason shown. New tests cover the page, cursor row, save, cancel and rejection of a single value; prompt wrapping and command routing; masking at wide and narrow widths; and the wrapping helper. All 168 Rust tests, formatting, warnings-denied Clippy and the four PTY scripts pass. One full run had a single library-test failure that six further full runs and five library-only runs did not reproduce; its name was not captured. Not yet checked by eye in Ghostty.

## v0.4.2 publication

[v0.4.2](https://github.com/dastari/d-hub/releases/tag/v0.4.2) is published (settings values edited in place). The [release jobs and publication](https://github.com/Dastari/d-hub/actions/runs/39161268731) and [main CI](https://github.com/Dastari/d-hub/actions/runs/39161268853) passed. Locally, all 168 Rust tests, formatting and warnings-denied Clippy passed before tagging. The public installer, run in a temporary HOME, installed the x86_64 asset reporting d-hub 0.4.2, and the startup/account-setup PTY passed against that binary. Temporary data was removed; no owner account or keyring was accessed.
