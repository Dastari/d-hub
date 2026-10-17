# d-hub configuration and themes

Use one typed settings registry to describe keys, types, defaults, validation, secrecy, help text and whether a setting needs a restart. The same registry powers the settings TUI, CLI, configuration validation and generated reference. The examples are proposed contracts, not implemented commands.

## Implemented prototype subset

The offline prototype implements a strict flat TOML schema with `schema_version = 1`, `focus` (`all`, `work`, `personal`), `source` (`all`, `discord`, `teams`, `email`, `icloud`, `office365`; default `all`), `theme` (`midnight`, `dusk`), `reduced_motion` (default false), `insert_on_focus` (default true), `padding_horizontal` and `padding_vertical` (integers 0–16, both default 0). Source and focus filters intersect; Email combines iCloud and Office 365. Left/Right cycles sources in navigation mode while retaining cursor movement in insert mode. Existing profiles without `source` use All. Padding applies per side in terminal cells/rows around the complete application layout; internal composer and dialog spacing is separate. Existing profiles that omit padding use zero. If padding leaves less than 28×16 content space, the app shows its resize/reduce-padding fallback. The shared registry in `src/config.rs` powers both the settings TUI (navigation `,`) and `config list|get|set|reset|validate|export`. Use Left/Right to cycle values backwards/forwards in settings; Enter/Space advances. Unknown keys, invalid values and unsupported schema versions are rejected before atomic replacement. Configuration files use private permissions.

`demo` uses a temporary isolated profile by default; `demo --data-dir DIR` reopens a persistent demo profile. Its settings live at `DIR/config.toml`, unless global `--config PATH` is supplied. Configuration commands use `--config PATH` or the normal XDG config location. Close the demo before making CLI configuration edits; concurrent settings coordination is not implemented yet. `config list --json` reports each value's origin as config or default.

The sections below describe the full future contract. The larger `examples/config.toml` and theme example are design fixtures and cannot be loaded by this smaller prototype schema. Profile/environment/value overrides, arbitrary theme files, account settings and the remaining command families are not implemented yet.

## Files and precedence

Use XDG paths: configuration in `$XDG_CONFIG_HOME/d-hub` (normally `~/.config/d-hub`), state/database in `$XDG_STATE_HOME/d-hub`, disposable cache in `$XDG_CACHE_HOME/d-hub`, and user-private runtime socket under `$XDG_RUNTIME_DIR/d-hub`. Keep themes and profile definitions under configuration. Separate live message/draft state from portable settings.

Resolve values in this order, highest precedence first: explicit CLI flags or `--set`, documented environment overrides, selected profile overrides, persisted user configuration, defaults. Report each effective value's origin. `config set` changes persisted values; launch overrides do not. Define which overrides belong to the daemon versus only the current window; reject attempts to silently mutate global settings with a per-window flag.

Use versioned TOML with strict key/type validation, atomic writes and migrations with recoverable backups. Preserve unrelated values when editing. Reject invalid edits as a unit and show the error without corrupting a running configuration. The TUI should show when a CLI override masks a saved value.

## Settings interface

Search by name, description or shortcut. Categories include Accounts, Focus, AI, Composer, Attachments, Navigation, Appearance, Windows, Notifications, Sounds, Privacy and Storage. Categories in settings do not introduce source tabs into the communications feed.

Provide toggles, enum selectors, validated fields, lists, file pickers and per-value reset. Include effective-value inspection, preview/apply/cancel for themes, sound/notification tests, shortcut capture and conflict detection. Account authentication is an explicit flow; secret values are masked and stored outside TOML. Show disabled capabilities and their reasons.

## Proposed command surface

```text
d-hub [drawer] [--profile NAME] [--focus NAME] [--theme NAME] [--set KEY=VALUE]
d-hub demo
d-hub daemon [--foreground]
d-hub preview MESSAGE_ID
d-hub client [mail|teams|discord] [--account ID]       # later full clients
d-hub settings [SECTION]
d-hub config list|get KEY|set KEY VALUE|reset KEY|validate|export
d-hub accounts list|add|auth|status|remove
d-hub focus list|show NAME|set NAME
d-hub themes list|inspect NAME|set NAME
d-hub keys list|check
d-hub notifications test|mute|unmute
d-hub sounds list|test NAME
d-hub doctor [--json]
d-hub completion SHELL
d-hub --help
```

Every persistent feature setting is reachable by `config` even without a dedicated convenience flag. Include `--config`, `--no-color`, `--version` and structured output where relevant. `--json` must not start an interactive TUI. Emit diagnostics to stderr and machine output to stdout; document exit codes and cursor pagination for future message-list commands. Do not add a broad raw network escape hatch merely to satisfy a generic CLI template.

The Omarchy companion plugin uses this same registry and persistence path for application preferences and focus-profile actions. Do not create a second copy of d-hub settings in the plugin. Omarchy owns desktop plugin placement and enabled state; diagnostics report missing/incompatible plugin or backend capabilities without preventing standalone TUI/CLI use.

Credentials use secret-store references, interactive auth or dedicated stdin input. Do not accept raw secrets on argv as the normal setup path. Exports omit secrets and account message data. Removal distinguishes disconnecting an account, deleting cached data and deleting provider content; never make the latter an implicit side effect.

## Configuration domains

| Domain | Examples |
| --- | --- |
| Account | Provider, display name, enabled, credential ref, sync interval, permitted AI fields |
| Feed | Focus, grouping, unread filter, time display, visible fields, backfill window |
| Composer | Focus-follow behaviour, insert-on-focus, reply mode, send shortcut, draft persistence |
| Attachment | Paste policy, staging limits, download location, preview behaviour |
| AI | Decision provider/model, timeout, question set, thresholds, summary provider, budget |
| Input | Keymap, mode, mouse, prefix timeout, terminal capability overrides |
| Appearance | Theme, density, border style, icons, motion, minimum layout |
| Window | Drawer sizing preference, reader launcher, preview reuse preference |
| Alert | Source/sender/focus rules, grouping, quiet hours, preview privacy, active-window policy |
| Sound | Enabled, volume, sound mappings, cooldown, DND behaviour |
| Storage | Retention, cache quota, attachment cleanup, log level/redaction |

These include invariants with configurable behaviour around them: customization must not allow a background event to send a draft to a different recipient. Do not interpret “everything configurable” as permission to disable correctness.

## Theme contract

Themes contain metadata, semantic colours and styles plus border/icon choices. Tokens should cover background, surface, text, muted text, accent, selection, unread, urgent, warning, error, success, draft lock, attachment and source label. Avoid hardcoded provider brand colours. Each style can specify foreground/background and supported modifiers; every important status has a non-colour indication.

Support RGB hex values, Unicode and conservative fallback borders, optional Nerd Font icons, density and reduced motion. Validate tokens and missing glyph fallbacks. Allow import/export and immediate preview without writing until Apply. Theme changes must not reset navigation or draft state.

Optional Omarchy theme following should read the current desktop theme through a version-aware adapter, translate a small semantic palette and fall back to a bundled theme. Never write into packaged Omarchy files or require the development machine to run Omarchy. A stale or missing theme file is recoverable.
