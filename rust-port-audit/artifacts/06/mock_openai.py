#!/usr/bin/env python3
"""Mock OpenAI-compatible chat completions SSE server for the audit."""
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = "/root/opencode-rs/rust-port-audit/artifacts/06/openai_trace.log"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        with open(LOG, "a") as f:
            f.write(f"POST {self.path} {body.decode(errors='replace')}\n")
        if self.path.endswith("/chat/completions"):
            chunks = [
                {"id": "chatcmpl-mock1", "object": "chat.completion.chunk",
                 "choices": [{"index": 0, "delta": {"content": "Hello from mock OpenAI"}}]},
                {"id": "chatcmpl-mock1", "object": "chat.completion.chunk",
                 "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
            ]
            payload = b""
            for c in chunks:
                payload += b"data: " + json.dumps(c).encode() + b"\n\n"
            payload += b"data: [DONE]\n\n"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        else:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()

    def log_message(self, *args):
        pass


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 4399
    srv = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"mock OpenAI on 127.0.0.1:{port}", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
