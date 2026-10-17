---
name: d-hub-omarchy-ghostty
description: Integrate d-hub with Ghostty windows, Omarchy themes and notifications, Wayland clipboard and desktop sounds. Use for desktop integration and target-machine smoke testing, not generic Rust domain code.
---

# Omarchy and Ghostty integration

Read the desktop sections of [integration design](../../../docs/INTEGRATIONS.md) and [interaction specification](../../../docs/INTERACTIONS.md). The development box may be headless; integration runs on the actual owner desktop.

If an installed general Omarchy skill is available, read it before desktop config edits. Otherwise inspect installed commands/config and back up files you change. Keep edits in user-owned configuration. Never edit packaged `/usr/share/omarchy` content, and do not reset desktop defaults to integrate d-hub.

## Window and terminal support

Inspect installed Ghostty help for new-window/command syntax. Spawn by argument array and pass only opaque message IDs. A reader uses the same backend and does not modify the drawer's selected item or locked draft. Keep a new-window fallback when reuse is unsupported.

Check focus reporting, modified Enter keys, Kitty graphics, synchronized output and resize/image cleanup in actual Ghostty. Use [Ghostty docs](https://ghostty.org/docs) and [ratatui-image](https://github.com/ratatui/ratatui-image). Do not claim that arbitrary pixels make Ratatui layout independent of character cells.

## Desktop notifications and audio

Use freedesktop notification D-Bus and d-hub's app identity. Discover installed Omarchy DND IPC from local service code/help instead of guessing command syntax. Apply one suppression policy to popups and custom audio. If desktop DND status is unknown, suppress custom sounds and show degraded integration status by default.

Only the daemon emits alerts. Use event identity/grouping/cooldown to prevent duplicates from multiple windows and reconnect. A notification click opens the exact conversation without discarding another draft. Do not assume arbitrary inline replies are supported by the notification shell.

## Clipboard and theme

Inspect MIME types for file URI lists, file-manager copied-file formats and images. Use [wl-clipboard](https://github.com/bugaevc/wl-clipboard) or compatible native access. Keep exported files and clipboard ownership alive long enough to paste. Text paste and rich paste are separate actions; test the owner's actual file manager and screenshot tool.

Read Omarchy theme colours into semantic tokens with a fallback; never mutate the packaged theme. Leave Super/window-management shortcuts to the compositor. Verify sounds, clipboard, focus and alert actions manually and record terminal/desktop versions.
