#!/usr/bin/env python3
"""Smoke-test the Solveig stdio language server.

Usage:
    cargo build
    python3 scripts/lsp_smoke.py tests/golden/diag/type_mismatch

Set SOLVEIG_BIN=/path/to/solveig to test a different binary.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import select
import subprocess
import sys
import time


def packet(message: dict) -> bytes:
    body = json.dumps(message, separators=(",", ":")).encode("utf-8")
    return f"Content-Length: {len(body)}\r\n\r\n".encode("ascii") + body


def send(process: subprocess.Popen[bytes], message: dict) -> None:
    assert process.stdin is not None
    process.stdin.write(packet(message))
    process.stdin.flush()


def read_packet(process: subprocess.Popen[bytes], timeout_seconds: float = 5.0) -> dict:
    assert process.stdout is not None
    deadline = time.monotonic() + timeout_seconds
    stdout_fd = process.stdout.fileno()
    header = b""

    while b"\r\n\r\n" not in header:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("timed out waiting for an LSP header")
        ready, _, _ = select.select([stdout_fd], [], [], remaining)
        if not ready:
            raise TimeoutError("timed out waiting for an LSP header")
        chunk = os.read(stdout_fd, 1)
        if not chunk:
            raise RuntimeError("language server exited before sending a complete header")
        header += chunk

    length = None
    for line in header.decode("ascii").split("\r\n"):
        if line.lower().startswith("content-length:"):
            length = int(line.split(":", 1)[1].strip())
            break

    if length is None:
        raise RuntimeError(f"LSP message had no Content-Length header: {header!r}")

    chunks = []
    remaining_length = length
    while remaining_length > 0:
        remaining_time = deadline - time.monotonic()
        if remaining_time <= 0:
            raise TimeoutError("timed out waiting for an LSP body")
        ready, _, _ = select.select([stdout_fd], [], [], remaining_time)
        if not ready:
            raise TimeoutError("timed out waiting for an LSP body")
        chunk = os.read(stdout_fd, remaining_length)
        if not chunk:
            raise RuntimeError("language server exited before sending a complete body")
        chunks.append(chunk)
        remaining_length -= len(chunk)

    body = b"".join(chunks)
    if len(body) != length:
        raise RuntimeError("language server exited before sending a complete body")
    return json.loads(body.decode("utf-8"))


def read_until(
    process: subprocess.Popen[bytes],
    predicate,
    timeout_seconds: float = 8.0,
) -> dict:
    deadline = time.monotonic() + timeout_seconds
    while True:
        message = read_packet(process, max(0.1, deadline - time.monotonic()))
        if predicate(message):
            return message
        if time.monotonic() >= deadline:
            raise TimeoutError("timed out waiting for the expected LSP message")


def source_and_root(argument: str) -> tuple[Path, Path]:
    path = Path(argument).resolve()
    source = path if path.is_file() else path / "src" / "main.sv"
    if not source.is_file():
        raise FileNotFoundError(f"could not find source file: {source}")

    if source.parent.name == "src":
        return source, source.parent.parent
    return source, source.parent


def main() -> int:
    fixture = sys.argv[1] if len(sys.argv) > 1 else "tests/golden/diag/type_mismatch"
    binary = Path(os.environ.get("SOLVEIG_BIN", "target/debug/solveig")).resolve()
    if not binary.is_file():
        print(f"missing binary: {binary}", file=sys.stderr)
        print("run `cargo build` first, or set SOLVEIG_BIN", file=sys.stderr)
        return 2

    source, root = source_and_root(fixture)
    process = subprocess.Popen(
        [str(binary), "lsp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    try:
        send(
            process,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": os.getpid(),
                    "rootUri": root.as_uri(),
                    "capabilities": {
                        "general": {"positionEncodings": ["utf-8", "utf-16"]}
                    },
                    "workspaceFolders": [{"uri": root.as_uri(), "name": root.name}],
                },
            },
        )
        initialize = read_until(process, lambda message: message.get("id") == 1)
        encoding = initialize["result"]["capabilities"].get("positionEncoding")
        print(f"initialized: positionEncoding={encoding}")

        send(process, {"jsonrpc": "2.0", "method": "initialized", "params": {}})
        send(
            process,
            {
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": source.as_uri(),
                        "languageId": "solveig",
                        "version": 1,
                        "text": source.read_text(encoding="utf-8"),
                    }
                },
            },
        )

        publish = read_until(
            process,
            lambda message: message.get("method") == "textDocument/publishDiagnostics",
        )
        params = publish["params"]
        diagnostics = params["diagnostics"]
        print(f"diagnostics for {params['uri']}:")
        for diagnostic in diagnostics:
            range_ = diagnostic["range"]
            start = range_["start"]
            end = range_["end"]
            message = diagnostic["message"].splitlines()[0]
            print(
                "  "
                f"{diagnostic.get('code', '<no-code>')}: "
                f"{message} "
                f"@ {start['line']}:{start['character']}-{end['line']}:{end['character']}"
            )

        send(process, {"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": None})
        read_until(process, lambda message: message.get("id") == 2)
        send(process, {"jsonrpc": "2.0", "method": "exit"})
        assert process.stdin is not None
        process.stdin.close()
        process.wait(timeout=2)

        return 0 if diagnostics else 1
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()


if __name__ == "__main__":
    raise SystemExit(main())
