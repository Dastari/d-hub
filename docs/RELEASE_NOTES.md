Fix account setup commands and completion.

- Complete account/provider commands, existing account IDs for authentication, and custom profile IDs with Tab/Shift+Tab.
- Accept a full pasted :account add command in settings or command entry; wait for Enter before running it.
- Keep settings visible, return there on cancellation, and select a newly added account with a sign-in hint.
- Scroll long commands in narrow windows and show command errors in settings.
- Preserve reply drafts and treat ordinary composer text as text. From the feed, press Escape then : to enter a command.

Update using :update in a managed installation, or run:

```sh
curl -fsSL https://github.com/dastari/d-hub/releases/latest/download/install.sh | sh
```

For an existing development executable/symlink, add `-s -- --replace` after `sh` to retain a private launcher backup. Pre-v0.2.1 development services need a one-time save/close and `d-hub service stop` before managed launch.

Personal Discord QR setup is available; DM synchronization/replies/history, attachment transfers and the Omarchy companion remain unfinished. Actual provider/desktop acceptance remains an owner setup step. No license has been selected.
