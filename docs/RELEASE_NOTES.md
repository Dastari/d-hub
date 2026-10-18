Responsive settings for tall drawers and expanded terminals.

- Fill the available window height instead of showing only three settings.
- Accent section headings, clear field names, secondary-colour values and muted help text, with continuous selection shading.
- Align name/value columns in wide windows; stack and Unicode-wrap fields in narrow windows.
- PageUp/PageDown, Home/End, mouse wheel and row selection; keep the selected field visible after resize.
- Preserve active drafts and recipients while browsing/editing settings.

Use :update in a managed installation, or install:

```sh
curl -fsSL https://github.com/dastari/d-hub/releases/latest/download/install.sh | sh
```

For an existing development executable/symlink, add `-s -- --replace` after `sh` to retain a private launcher backup. Pre-v0.2.1 development services need a one-time save/close and `d-hub service stop` before managed launch.

This remains an early application release: personal Discord DM synchronization/replies/history, attachment transfers and the Omarchy companion remain unfinished. Live account and actual desktop acceptance remain owner setup checks. See README.md and docs/UPDATES.md. No license has been selected.
