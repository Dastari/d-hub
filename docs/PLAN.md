# d-hub implementation plan

Deliver vertical slices with an offline demo first. Provider restrictions and desktop integration are independent risks; a blocked Discord capability must not stop the working mail drawer.

Deliver a standalone Rust application with an optional Omarchy companion plugin. The daemon owns accounts, sync, storage, drafts, sends, AI and alert policy; the plugin displays unread/connection status, activates the Ghostty drawer and offers focus-profile actions through the same backend. Keep the drawer and full reader as TUIs. A native QML communications drawer would require a separate product decision.

## Implementation status · 17 October 2026

The foundations and first offline TUI slice are implemented with locked dependencies, a deterministic composer, shared settings registry, fake send outcomes and isolated demo snapshot persistence. The interface follows the owner's minimal-border, shared-colour, animated-accent direction, with a reduced-motion option. A basic read-only fixture reader is available; the SQLite daemon, shared-reader IPC, image graphics and Omarchy companion remain future work. Actual target-desktop acceptance is pending. See [verification record](VERIFICATION.md) before treating milestone exit criteria as passed.

## Milestone 0 Project foundations

Verify Rust/toolchain availability on the development box, read project instructions and choose compatible crate versions. Scaffold the Cargo package, formatting/linting and focused tests. Define IDs, capabilities, composer events, error types and the typed settings registry. Add fake mixed-source messages, a long HTML/plain email and harmless attachment fixtures. Keep all sample people/content fictional.

Exit: `cargo fmt --check`, appropriate clippy checks and tests pass; `d-hub --help` and `d-hub demo` run without credentials. Demo writes to an isolated temporary/profile data directory and cannot send real messages.

## Milestone 1 Drawer and composer

Implement chronological mixed feed, focus selection, previews, multiline input, attachment chips, explicit navigation/insert modes, arrows, mouse, Vim-style movement, command palette and shortcut help. Implement the composer reducer before coupling it to render state. Simulate new arrivals, send success/failure and focus changes. Add two themes with Unicode and truecolor. Prefer open spacing over borders, a consistent semantic palette for all sources, slow colour shifts and braille activity indicators; provide reduced motion.

Exit: acceptance scenarios C01–C09, U01 and K01 pass in tests/demo. The owner can switch to the window, type a reply, browse elsewhere and see that the draft remains correctly targeted.

## Milestone 2 Persistent service and full reader

Add SQLite migrations, private attachment staging, durable drafts/outbox, single daemon ownership and versioned local IPC. Open full-message reader in a second Ghostty window. Add shared event replay, crash/restart recovery and conservative focus handling. Use typed launch arguments and readable HTML-to-terminal conversion.

Exit: restart preserves drafts; two windows cannot duplicate sends/alerts; reader search and long-email scrolling work on actual Ghostty; preview never changes the drawer target. If development is headless, record desktop tests as pending rather than passing them by inspection.

## Milestone 2a Omarchy companion plugin prototype

Immediately after milestone 2, add a small QML companion plugin with a manifest, an unread/connection-status bar widget, drawer activation and quick focus-profile actions. Use fixture data and the existing local IPC contract, directly or through a narrow typed CLI bridge. Keep the plugin source in this project; no remote repository is needed for the prototype. Inspect the installed Omarchy plugin API and Ghostty/Hyprland capabilities before choosing launch, focus and show/hide mechanisms.

Reuse or focus the existing drawer where supported, with a safe launch fallback that connects to the single daemon. Distinguish drawer activation from selecting a reply target. Treat unknown visibility/focus conservatively. Plugin focus-profile changes use the same settings and composer rules as TUI/CLI changes. Do not add provider sessions, database ownership, draft editing, sending or alert emission to the plugin.

Exit: O01–O05 pass on the target desktop with fake messages and stub sends. Repeated activation, plugin reload/disable, shell restart and backend reconnect preserve drafts and do not duplicate the daemon, provider work or alerts. Record Omarchy/Ghostty versions and verified focus behaviour. The demo, drawer, reader and daemon remain usable without the plugin; headless builds leave desktop checks pending without blocking independent core work.

## Milestone 3 Clipboard and attachment lifecycle

Implement Wayland rich paste for files and screenshot/image data, TUI multi-file picker, preview, remove, received-file save/copy and temporary-file lifetime. Support text paste separately. Model provider limits and progress using fake adapters first. An attachment alone must lock context.

Exit: A01–A05 pass on the target desktop; stale clipboard/files and oversized inputs fail without losing drafts. Copy-to-file-manager behaviour is actually demonstrated.

## Milestone 4 First live email adapter

Implement iCloud IMAP/SMTP with secret-store setup, incremental sync, MIME, unread flags, threading, attachments and reply sending. Test protocol translation against a fake server before owner-authorised live smoke testing. Verify sent-copy behaviour and uncertain-send handling.

Exit: real iCloud messages appear once, attachments work, read/dismiss policy is clear, reply is sent with correct identity and references during an explicit live test. SMTP uncertainty never triggers blind retries.

## Milestone 5 Office 365 mail

Add Graph delegated authentication, mail sync, pagination/delta recovery, replies, upload paths and revocation/re-auth flows. Build on the same domain contract while preserving capability differences. Document required app registration and consent.

Exit: second mail account works in the same feed; account-specific sender/recipient and attachment limits remain correct. Token expiry and throttling do not freeze the UI or break iCloud.

## Milestone 6 Settings and CLI completeness

Fill out searchable TUI settings over the common registry; complete config commands, shell completions, structured output, profile overrides, keymap editing/conflicts, theme editing/preview and diagnostics. Add account flows and all implemented options. Registry/schema support begins in milestone 0; this milestone closes coverage gaps.

Exit: every persistent setting is readable/editable through both surfaces, exports omit secrets, invalid values are rejected atomically and theme/key changes preserve drafts.

## Milestone 7 Focus intelligence and summaries

Add Jev and OpenAI Decisions adapters, bounded message/context selection, per-account enablement and typed focus questions. Add independent optional summary generation. Implement cache keys, stale-result rejection, confidence handling and local deterministic fallback. Evaluate thresholds on labelled fixtures and report provider latency/cost observations rather than assumed guarantees.

Exit: AI outage leaves a useful feed; filters never drop a draft's target; stale results cannot replace a newer classification/summary; AI has no send/tool authority.

## Milestone 8 Sounds and Omarchy alerts

Implement the shared alert policy, desktop notification actions, sound mappings, DND adapter, quiet hours, cooldowns, burst grouping and ledger. Add test actions and inactive-window behaviour, and complete Omarchy theme following through the desktop adapter. Build on milestone 2a's drawer activation and plugin lifecycle handling. Provide optional systemd user-service and companion-plugin setup instructions without enabling them silently; shell/plugin lifecycle must not own the daemon's lifetime.

Exit: desktop tests prove DND suppresses sounds and popups, one event alerts once across windows, notification activation preserves a draft and reconnect/backfill is quiet.

## Milestone 9 Teams and feasible Discord support

Resolve owner questions about Teams chats/channels and Discord DMs/server channels. Verify Graph permissions, tenant policy and Teams file-upload semantics before implementing the chosen scope. Implement a Discord bot integration only for authorised resources; personal-account automation is excluded. Make unsupported capabilities visible.

Exit: supported sources join the same feed and context model with honest capability reporting. Record a deliberate product decision for any unsupported personal DM requirement.

## Milestone 10 Complete TUI workspaces

Expand the second-window reader into full mail navigation/composition and supported Teams/Discord history/navigation. Reuse the same service, settings, drafts and attachment paths. Add explicit draft ownership transfer before editing in multiple windows. Define feature scope per client; avoid promising calls, video or parity with official apps by implication.

Exit: user can move from drawer to full workspace and back without losing state or creating duplicate operations.

## Milestone 11 Packaging and release readiness

Package the Rust application for Arch and the optional QML companion plugin as separate installable components. Document their version compatibility, missing-backend diagnostics, installation/upgrades, removal and data migration. Verify the installed Omarchy plugin distribution rules; do not assume adding a plugin installs or compiles the Rust application. Measure performance, test terminal resize/Unicode/accessibility and review logs for leaked credentials/content. Choose license/repository publishing with the owner. Include troubleshooting for Ghostty capabilities, plugin/backend reconnect, clipboard, Graph consent, IMAP and DND.

Exit: install/uninstall leaves user data policy explicit, application/plugin upgrades preserve drafts/settings and recorded target-desktop checks pass. Removing or disabling the companion plugin leaves the application and its data usable. Do not claim a released version before account and desktop acceptance checks have been completed.

## Recommended next agent prompt

> Read AGENTS.md, HANDOFF.md, docs/VERIFICATION.md and docs/PLAN.md. Use the relevant project skills under .agents/skills. Review the existing offline prototype and preserve its composer locks and minimal-border visual direction. Complete actual Ghostty acceptance where available, then implement milestone 2's single background owner, SQLite persistence and versioned IPC. Upgrade the fixture reader to use that shared backend; follow with milestone 2a's Omarchy companion prototype. Do not connect live accounts, send real messages or silently change desktop configuration. Record automated versus actual desktop checks separately.
