Stable release channel with managed Linux installation and automatic updates.

- Optimized Linux x86_64 (glibc 2.35+) and aarch64 (glibc 2.39+) executables, SHA256SUMS and one-line installer.
- Background release checks every six hours, on frontend launch, and through :update / d-hub update.
- Atomic checksum-verified binary replacement, saved drafts, active-send draining, automatic service handover and managed frontend restart.
- Private SQLite live profiles, persistent replies, account setup, personal Discord QR sign-in and opt-in iCloud/Microsoft mail/Teams transports from the existing preview.

This is an early application release: personal Discord DM synchronization/replies/history, attachment transfers and the Omarchy companion remain unfinished. Live provider and actual desktop acceptance have not been completed by development tools. See README.md, docs/LIVE_SETUP.md and docs/UPDATES.md before enabling accounts. No license has been selected.

Install:

```sh
curl -fsSL https://github.com/dastari/d-hub/releases/latest/download/install.sh | sh
```

If upgrading from the development build before v0.2.1: save and close the drawer, run `d-hub service stop` once, then install and relaunch. Subsequent managed releases use automatic handover.
