#!/usr/bin/env python3
"""Default-launch regression: empty isolated config, no keyring or provider access."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
from terminal_smoke import BINARY, Terminal


def main():
    with tempfile.TemporaryDirectory(prefix="d-hub-startup-") as temporary:
        directory = Path(temporary)
        runtime = directory / "runtime"
        runtime.mkdir(mode=0o700)
        overrides = {
            "XDG_RUNTIME_DIR": str(runtime),
            "XDG_STATE_HOME": str(directory / "state"),
            "XDG_CONFIG_HOME": str(directory / "config"),
        }
        previous = {key: os.environ.get(key) for key in overrides}
        os.environ.update(overrides)
        terminal = None
        socket = runtime / "d-hub/service-live.sock"

        def cli(*arguments, check=True):
            return subprocess.run(
                [str(BINARY), *arguments], capture_output=True, text=True, check=check
            )

        def stop():
            cli("service", "stop")
            # Stop acknowledges before the owner drops its socket guard. Wait for
            # that cleanup before testing a missing-service diagnostic/restart.
            deadline = time.monotonic() + 5
            while socket.exists():
                assert time.monotonic() < deadline, "stopped service did not remove its socket"
                time.sleep(0.01)

        def snapshot():
            return json.loads(cli("service", "snapshot", "--json").stdout)

        try:
            terminal = Terminal([])
            terminal.wait(lambda: b"Set up accounts" in terminal.output, "automatic first-launch setup")
            first = snapshot()
            assert first["live"] is True and first["session"]["messages"] == []
            inode = socket.stat().st_ino
            database = directory / "state/d-hub/live/service.sqlite3"
            assert database.exists()
            assert socket.stat().st_mode & 0o077 == 0
            assert database.stat().st_mode & 0o077 == 0

            # Create account metadata through the first-launch TUI, without authentication.
            terminal.output.clear()
            terminal.resize(32, 52)
            terminal.wait(lambda: b"\x1b[52;" in terminal.output, "tall narrow settings resize")
            terminal.send(":account add dis\tpersonal\r")
            terminal.wait(lambda: any(a["id"] == "personal" for a in snapshot()["settings"]["accounts"]), "account setup metadata")
            assert snapshot()["settings"]["accounts"][0]["enabled"] is False
            # Copying a documented command in settings opens entry without running it.
            terminal.screen_action(b"\x1b[200~:account add icloud home-mail\r\n\x1b[201~", b"Command")
            assert len(snapshot()["settings"]["accounts"]) == 1
            terminal.send("\r")
            terminal.wait(lambda: len(snapshot()["settings"]["accounts"]) == 2, "pasted iCloud account")
            # A full :command pasted into an already-open command entry works too.
            terminal.send(":\x1b[200~:account add teams work-chat\x1b[201~\r")
            terminal.wait(lambda: len(snapshot()["settings"]["accounts"]) == 3, "pasted Teams account")
            terminal.send(":account add office365 work-mail\r")
            terminal.wait(lambda: len(snapshot()["settings"]["accounts"]) == 4, "typed Microsoft account")
            assert all(not a["enabled"] and not a["allow_send"] for a in snapshot()["settings"]["accounts"])
            terminal.screen_action(b":account add discord cancelled", b"Command")
            terminal.screen_action(b"\x1b", b"PgUp/PgDn")  # Wait for settings before sending another command.
            assert len(snapshot()["settings"]["accounts"]) == 4
            # Enabling an unsigned account must report missing QR setup without looking up a secret or connecting.
            terminal.send(":account enable per\t\r")
            terminal.wait(lambda: snapshot()["settings"]["accounts"][0]["enabled"], "enable account completion")
            terminal.wait(lambda: "QR sign-in" in snapshot()["provider_status"].get("personal", ""), "honest unsigned Discord status")
            terminal.send(":account disable personal\r")
            terminal.wait(lambda: not snapshot()["settings"]["accounts"][0]["enabled"], "disable account")
            terminal.close()
            terminal = None
            assert snapshot()["session"]["messages"] == []  # service survives the drawer
            terminal = Terminal([])
            terminal.wait(lambda: b"LIVE" in terminal.output, "reopen existing live service")
            assert socket.stat().st_ino == inode  # reuse, no second service
            terminal.close()
            terminal = None
            stop()

            # A stale socket does not require a foreground daemon command from the user.
            terminal = Terminal(["drawer"])
            terminal.wait(lambda: b"LIVE" in terminal.output, "automatic restart")
            terminal.close()
            terminal = None
            stop()

            # Connect-only behavior and noninteractive calls cannot create a hidden service.
            terminal = Terminal(["drawer", "--no-start-service"])
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                terminal.pump()
            assert b"service socket unavailable" in terminal.output
            terminal.abort()
            terminal = None
            assert not socket.exists()
            result = cli(check=False)
            assert result.returncode != 0 and "interactive terminal" in result.stderr
            assert not socket.exists()
        finally:
            if terminal is not None:
                terminal.abort()
            cli("service", "stop", check=False)
            for key, value in previous.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
    print("Startup PTY smoke passed: automatic live owner, narrow account completion, typed/pasted setup for all providers, paste waits for Enter, cancellation, reuse/restart and private-state guards.")


if __name__ == "__main__":
    main()
