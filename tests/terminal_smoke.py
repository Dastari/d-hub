#!/usr/bin/env python3
"""Exercise the real event loop in a Linux PTY with fictional data only.

Run `cargo build --locked` first. This is not a Ghostty/Omarchy desktop test.
"""
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import tempfile
import termios
import time


BINARY = Path(__file__).resolve().parents[1] / "target/debug/d-hub"


class Terminal:
    def __init__(self, arguments):
        ready_read, ready_write = os.pipe()
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.close(ready_write)
            os.read(ready_read, 1)
            os.close(ready_read)
            os.environ["TERM"] = "xterm-256color"
            os.environ["COLORTERM"] = "truecolor"
            os.execv(str(BINARY), [str(BINARY), *arguments])
        os.close(ready_read)
        self.output = bytearray()
        self.initial_modes = termios.tcgetattr(self.fd)
        self.resize(66, 36)
        os.write(ready_write, b"1")
        os.close(ready_write)

    def resize(self, columns, rows):
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))

    def pump(self, seconds=0.04):
        if select.select([self.fd], [], [], seconds)[0]:
            try:
                self.output.extend(os.read(self.fd, 65536))
            except OSError as error:
                if error.errno != errno.EIO:
                    raise

    def wait(self, predicate, description):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            self.pump()
            if predicate():
                return
        raise AssertionError(f"Timed out: {description}; terminal tail: {self.output[-400:]!r}")

    def send(self, value):
        os.write(self.fd, value if isinstance(value, bytes) else value.encode())

    def screen_action(self, value, marker):
        self.output.clear()
        self.send(value)
        self.wait(lambda: marker in self.output, f"screen contains {marker!r}")

    def close(self):
        self.send(b"\x11")  # Ctrl+q saves and exits in every mode.
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            self.pump()
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.pid = None
                assert os.waitstatus_to_exitcode(status) == 0, self.output[-600:]
                modes = termios.tcgetattr(self.fd)
                assert modes[3] & (termios.ECHO | termios.ICANON) == self.initial_modes[3] & (termios.ECHO | termios.ICANON)
                os.close(self.fd)
                return
        raise AssertionError(f"demo did not exit; terminal tail: {self.output[-1000:]!r}")

    def abort(self):
        if self.pid:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
            os.close(self.fd)
            self.pid = None


def state(directory):
    try:
        return json.loads((directory / "demo-state.json").read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def main():
    with tempfile.TemporaryDirectory(prefix="d-hub-pty-") as temporary:
        directory = Path(temporary)
        terminal = Terminal(["demo", "--data-dir", temporary])
        body = "jkg?/: hello 東京 👩🏽‍💻\nsecond line"
        try:
            terminal.wait(lambda: state(directory) is not None, "initial profile")
            terminal.send("\x1b[200~" + body + "\x1b[201~")
            terminal.wait(lambda: (state(directory)["composer"]["draft"] or {}).get("body") == body, "bracketed Unicode paste")
            target = state(directory)["composer"]["draft"]["target"]
            terminal.send(b"\x01")  # Ctrl+a: fixture attachment
            terminal.wait(lambda: len(state(directory)["composer"]["draft"]["attachments"]) == 1, "attachment")
            terminal.screen_action(b"\x1b", b"NAV")
            terminal.send("jn")  # browse elsewhere and simulate incoming
            terminal.wait(lambda: len(state(directory)["messages"]) == 6, "incoming fixture")
            terminal.send("f")
            terminal.wait(lambda: (directory / "config.toml").exists(), "shared settings save")
            terminal.screen_action("o", b"Sasha")
            terminal.send("j")
            terminal.screen_action("q", b"NAV")
            assert state(directory)["composer"]["draft"]["target"] == target
            assert state(directory)["composer"]["draft"]["body"] == body
            for dimensions, marker in [((28, 16), b"d-hub"), ((20, 10), b"Resize"), ((80, 40), b"d-hub")]:
                terminal.output.clear()
                terminal.resize(*dimensions)
                terminal.wait(lambda: marker in terminal.output, f"resize to {dimensions}")
            terminal.close()

            # Reopen the actual executable, then drive failure and uncertainty.
            terminal = Terminal(["demo", "--data-dir", temporary])
            terminal.wait(lambda: b"LOCKED" in terminal.output, "restored locked draft")
            assert state(directory)["composer"]["draft"]["body"] == body
            terminal.send(b"\x1b[18~\x1b[15~")  # F7 failure, F5 send
            terminal.wait(lambda: "Failed" in state(directory)["composer"]["send"], "mock failure")
            assert state(directory)["composer"]["draft"]["target"] == target
            terminal.send(b"\x1b[18~\x1b[15~")  # F7 uncertain, F5 send
            terminal.wait(lambda: "Uncertain" in state(directory)["composer"]["send"], "uncertain result")
            terminal.send(b"\x1b[15~")  # cannot blindly retry
            terminal.pump(0.15)
            assert state(directory)["composer"]["sent"] == []
            assert state(directory)["composer"]["draft"]["body"] == body
            terminal.screen_action(b"\x1b", b"NAV")
            terminal.send(":reconcile\r")
            terminal.wait(lambda: len(state(directory)["composer"]["sent"]) == 1, "mock reconciliation")
            assert state(directory)["composer"]["draft"] is None
            terminal.close()
        finally:
            terminal.abort()

        terminal = Terminal(["preview", "--demo", "4"])
        try:
            terminal.wait(lambda: b"Sasha" in terminal.output, "standalone fixture reader")
            terminal.close()
        finally:
            terminal.abort()
    print("PTY smoke passed: paste, draft lock, attachment, navigation, preview, resize, reopen, mock outcomes, reconciliation and terminal restoration.")


if __name__ == "__main__":
    main()
