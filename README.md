# d-hub

d-hub is a keyboard-first communications drawer for dastari, built for Arch Linux, Omarchy and Ghostty. It is designed to bring unread iCloud mail, Office 365 mail, Teams and personal Discord DMs into one chronological feed with a persistent, context-aware reply composer. Personal QR sign-in, new DM reception and text replies are implemented; live owner acceptance, history and remote read synchronization remain pending. Bot channels do not meet the owner’s needs.

Personal Discord [setup](docs/DISCORD_SETUP.md) separates QR sign-in, enabling reception and allowing replies. Existing signed-in accounts need `:account enable ACCOUNT` to start receiving new DMs.

The interface uses open spacing, minimal borders and a consistent colour language across sources, with a slow accent shift and braille activity indicators. It aims for a modern AI-harness feel: Unicode, RGB themes, image previews, attachment chips, a command palette, discoverable shortcuts and a multiline input. Focus profiles filter the shared feed without source tabs. Reduced motion is available.

Deliver d-hub as a standalone Rust application with an optional Omarchy companion plugin. The plugin provides an unread/status indicator, drawer activation and quick focus-profile actions; the communications engine and TUI remain in d-hub. The current priority is an owner-tested live mail/Teams feed; the companion remains a separate desktop milestone.

## Install

```sh
curl -fsSL https://github.com/dastari/d-hub/releases/latest/download/install.sh | sh
```

If an older executable or development symlink already occupies `~/.local/bin/d-hub`, migrate it with a backup:

```sh
curl -fsSL https://github.com/dastari/d-hub/releases/latest/download/install.sh | sh -s -- --replace
```

The installer prints the backup location and leaves symlink targets unchanged. It verifies the release before replacing the launcher.

Then run `d-hub` in your terminal. Linux x86_64 and aarch64 production builds install under your user account, without sudo or Rust. Managed installations check for updates every six hours and when the frontend opens; use `:update` to check manually. Updates save drafts, finish active sends and restart the background service and managed frontends. Settings include `auto_update` (default true). See [installation and updates](docs/UPDATES.md) for requirements, paths and recovery.

## Project status

The shared SQLite service now supports opt-in **real iCloud mail, Microsoft mail, selected Teams chats and personal Discord DMs**, with persistent drafts, background sync/replies, remote mail read updates and optional daemon-owned Jev/OpenAI filtering. Live profiles start empty; demo data stays isolated. Sending and account connections default off. Provider tests use local fake servers; owner account and actual desktop acceptance remain pending. Discord history/offline catch-up, remote read markers, attachments and the Omarchy companion are unfinished.

Start with the [real-account setup guide](docs/LIVE_SETUP.md). It covers authentication, a separate live profile, read sync, safe first replies, send recovery and current limits. Running `d-hub` starts the live service if missing and opens the drawer; empty profiles open account setup. `d-hub demo` remains explicitly offline. [Personal Discord QR setup](docs/DISCORD_SETUP.md) is now implemented; new DM reception/text replies require account enablement; history and remote read markers remain pending.

The first Discord delivery needs new personal DMs in the shared arrival-ordered feed and quick replies. Historical context is deferred to phase 2: opening a chat loads recent context, with older pages fetched on demand. A broader client follows in phase 3. History is not an initial-release gate. Discord’s supported bot API cannot supply this personal inbox. The optional notification bridge does **not** satisfy that requirement: it has no history, replies, remote read state or reliable conversation deep link. It remains disabled unless explicitly configured. See [integration limits](docs/INTEGRATIONS.md#discord).

## Run the prototype

Use a current stable Rust toolchain on Linux:

```sh
cargo run --locked -- demo
```

The default demo uses a temporary profile removed on exit. To keep drafts between runs, use a dedicated demo directory:

```sh
cargo run --locked -- demo --data-dir ./demo-profile
```

Every message is fictional and every send is a local simulation. Demo mode never schedules account authentication, notification capture or AI requests. Ctrl+q saves and exits; Escape enters navigation mode, where `?` opens help. In navigation, `j`/`k` or arrows browse, `f` changes focus, `a` adds a fixture attachment, `s` saves-and-switches drafts, `n` simulates incoming mail, and `,` opens settings. Insert mode keeps normal typing as text. Enter sends a mock reply (F5 too); Shift+Enter, Ctrl+Enter or Alt+Enter insert a newline where the terminal distinguishes them, and Ctrl+J always does. F7 cycles success/failure/uncertain outcomes.

In navigation mode, Left/Right cycles the source filter in either direction: All, iCloud, Discord, Teams, Email and Office 365. Email combines both mail providers. The header shows the active source; source, Work/Home focus and search filter the same chronological feed. Locked drafts keep their original recipient and attachments. With the default `reply_first = true`, Up/Down saves the current message’s reply and restores the next one while staying ready to type; Left/Right cycles sources and prepares a visible reply. Alt+arrows moves the text cursor, including multiline movement. Each incoming message has its own draft, even within the same conversation. Source combinations with no messages park the draft and clear the recipient. Pending/uncertain sends block reply switching. Escape still provides explicit browsing/command mode. Set `reply_first false` for the original insert-mode cursor arrows and explicit `s` save-and-switch workflow. The source setting is also available through `config set source all|discord|teams|email|icloud|office365` or the settings TUI.

Each source has a distinct colour (Discord uses blurple). A solid `●` means unread and a hollow `○` means read. Explicit selection marks seen by default (a whole chat stack at once). `u` toggles unread, `e` (or Ctrl+E while typing) dismisses/restores an item or a whole chat stack without replying, Delete (before typing a reply) dismisses and marks read, PageUp/PageDown scroll the preview, `D` includes dismissed items, and `S` includes spam matches. Accepted replies dismiss their original incoming message and the older messages stacked with it (newer arrivals stay); failures and uncertain sends retain it. These states survive reopening a persistent profile. The offline demo queues fictional read intent; live iCloud/Microsoft mail use real revision-checked transports.

`o` opens the selected message in the in-process reader; Ctrl+o requests a separate Ghostty fixture reader and falls back when Ghostty cannot be launched. The standalone reader can also run in another terminal:

```sh
cargo run --locked -- preview --demo 4
cargo run --locked -- demo --snapshot
cargo run --locked -- doctor --json
```

The `preview --demo` reader still uses immutable fixtures. The shared service reader below reads current stored messages and synchronizes seen state without changing the drawer target. Mail HTML is converted to safe terminal text; actual Ghostty window acceptance remains pending.

Settings in a persistent demo profile use the same schema and validation as the CLI:

```sh
cargo run --locked -- --config ./demo-profile/config.toml config list --json
cargo run --locked -- --config ./demo-profile/config.toml config set reduced_motion true
cargo run --locked -- --config ./demo-profile/config.toml config set padding_horizontal 2
cargo run --locked -- --config ./demo-profile/config.toml config set padding_vertical 1
```

Outer padding defaults to zero on both axes; values 0–16 apply per side in terminal cells/rows. You can also change it in the settings TUI (Escape, then `,`). Separate fixture readers inherit the drawer's config, or accept an explicit `--config PATH`.

Apply CLI changes while the demo is closed; concurrent settings coordination belongs to the daemon milestone. Without `--config`, configuration commands use the normal XDG config path. The demo defaults to its own isolated settings instead. [Configuration documentation](docs/CONFIGURATION.md) distinguishes the implemented subset from the planned full schema.

## Shared offline service and reply-first drawer

Start the service in one terminal, then attach the drawer in another. All data and sends remain fictional; this does not enable real account connections or install a service:

```sh
cargo run --locked -- daemon --demo --foreground --data-dir ./service-demo --socket ./service-demo/run/service.sock
```

```sh
cargo run --locked -- drawer --socket ./service-demo/run/service.sock
```

The service creates its private profile and socket directories. Closing the drawer leaves it running. Only one drawer owns the editor; additional terminals may open readers. Ctrl+o requests a Ghostty reader attached to the same socket, with in-process fallback on launch failure. Manual reader and diagnostic commands:

```sh
cargo run --locked -- preview --socket ./service-demo/run/service.sock 4
cargo run --locked -- service --socket ./service-demo/run/service.sock status --json
cargo run --locked -- config --socket ./service-demo/run/service.sock set padding_horizontal 0
cargo run --locked -- config --socket ./service-demo/run/service.sock set reply_first true
cargo run --locked -- service --socket ./service-demo/run/service.sock stop
```

Without `--socket`, drawer/service commands use `$XDG_RUNTIME_DIR/d-hub/service-live.sock`. So do config/account/profile commands without `--config`; they refuse while a live profile exists but its service is stopped. `d-hub account test ID` runs a read-only connection check. The demo daemon defaults to service-demo.sock; use an explicit demo socket as above. Existing demo JSON in the selected data directory is imported once and retained as a backup; SQLite then owns session state, settings, revisions and the mock outbox. Initial settings come from the profile TOML or explicit global `--config`. While using the service, edit settings through the drawer or `config --socket`; standalone TOML changes do not overwrite service-owned preferences. `config --socket … export` produces portable TOML/JSON through the same typed schema.

Acknowledged edits survive restart. An interrupted submission becomes uncertain and cannot automatically resend. If the socket disconnects, the drawer freezes edits/sends and writes unacknowledged work to a private recovery file, whose path is shown on exit. After restarting the service, use `drawer --socket PATH --recover RECOVERY_FILE` to restore unsent active/saved drafts. Recovery retains the file, refuses conflicting drafts and unresolved sends, and never replays a send or restores settings implicitly.

This is the persistent-service foundation of milestone 2. It uses transactional JSON domain snapshots inside SQLite plus a separate mock outbox and bounded change ledger. Schema 2 also stores live/demo identity, normalized account-scoped provider records and durable local state. Live adapters and daemon AI ownership are available separately through `daemon --live`; attachment bytes, standalone notification migration, automatic reconnect, systemd setup and target-desktop acceptance remain pending.

## Settings and explicit setup

`cargo run --locked -- settings` opens the shared settings surface. Settings are split into an overview, global sections (General, Feed & composer, Focus profiles, AI decisions, Filtering) and Accounts. Accounts shows each connection's status and unread count; each account page offers rename, sign in again, connect/disconnect, reply/AI permissions, provider-specific fields, statistics and confirmed removal. + Add account suggests a free ID (`discord`, `discord-2`, …), so several accounts of one provider are easy; display names keep them distinct in the feed. Escape goes back a page; `q` closes. Settings fill the available height: wider windows align names and values in columns, narrow windows stack them, and wrapped help stays with each field. Section headings, names, values and notes have distinct colours; the selected field has a continuous background. PageUp/PageDown, Home/End, mouse wheel and row clicks complement arrows/j/k. Enter edits a value in its own row, without leaving the page; Enter saves single values, Ctrl+s saves multiline prompts, Escape cancels; Left/Right cycles choices. In navigation, `f` cycles All, Work, Home and any added profiles. Commands include `:profile add family`, `:profile prompt family`, `:spam prompt`, `:spam person`, `:spam for-you`, `:about`, `:account add discord home-discord` (ID optional), `:account rename ID NAME`, `:account remove ID`, `:accounts` and `:auth ACCOUNT` / `:auth ai`. The demo permits metadata editing but refuses credential setup.

Open commands directly with `:` in settings; from the reply composer press Escape, then `:`. Tab/Shift+Tab completes matching commands, providers and configured account/profile IDs. For example, type `:account add dis`, press Tab, type `personal`, then Enter. You can also paste the complete `:account add discord personal` into settings or command entry; it waits for Enter. Long command input scrolls in narrow drawers, and adding an account opens its settings with a sign-in hint. Escape cancels command entry and returns to settings.

The same settings work through CLI commands:

```sh
cargo run --locked -- profile add family
cargo run --locked -- config set profiles.family.prompt 'Is this about my family or home?'
cargo run --locked -- config set spam.prompt --file ./spam-prompt.txt
cargo run --locked -- account add icloud home-mail
cargo run --locked -- account add icloud --name 'Old address'   # ID icloud, then icloud-2, …
cargo run --locked -- account rename home-mail 'Home mail'
cargo run --locked -- account auth home-mail
cargo run --locked -- account add office365 work-mail   # starts with the built-in work app ID and tenant
cargo run --locked -- account auth work-mail
cargo run --locked -- account test work-mail
cargo run --locked -- config set ai.provider jev
cargo run --locked -- ai-auth
```

Credentials use the desktop Secret Service through `secret-tool` (Arch package `libsecret`); unlock your keyring first. No secret arguments or credentials in TOML. Microsoft requires a public-client app registration and tenant consent; new Microsoft accounts default to the owner's work registration (docs/LIVE_SETUP.md). The live service connects explicitly enabled mail/Teams accounts. Personal Discord setup uses a QR screen and keyring storage; DM transport remains pending. AI is off globally and per account by default. To opt in, set `ai.enabled true` and `accounts.ACCOUNT.allow_ai true`; the live daemon and explicitly launched notification feed schedule real decisions. Demo remains offline regardless of these settings. Attachment bytes are never sent for classification.

For the **optional interim** Discord bridge, keep the official desktop client running with message previews and desktop notifications enabled:

```sh
cargo run --locked -- account add discord home-discord
cargo run --locked -- config set accounts.home-discord.discord_mode notifications
cargo run --locked -- config set accounts.home-discord.notification_sender 'Exact fictional display name'
cargo run --locked -- notifications
```

Configure the actual notification title locally; no Discord login/token is requested. Only matching app/title events are retained, with arrival timestamps and duplicate protection. `o` reads the captured preview; `v` requests the official Discord DM list for history/replies, where you must choose the conversation. Titles are not verified user identities. Capture depends on session-bus monitoring permission, notification format and desktop behaviour; messages that Discord does not notify are unavailable. No bus policy, DND, sound or desktop settings are changed. This standalone bridge stops on exit; it is not yet integrated into the shared live daemon. A shared user capture lock prevents duplicate capture windows, even with separate data directories. State contains private message previews under the normal XDG state directory, capped at 2,000 items. Desktop acceptance is pending.

## Development checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 tests/terminal_smoke.py
python3 tests/service_terminal_smoke.py
python3 tests/startup_terminal_smoke.py
python3 tests/update_terminal_smoke.py
python3 tests/install_smoke.py
```

The PTY smoke test uses the debug executable built by `cargo test` (or `cargo build`). It exercises the real event loop with fake data; it does not validate an actual Omarchy desktop.

## Read in this order

1. [Agent handoff](HANDOFF.md) for the next concrete implementation task and unresolved decisions.
2. [Product specification](docs/SPEC.md) for scope and requirements.
3. [Interaction specification](docs/INTERACTIONS.md) for composer state, navigation and preview behaviour.
4. [Architecture](docs/ARCHITECTURE.md) and [integrations](docs/INTEGRATIONS.md).
5. [Configuration and themes](docs/CONFIGURATION.md).
6. [Implementation plan](docs/PLAN.md) and [acceptance scenarios](docs/ACCEPTANCE.md).

[Example configuration](examples/config.toml) and [example theme](examples/themes/dastari-dark.toml) illustrate the intended schema. They are design fixtures, not executable configuration for an existing application.

[Project agent skills](docs/AGENT_SKILLS.md) cover Rust TUI work, iCloud mail, Microsoft Graph, Discord, Omarchy/Ghostty and AI decisions, alongside upstream CLI and OpenAI documentation skills.

## Scope boundary

The owner has authorised implementation and a public repository at [dastari/d-hub](https://github.com/dastari/d-hub). Keep fixtures and demo mode independent of credentials. Live account setup, real sends, desktop bindings and service installation remain separate explicit setup/testing steps. A project license has not yet been selected.
