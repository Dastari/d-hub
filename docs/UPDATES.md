# Production installation and updates

Install the latest stable GitHub release on Linux:

```sh
curl -fsSL https://github.com/dastari/d-hub/releases/latest/download/install.sh | sh
```

The installer supports x86_64 and aarch64. It needs curl, coreutils and util-linux/flock, validates SHA-256 checksums, checks that the executable runs and matches the release, then atomically installs it. No sudo, Rust toolchain, account authentication or desktop configuration is required. Existing launchers at ~/.local/bin/d-hub are preserved by default, with instructions for explicit migration. On Arch, install libsecret separately for account credentials; unlock the desktop keyring before setup. Native TLS also needs the system OpenSSL 3 and CA certificates. x86_64 builds target glibc 2.35 or later; aarch64 builds target glibc 2.39 or later. Arch/Omarchy meets these baselines.

The executable lives at $XDG_DATA_HOME/d-hub/bin/d-hub (normally ~/.local/share/d-hub/bin/d-hub), with a managed marker alongside it and a symlink at ~/.local/bin/d-hub. The installer prints a PATH instruction when necessary. Launch d-hub from a terminal; it starts the background owner and opens account setup. Production describes the release channel; the application still has the feature limits listed in README.md, including unfinished Discord history/offline catch-up/remote read markers and attachment transfers.

## Migrating an existing launcher

If the default install reports an existing ~/.local/bin/d-hub, use:

```sh
curl -fsSL https://github.com/dastari/d-hub/releases/latest/download/install.sh | sh -s -- --replace
```

After the new release passes its checksum and version checks, --replace copies the user-owned file or symlink to a unique private ~/.local/bin/d-hub-backup.XXXXXXXX/d-hub, prints that path and atomically installs the managed launcher. Relative and dangling symlinks are backed up as links; their original targets are untouched. A directory or other special file at the launcher path is refused. Failed validation leaves the existing launcher in place without creating a backup. Managed reinstalls do not need --replace and continue retaining the prior binary as d-hub.previous.

To restore a saved launcher after stopping d-hub, move the backup's d-hub entry back to ~/.local/bin/d-hub. Restoring a relative symlink to its original directory restores its original relative target. Configuration, database and keyring entries are separate from these executable backups.

## Checks and replacement

Managed live services check GitHub's latest non-draft, non-prerelease release on startup, every six hours, and when a drawer or reader connects. Checks run outside the owner/render threads and are coalesced for 30 seconds. A temporary network/GitHub rate-limit failure leaves the existing version running; the next periodic or manual request can retry. Only numeric vMAJOR.MINOR.PATCH releases newer than the running version qualify. No automatic downgrade.

Use :update in the drawer, or d-hub update [--socket PATH] from another terminal, for a manual check/install request. CLI returns immediately with check status. Read d-hub service status --json for the current result, running version, available version and pending restart state. Checks/downloads use fixed GitHub endpoints and restricted HTTPS redirects, with size/time bounds. The updater requires GitHub's SHA-256 asset digest and validates binary size, checksum and reported version before replacement. This is integrity verification through GitHub HTTPS metadata, not an independent release-signing system.

Each verified binary is staged in the installation directory, fsynced and renamed over the executable. The prior executable is kept as d-hub.previous. Downloads, validation and permission errors leave the running owner and current executable intact. A curl reinstall also replaces atomically; the next managed service check detects a newer executable already on disk before contacting GitHub.

## Service and open windows

An update advertises a pending handover to all attached drawers/readers. The drawer flushes its queued changes, pauses input, and acknowledges that its drafts are saved. Readers acknowledge without taking the editor lease. Credential/QR setup must finish or be closed before a drawer acknowledges. Older clients that cannot acknowledge defer the restart until they close. No timeout forcibly terminates an active send or discards an unresponsive client's work.

The owner continues recording provider results and waits for active/queued sends to finish. Once every frontend is paused/disconnected and no submission is in flight, it closes SQLite and releases the socket and live-owner locks, then execs the installed binary with the same arguments. Its PID stays the same. Frontends reconnect for up to 30 seconds after their explicit pause; a stable minimal build-info probe lets managed frontends exec the new binary before decoding changed settings or protocol snapshots. Mutating IPC requests are never blindly replayed. Managed frontends then restore terminal modes and exec the installed frontend. A development frontend reconnects without replacing its own binary. Pending/uncertain outbox state retains its existing no-blind-retry rules.

The database, settings, read state and drafts stay in the same private profile. Existing transactional migrations run on new-service startup; unsupported future schemas are refused. The previous binary is retained for diagnosis, but downgrading a migrated database is not automatic. If startup fails, frontends retain cached state and write private recovery files; inspect the new executable with daemon --live --foreground and retain your data before repair. There is no crash rollback supervisor or systemd installation in this delivery.

## Settings, development and removal

The shared auto_update setting defaults true. Change it in the TUI settings or through the live owner:

```sh
d-hub config --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" set auto_update false
```

Disabled automatic updates suppress periodic/startup/frontend network checks. An explicit :update still checks/installs; it also detects an executable replaced by the installer. Source builds and system-package installs without the owned managed marker are never overwritten; manual checks report available versions and installation guidance. Offline demo mode never checks releases, even when auto_update is true.

To remove a managed installation, close the drawer, stop the service with d-hub service stop, remove ~/.local/bin/d-hub and the managed d-hub/bin directory. User configuration, keyring entries and the private state directory are separate and are retained until explicitly removed. The optional Omarchy companion is not shipped yet.

## Publishing another production release

Update Cargo.toml and Cargo.lock together, update docs/RELEASE_NOTES.md, commit and push an annotated version tag such as v0.2.5. The Production release workflow verifies the tag/version, tests both native Linux architectures, builds optimized executables, uploads binaries, SHA256SUMS and install.sh to a draft, then publishes the complete release as latest. A failed build leaves the existing latest release available. The workflow also supports manually dispatching an existing tag. Do not republish changed contents under an existing version.

GitHub's [release API](https://docs.github.com/en/rest/releases/releases) defines latest-release selection and asset digests; the workflow uses [GitHub CLI release creation](https://cli.github.com/manual/gh_release_create) with draft-first uploads.

For an already-running development service from before v0.2.1, bootstrap once by saving/closing its drawer and running d-hub service stop before the first installed launch. Those versions do not implement the new handover protocol.
