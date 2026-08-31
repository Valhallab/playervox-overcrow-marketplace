#!/usr/bin/python3
"""Bounded loopback HTTP fixture for the deployment verifier smoke test."""

from __future__ import annotations

import argparse
import http.server
from pathlib import Path, PurePosixPath
import sys


MAX_OVERSIZED_BYTES = 32 * 1024 * 1024
CHUNK_BYTES = 64 * 1024


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--port-file", required=True, type=Path)
    parser.add_argument("--log-file", required=True, type=Path)
    parser.add_argument(
        "--mode",
        required=True,
        choices=(
            "valid",
            "redirect",
            "catalog-mismatch",
            "bytes-mismatch",
            "wrong-cache",
            "duplicate-cache",
            "wrong-mime",
            "duplicate-mime",
            "oversized",
        ),
    )
    parser.add_argument("--target", default="")
    parser.add_argument("--replacement", type=Path)
    return parser.parse_args()


def expected_headers(relative: str) -> tuple[str, str | None]:
    if relative == "marketplace/v1/catalog.json":
        return "application/json", "no-cache"
    if relative == "marketplace/catalog-policy.js":
        return "application/javascript", "no-cache"
    if relative.startswith("marketplace/v1/packages/") and relative.endswith(".ocpkg"):
        return "application/octet-stream", "public, max-age=31536000, immutable"
    if relative.startswith("marketplace/v1/previews/") and relative.endswith(".png"):
        return "image/png", "public, max-age=31536000, immutable"
    if relative.endswith(".html"):
        return "text/html", "no-cache"
    if relative.endswith(".js"):
        return "application/javascript", "no-cache"
    if relative.endswith(".css"):
        return "text/css", "no-cache"
    if relative.endswith(".png"):
        return "image/png", "no-cache"
    if relative.endswith(".svg"):
        return "image/svg+xml", "no-cache"
    if relative.endswith((".jpg", ".jpeg")):
        return "image/jpeg", "no-cache"
    if relative.endswith(".webp"):
        return "image/webp", "no-cache"
    if relative.endswith(".woff2"):
        return "font/woff2", "no-cache"
    if relative.endswith(".txt"):
        return "text/plain", "no-cache"
    return "application/octet-stream", "no-cache"


def request_relative(path: str) -> str | None:
    if "?" in path or "#" in path or not path.startswith("/"):
        return None
    if path == "/":
        return "index.html"
    if path == "/marketplace/":
        return "marketplace/index.html"
    relative = path[1:]
    parsed = PurePosixPath(relative)
    if parsed.is_absolute() or ".." in parsed.parts or "." in parsed.parts:
        return None
    return relative


def mutate_same_length(data: bytes) -> bytes:
    if not data:
        return data
    return bytes((data[0] ^ 1,)) + data[1:]


def main() -> int:
    arguments = parse_arguments()
    root = arguments.root.resolve(strict=True)
    log_file = arguments.log_file

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, _format: str, *_args: object) -> None:
            return

        def record(self, line: str) -> None:
            with log_file.open("a", encoding="utf-8") as stream:
                stream.write(f"{line}\n")
                stream.flush()

        def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
            self.record(self.path)
            if arguments.mode == "redirect" and self.path == "/":
                self.send_response(302)
                self.send_header("Location", "/redirected")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return

            relative = request_relative(self.path)
            if relative is None:
                self.send_error(400)
                return
            target = (root / relative).resolve()
            try:
                target.relative_to(root)
            except ValueError:
                self.send_error(400)
                return
            if not target.is_file() or target.is_symlink():
                self.send_error(404)
                return

            content_type, cache_control = expected_headers(relative)
            if arguments.mode == "catalog-mismatch" and relative == arguments.target:
                if arguments.replacement is None:
                    self.send_error(500)
                    return
                data = arguments.replacement.read_bytes()
            else:
                data = target.read_bytes()
            if arguments.mode == "bytes-mismatch" and relative == arguments.target:
                data = mutate_same_length(data)
            if arguments.mode == "wrong-mime" and relative == arguments.target:
                content_type = "text/plain"

            self.send_response(200)
            self.send_header("Content-Type", content_type)
            if arguments.mode == "duplicate-mime" and relative == arguments.target:
                self.send_header("Content-Type", "text/plain")
            if cache_control is not None:
                if arguments.mode == "wrong-cache" and relative == arguments.target:
                    self.send_header("Cache-Control", "no-store")
                elif arguments.mode == "duplicate-cache" and relative == arguments.target:
                    self.send_header("Cache-Control", cache_control)
                    self.send_header("Cache-Control", "no-store")
                else:
                    self.send_header("Cache-Control", cache_control)

            if arguments.mode == "oversized" and relative == arguments.target:
                self.end_headers()
                sent = 0
                broken = False
                chunk = b"x" * CHUNK_BYTES
                try:
                    while sent < MAX_OVERSIZED_BYTES:
                        self.wfile.write(chunk)
                        self.wfile.flush()
                        sent += len(chunk)
                except (BrokenPipeError, ConnectionResetError):
                    broken = True
                self.record(f"OVERSIZED sent={sent} broken={int(broken)}")
                self.close_connection = True
                return

            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    arguments.port_file.write_text(f"{server.server_port}\n", encoding="ascii")
    try:
        server.serve_forever(poll_interval=0.1)
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
