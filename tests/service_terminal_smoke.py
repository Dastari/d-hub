#!/usr/bin/env python3
"""Real daemon + two PTYs, fictional messages only; no owner desktop access."""
import json
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import time
from terminal_smoke import BINARY, Terminal


class Reader:
    def __init__(self, path):
        self.stream = socket.socket(socket.AF_UNIX)
        self.stream.settimeout(3)
        self.stream.connect(str(path))
        self.call({"method": "hello", "role": "reader"})

    def exact(self, size):
        result = bytearray()
        while len(result) < size:
            part = self.stream.recv(size - len(result))
            assert part, "service disconnected"
            result.extend(part)
        return bytes(result)

    def call(self, command):
        message = json.dumps({"version": 2, "command": command}).encode()
        self.stream.sendall(struct.pack(">I", len(message)) + message)
        response = json.loads(self.exact(struct.unpack(">I", self.exact(4))[0]))
        assert response["result"] != "error", response
        return response.get("snapshot", response)

    def snapshot(self):
        return self.call({"method": "poll", "after": 0})


def main():
    with tempfile.TemporaryDirectory(prefix="d-hub-service-pty-") as temporary:
        root = Path(temporary)
        path = root / "service.sock"
        daemon = subprocess.Popen([str(BINARY), "daemon", "--demo", "--foreground", "--data-dir", str(root / "profile"), "--socket", str(path)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        drawer = preview = None
        reader = None
        try:
            deadline = time.monotonic() + 5
            while True:
                try:
                    reader = Reader(path)
                    break
                except (FileNotFoundError, ConnectionRefusedError):
                    assert daemon.poll() is None, "daemon exited"
                    assert time.monotonic() < deadline
                    time.sleep(0.02)
            drawer = Terminal(["drawer", "--socket", str(path)])
            drawer.wait(lambda: b"SERVICE DEMO" in drawer.output, "attached drawer")
            drawer.send("\x1b[200~reply A\n東京\x1b[201~")
            drawer.wait(lambda: (reader.snapshot()["session"]["composer"]["draft"] or {}).get("body") == "reply A\n東京", "first durable draft")
            drawer.send(b"\x1b[B")
            drawer.wait(lambda: reader.snapshot()["session"]["composer"]["target"]["incoming"] == 4, "Down saves and switches")
            drawer.send("reply B")
            drawer.wait(lambda: (reader.snapshot()["session"]["composer"]["draft"] or {}).get("body") == "reply B", "second draft")
            drawer.send(b"\x1b[A")
            drawer.wait(lambda: (reader.snapshot()["session"]["composer"]["draft"] or {}).get("body") == "reply A\n東京", "Up restores first draft")
            drawer.send(b"\x1b[1;3D!")
            drawer.wait(lambda: (reader.snapshot()["session"]["composer"]["draft"] or {}).get("body") == "reply A\n東!京", "Alt+Left edits instead of filtering")
            drawer.send(b"\x1b[C\x1b[C")
            drawer.wait(lambda: reader.snapshot()["settings"]["source"] == "discord", "Right cycles sources while composing")
            drawer.wait(lambda: reader.snapshot()["session"]["composer"]["target"]["incoming"] == 2, "Discord reply target")
            drawer.send(b"\x1b[D")
            drawer.wait(lambda: (reader.snapshot()["session"]["composer"]["draft"] or {}).get("body") == "reply A\n東!京", "Left restores iCloud draft")
            preview = Terminal(["preview", "--socket", str(path), "4"])
            preview.wait(lambda: b"Sasha" in preview.output, "shared full reader")
            snapshot = reader.snapshot()
            assert snapshot["session"]["composer"]["draft"]["target"]["incoming"] == 5
            assert not next(m for m in snapshot["session"]["messages"] if m["id"] == 4)["unread"]
            preview.close()
            drawer.send(b"\x1b[15~")
            drawer.wait(lambda: "Submitting" in reader.snapshot()["session"]["composer"]["send"], "service-owned mock submission")
            drawer.close()
            deadline = time.monotonic() + 4
            while not reader.snapshot()["session"]["composer"]["sent"]:
                assert time.monotonic() < deadline
                time.sleep(0.02)
            snapshot = reader.snapshot()
            assert len(snapshot["session"]["composer"]["sent"]) == 1
            assert snapshot["session"]["message_state"]["5"]["dismissed"]
            assert any(d["body"] == "reply B" for d in snapshot["session"]["saved"].values())
            result = subprocess.run([str(BINARY), "service", "--socket", str(path), "stop"], capture_output=True, timeout=5)
            assert result.returncode == 0, result.stderr
            assert daemon.wait(timeout=5) == 0
        finally:
            if preview:
                preview.abort()
            if drawer:
                drawer.abort()
            if reader:
                reader.stream.close()
            if daemon.poll() is None:
                daemon.kill()
                daemon.wait()
    print("Service PTY smoke passed: reply-first arrows, Alt cursor, per-message drafts, shared reader isolation, seen state, service-owned send after drawer exit, and graceful shutdown.")


if __name__ == "__main__":
    main()
