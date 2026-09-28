"""Thin tui-snap client (A09).

Spawns ``tuisnap --machine``, sends one Op JSON object per line on stdin,
reads one envelope JSON object per line on stdout. Op errors raise
verbatim with the engine's ``{code, message, session}``. No assertions
live here: even the ``assert`` op just round-trips to the shared engine.
"""

from __future__ import annotations

import json
import os
import subprocess


class OpError(Exception):
    def __init__(self, code: str, message: str, session=None):
        self.code = code
        self.session = session
        label = f"[{code}] {session}: {message}" if session else f"[{code}] {message}"
        super().__init__(label)


class Client:
    def __init__(self, binary: str | None = None):
        self.binary = binary or os.environ.get("TUISNAP_BIN", "tuisnap")
        self.proc: subprocess.Popen | None = None

    def start(self) -> "Client":
        if self.proc is None:
            self.proc = subprocess.Popen(
                [self.binary, "--machine"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=None,
                text=True,
                bufsize=1,
            )
        return self

    def call(self, op: dict) -> dict:
        self.start()
        assert self.proc is not None and self.proc.stdin and self.proc.stdout
        self.proc.stdin.write(json.dumps(op) + "\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        if not line:
            raise OpError("op-failed", f"{self.binary} closed stdout")
        try:
            env = json.loads(line)
        except json.JSONDecodeError as e:
            raise OpError("op-failed", f"bad envelope: {e}: {line.strip()}") from e
        if env.get("ok"):
            return env["result"]
        err = env.get("error") or {}
        raise OpError(err.get("code", "op-failed"), err.get("message", "unknown error"),
                      err.get("session"))

    def close(self) -> None:
        proc, self.proc = self.proc, None
        if proc is None:
            return
        try:
            if proc.stdin:
                proc.stdin.close()
            proc.wait(timeout=10)
        finally:
            if proc.poll() is None:
                proc.kill()

    def __enter__(self) -> "Client":
        return self.start()

    def __exit__(self, *exc) -> None:
        self.close()
