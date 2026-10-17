# d-hub

d-hub is a keyboard-first communications drawer for dastari, built for Arch Linux, Omarchy and Ghostty. It is designed to bring unread iCloud mail, Office 365 mail, Teams and supported Discord conversations into one chronological feed with a persistent, context-aware reply composer.

The interface uses open spacing, minimal borders and a consistent colour language across sources, with a slow accent shift and braille activity indicators. It aims for a modern AI-harness feel: Unicode, RGB themes, image previews, attachment chips, a command palette, discoverable shortcuts and a multiline input. Focus profiles filter the shared feed without source tabs. Reduced motion is available.

Deliver d-hub as a standalone Rust application with an optional Omarchy companion plugin. The plugin provides an unread/status indicator, drawer activation and quick focus-profile actions; the communications engine and TUI remain in d-hub. Prototype the plugin immediately after the persistent service and reader, before live providers.

## Project status

An offline Rust prototype is implemented. It includes a mixed fixture feed, deterministic draft locking, multiline Unicode editing, fixture attachments, simulated arrivals and send outcomes, a basic full-text reader, two palettes and shared TUI/CLI settings. No live providers, daemon, terminal image graphics or Omarchy companion plugin are implemented yet. See [verification notes](docs/VERIFICATION.md) for tested behaviour and pending desktop checks.

## Run the prototype

Use a current stable Rust toolchain on Linux:

```sh
cargo run --locked -- demo
```

The default demo uses a temporary profile removed on exit. To keep drafts between runs, use a dedicated demo directory:

```sh
cargo run --locked -- demo --data-dir ./demo-profile
```

Every message is fictional and every send is a local simulation. The executable has no provider or network client code. Ctrl+q saves and exits; Escape enters navigation mode, where `?` opens help. In navigation, `j`/`k` or arrows browse, `f` changes focus, `a` adds a fixture attachment, `s` saves-and-switches drafts, `n` simulates incoming mail, and `,` opens settings. Insert mode keeps normal typing as text. Enter inserts a newline; F5 sends a mock reply, with Ctrl+Enter/Alt+Enter where the terminal distinguishes them. F7 cycles success/failure/uncertain outcomes.

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
