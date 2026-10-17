# d-hub

d-hub is a keyboard-first communications drawer for dastari, built for Arch Linux, Omarchy and Ghostty. It is designed to bring unread iCloud mail, Office 365 mail, Teams and personal Discord DMs into one chronological feed with a persistent, context-aware reply composer. Personal Discord history and replies remain an unresolved integration requirement; bot channels do not meet the owner’s needs.

The interface uses open spacing, minimal borders and a consistent colour language across sources, with a slow accent shift and braille activity indicators. It aims for a modern AI-harness feel: Unicode, RGB themes, image previews, attachment chips, a command palette, discoverable shortcuts and a multiline input. Focus profiles filter the shared feed without source tabs. Reduced motion is available.

Deliver d-hub as a standalone Rust application with an optional Omarchy companion plugin. The plugin provides an unread/status indicator, drawer activation and quick focus-profile actions; the communications engine and TUI remain in d-hub. Prototype the plugin immediately after the persistent service and reader, before live providers.

## Project status

The Rust prototype includes a mixed offline feed, locked drafts, source colours, persistent local read/dismissal state, editable focus profiles, a global spam policy and shared TUI/CLI settings. Explicit keyring setup supports iCloud app passwords, Microsoft device login and Jev/OpenAI keys. Typed decision adapters and optional, read-only Discord desktop notification capture are implemented. Mail/Teams message transports, remote unread sync, daemon IPC and the Omarchy companion are pending.

The required Discord experience is personal DMs: new arrivals join the shared chronological feed; opening one reveals full conversation history and allows a quick reply. A full client is the longer-term goal. Discord’s supported bot API cannot supply this personal inbox. The optional notification bridge does **not** satisfy that requirement: it has no history, replies, remote read state or reliable conversation deep link. It remains disabled unless explicitly configured. See [integration limits](docs/INTEGRATIONS.md#discord).

## Run the prototype

Use a current stable Rust toolchain on Linux:

```sh
cargo run --locked -- demo
```

The default demo uses a temporary profile removed on exit. To keep drafts between runs, use a dedicated demo directory:

```sh
cargo run --locked -- demo --data-dir ./demo-profile
```

Every message is fictional and every send is a local simulation. Demo mode never schedules account authentication, notification capture or AI requests. Ctrl+q saves and exits; Escape enters navigation mode, where `?` opens help. In navigation, `j`/`k` or arrows browse, `f` changes focus, `a` adds a fixture attachment, `s` saves-and-switches drafts, `n` simulates incoming mail, and `,` opens settings. Insert mode keeps normal typing as text. Enter inserts a newline; F5 sends a mock reply, with Ctrl+Enter/Alt+Enter where the terminal distinguishes them. F7 cycles success/failure/uncertain outcomes.

In navigation mode, Left/Right cycles the source filter in either direction: All, iCloud, Discord, Teams, Email and Office 365. Email combines both mail providers. The header shows the active source; source, Work/Home focus and search filter the same chronological feed. Locked drafts keep their original recipient and attachments. In insert mode, Left/Right moves the text cursor. The source setting is also available through `config set source all|discord|teams|email|icloud|office365` or the settings TUI.

Each source has a distinct colour (Discord uses blurple). A solid `●` means unread and a hollow `○` means read. Explicit selection marks seen by default. `u` toggles unread, `e` dismisses/restores an item, `D` includes dismissed items, and `S` includes spam matches. Accepted mock replies dismiss their original incoming message; failures and uncertain sends retain it. These states survive reopening a persistent profile. Provider read syncing is queued by revision but has no connected transport yet.

`o` opens the selected message in the in-process reader; Ctrl+o requests a separate Ghostty fixture reader and falls back when Ghostty cannot be launched. The standalone reader can also run in another terminal:

```sh
cargo run --locked -- preview --demo 4
cargo run --locked -- demo --snapshot
cargo run --locked -- doctor --json
```

The fixture reader is read-only and does not use daemon IPC yet. General HTML conversion, shared reader snapshots and actual Ghostty window behaviour belong to milestone 2.

Settings in a persistent demo profile use the same schema and validation as the CLI:

```sh
cargo run --locked -- --config ./demo-profile/config.toml config list --json
cargo run --locked -- --config ./demo-profile/config.toml config set reduced_motion true
cargo run --locked -- --config ./demo-profile/config.toml config set padding_horizontal 2
cargo run --locked -- --config ./demo-profile/config.toml config set padding_vertical 1
```

Outer padding defaults to zero on both axes; values 0–16 apply per side in terminal cells/rows. You can also change it in the settings TUI (Escape, then `,`). Separate fixture readers inherit the drawer's config, or accept an explicit `--config PATH`.

Apply CLI changes while the demo is closed; concurrent settings coordination belongs to the daemon milestone. Without `--config`, configuration commands use the normal XDG config path. The demo defaults to its own isolated settings instead. [Configuration documentation](docs/CONFIGURATION.md) distinguishes the implemented subset from the planned full schema.

## Settings and explicit setup

`cargo run --locked -- settings` opens the shared settings surface. Enter edits a value; Ctrl+s saves, Escape cancels; Left/Right cycles choices. In navigation, `f` cycles All, Work, Home and any added profiles. Commands include `:profile add family`, `:profile prompt family`, `:spam prompt`, `:account add discord home-discord` and `:auth ACCOUNT` / `:auth ai`. The demo permits metadata editing but refuses credential setup.

The same settings work through CLI commands:

```sh
cargo run --locked -- profile add family
cargo run --locked -- config set profiles.family.prompt 'Is this about my family or home?'
cargo run --locked -- config set spam.prompt --file ./spam-prompt.txt
cargo run --locked -- account add icloud home-mail
cargo run --locked -- account auth home-mail
cargo run --locked -- account add office365 work-mail
cargo run --locked -- config set accounts.work-mail.client_id YOUR_PUBLIC_APP_ID
cargo run --locked -- account auth work-mail
cargo run --locked -- config set ai.provider jev
cargo run --locked -- ai-auth
```

Credentials use the desktop Secret Service through `secret-tool` (Arch package `libsecret`); unlock your keyring first. No secret arguments or credentials in TOML. Microsoft requires a registered public-client app and tenant consent. Setup stores credentials; mail/Teams fetching and sending are not connected yet. AI is off globally and per account by default. To opt in, set `ai.enabled true` and `accounts.ACCOUNT.allow_ai true`; only the explicitly launched notification feed schedules real decisions. Demo remains offline regardless of these settings. Attachment bytes are never sent for classification.

For the **optional interim** Discord bridge, keep the official desktop client running with message previews and desktop notifications enabled:

```sh
cargo run --locked -- account add discord home-discord
cargo run --locked -- config set accounts.home-discord.discord_mode notifications
cargo run --locked -- config set accounts.home-discord.notification_sender 'Exact fictional display name'
cargo run --locked -- notifications
```

Configure the actual notification title locally; no Discord login/token is requested. Only matching app/title events are retained, with arrival timestamps and duplicate protection. `o` reads the captured preview; `v` requests the official Discord DM list for history/replies, where you must choose the conversation. Titles are not verified user identities. Capture depends on session-bus monitoring permission, notification format and desktop behaviour; messages that Discord does not notify are unavailable. No bus policy, DND, sound or desktop settings are changed. Capture stops on exit; there is no background service yet. A shared user capture lock prevents duplicate capture windows, even with separate data directories. State contains private message previews under the normal XDG state directory, capped at 2,000 items. Desktop acceptance is pending.

## Development checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 tests/terminal_smoke.py
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
