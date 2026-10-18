#!/usr/bin/env python3
"""Build a next-version fixture; exercise managed hot handover with no HTTP/accounts.
The next binary is compiled from these same sources with only the package version changed.
"""
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import time
import tomllib
import terminal_smoke

ROOT = Path(__file__).resolve().parents[1]


def main():
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    parts = list(map(int, version.split(".")))
    parts[2] += 1
    newer = ".".join(map(str, parts))
    original_binary = terminal_smoke.BINARY
    with tempfile.TemporaryDirectory(prefix="d-hub-update-") as temporary:
        root = Path(temporary)
        source = root / "next-source"
        source.mkdir()
        shutil.copytree(ROOT / "src", source / "src")
        shutil.copytree(ROOT / "fixtures", source / "fixtures")
        # A new snapshot setting is unknown to the old frontend. The minimal build
        # probe must cause exec before that frontend tries to deserialize it.
        next_config = source / "src/config.rs"
        text = next_config.read_text().replace("pub auto_update: bool,", "pub auto_update: bool,\n    pub fixture_update_schema: bool,").replace("auto_update: true,", "auto_update: true,\n            fixture_update_schema: false,")
        next_config.write_text(text)
        (source / "Cargo.toml").write_text((ROOT / "Cargo.toml").read_text().replace(f'version = "{version}"', f'version = "{newer}"', 1))
        (source / "Cargo.lock").write_text((ROOT / "Cargo.lock").read_text().replace(f'name = "d-hub"\nversion = "{version}"', f'name = "d-hub"\nversion = "{newer}"', 1))
        # Share dependencies, but preserve the original executable before the second build.
        managed = root / "managed"
        managed.mkdir(mode=0o700)
        binary = managed / "d-hub"
        shutil.copy2(original_binary, binary)
        binary.chmod(0o755)
        (managed / ".d-hub-managed").write_text("d-hub-managed-v1\n")
        (managed / ".d-hub-managed").chmod(0o600)
        build_env = os.environ | {"CARGO_TARGET_DIR": str(ROOT / "target")}
        subprocess.run(["cargo", "build", "--locked", "--manifest-path", str(source / "Cargo.toml")], env=build_env, check=True)
        candidate = root / "candidate"
        shutil.copy2(ROOT / "target/debug/d-hub", candidate)
        candidate.chmod(0o755)
        # Restore the repository's expected debug executable for all other tests.
        shutil.copy2(binary, ROOT / "target/debug/d-hub")
        runtime = root / "runtime"
        runtime.mkdir(mode=0o700)
        config = root / "config.toml"
        config.write_text("auto_update = false\n")
        environment = {"XDG_RUNTIME_DIR": str(runtime), "XDG_STATE_HOME": str(root / "state"), "XDG_CONFIG_HOME": str(root / "config"), "HOME": str(root / "home")}
        previous = {key: os.environ.get(key) for key in environment}
        os.environ.update(environment)
        terminal_smoke.BINARY = binary
        socket = runtime / "d-hub/service-live.sock"
        daemon_args = [str(binary), "--config", str(config), "daemon", "--live", "--foreground", "--data-dir", str(root / "profile"), "--socket", str(socket)]
        daemon = drawer = None

        def cli(*args, check=True):
            return subprocess.run([str(binary), *args], capture_output=True, text=True, timeout=5, check=check)

        def snapshot():
            return json.loads(cli("service", "snapshot", "--json").stdout)

        def ready():
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if socket.exists():
                    try:
                        return snapshot()
                    except (subprocess.SubprocessError, ValueError):
                        pass
                assert daemon.poll() is None, "daemon exited"
                time.sleep(.04)
            raise AssertionError("service startup timed out")

        try:
            daemon = subprocess.Popen(daemon_args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            ready()
            cli("service", "stop")
            daemon.wait(timeout=5)
            # Seed fictional context directly in this isolated DB; no provider transport.
            fixture = root / "fixtures"
            fixture_config = root / "fixture.toml"
            fixture_config.write_text("auto_update = false\n")
            demo = terminal_smoke.Terminal(["--config", str(fixture_config), "demo", "--data-dir", str(fixture)])
            demo.wait(lambda: (fixture / "demo-state.json").exists(), "fictional fixture seed")
            demo.close()
            session = json.loads((fixture / "demo-state.json").read_text())
            with sqlite3.connect(root / "profile/service.sqlite3") as db:
                db.execute("UPDATE state SET session = ? WHERE id = 1", (json.dumps(session),))
            daemon = subprocess.Popen(daemon_args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            first = ready()
            assert first["update"]["running_version"] == version
            assert first["update"]["managed"] is True
            assert first["settings"]["auto_update"] is False
            drawer = terminal_smoke.Terminal(["drawer", "--socket", str(socket)])
            drawer.wait(lambda: b"Set up accounts" in drawer.output, "managed drawer")
            drawer.screen_action(b"\x1b", b"NAV")
            drawer.send(b"i")
            drawer.send("\x1b[200~Draft survives update 東京\x1b[201~")
            drawer.wait(lambda: (snapshot()["session"]["composer"]["draft"] or {}).get("body") == "Draft survives update 東京", "durable Unicode draft")
            target = snapshot()["session"]["composer"]["draft"]["target"]
            # Atomic replacement simulates a completed, verified download/curl reinstall.
            replacement = managed / "replacement"
            shutil.copy2(candidate, replacement)
            os.replace(replacement, binary)
            drawer.screen_action(b"\x1b", b"NAV")
            drawer.output.clear()
            drawer.send(b":update\r")
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                drawer.pump()
                try:
                    state = snapshot()
                    if state["update"]["running_version"] == newer and state["editor_connected"]:
                        # Same daemon PID after exec; old frontend must also exec the new inode.
                        if os.readlink(f"/proc/{drawer.pid}/exe") == str(binary):
                            break
                except (OSError, subprocess.SubprocessError, ValueError):
                    pass
            else:
                raise AssertionError(f"handover failed: {state.get('update')!r}; {drawer.output[:600]!r}; {drawer.output[-600:]!r}")
            assert daemon.poll() is None
            assert state["session"]["composer"]["draft"]["body"] == "Draft survives update 東京"
            assert state["session"]["composer"]["draft"]["target"] == target
            assert not state["session"]["composer"]["sent"]
            drawer.wait(lambda: b"Set up accounts" in drawer.output, "upgraded frontend rendered")
            drawer.screen_action(b"\x1b", b"NAV")
            drawer.send(b"i!")
            drawer.wait(lambda: (snapshot()["session"]["composer"]["draft"] or {}).get("body", "").endswith("!"), "typing after upgraded frontend reconnect")
            drawer.close()
            drawer = None
            cli("service", "stop")
            daemon.wait(timeout=5)
        finally:
            if drawer is not None:
                drawer.abort()
            if daemon is not None and daemon.poll() is None:
                daemon.kill()
                daemon.wait()
            terminal_smoke.BINARY = original_binary
            for key, value in previous.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
    print("Update PTY smoke passed: installed-version detection, :update, service/frontend exec, Unicode draft/recipient preservation and no send replay; no HTTP/accounts.")


if __name__ == "__main__":
    main()
