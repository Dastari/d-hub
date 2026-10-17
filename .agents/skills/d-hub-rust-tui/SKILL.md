---
name: d-hub-rust-tui
description: Build or modify d-hub's Rust Ratatui interface, composer state machine, keyboard modes, terminal images and multiwindow reader. Use for d-hub TUI implementation or rendering issues.
---

# Rust TUI work for d-hub

Read [interaction rules](../../../docs/INTERACTIONS.md) before touching selection, focus or the composer. Read [architecture](../../../docs/ARCHITECTURE.md) when adding IPC, persistence or background tasks.

## Preserve the interaction model

- Selected item, viewed item and reply target are separate values. Send from an immutable target snapshot and draft revision, never from the latest selected row.
- Text or an attachment locks context. Background arrivals and AI results cannot unlock it. Deleting all content does not implicitly retarget.
- Implement transitions as a deterministic reducer with fake-clock/focus/provider tests. Include interleaved arrivals, changed filters, failed sends and window reopening.
- One mixed chronological feed has source labels and filters, not source tabs.

## Work within terminal capabilities

Verify compatible Ratatui/Crossterm/ratatui-image versions before scaffolding. Use [Ratatui docs](https://ratatui.rs/) and [ratatui-image docs](https://docs.rs/ratatui-image/latest/ratatui_image/) rather than copying old API examples.

Keep network, database blocking work and image encoding off the render loop. Subscribe to backend events, coalesce redraws and use synchronized output where supported. Bound event queues and preserve keyboard responsiveness under sync bursts.

Handle terminal restoration on ordinary errors/exits. Test Unicode grapheme editing and display widths rather than indexing UTF-8 bytes as character columns. Support text fallback for image protocols and optional icons.

Navigation and insert modes must be explicit. In insert mode letters are text; Enter is newline. Verify enhanced-keyboard protocol support for modified Enter and provide alternatives. Mouse scrolling and hover do not select a reply target. Leave Super keys to the desktop.

Launch the second Ghostty reader with argv, not a shell string. Connect it to the same backend. Reader movement does not mutate the drawer draft. Test window focus/graphics/clipboard on actual Ghostty; headless test success is not desktop validation.

## Complete the slice

Use the matching [acceptance scenarios](../../../docs/ACCEPTANCE.md), run formatting, build/lints and meaningful state tests, then give the exact demo invocation. Do not require credentials for demo mode or silently enable a desktop service.
