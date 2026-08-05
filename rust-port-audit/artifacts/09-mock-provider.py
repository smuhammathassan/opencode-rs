#!/usr/bin/env python3
"""Mock OpenAI-compatible SSE provider for agent 09 audit.

Serves POST /v1/chat/completions returning a small SSE chat-completion stream.
Requires a Bearer API key and records the request to a log file so auditors can
confirm whether a real request flowed through the Rust CLI.
Usage: python3 09-mock-provider.py <port> <logfile>
"""
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
LOGFILE = sys.argv[2]


def chunk(obj):
    return f"data: {json.dumps(obj)}\n\n"


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        with open(LOGFILE, "a") as f:
            f.write(
                json.dumps(
                    {
                        "path": self.path,
                        "auth": self.headers.get("Authorization"),
                        "body": json.loads(body or "{}"),
                    }
                )
                + "\n"
            )
        if self.path != "/v1/chat/completions":
            self.send_response(404)
            self.end_headers()
            return
        if self.headers.get("Authorization") != "Bearer test-key-12345":
            self.send_response(401)
            self.end_headers()
            self.wfile.write(b'{"error":{"message":"bad key"}}')
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        self.wfile.write(chunk({"id": "c1", "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]}))
        self.wfile.write(chunk({"id": "c1", "choices": [{"index": 0, "delta": {"content": "Hello"}}]}))
        self.wfile.write(chunk({"id": "c1", "choices": [{"index": 0, "delta": {"content": " from mock!"}}]}))
        self.wfile.write(chunk({"id": "c1", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}))
        self.wfile.write(chunk({"id": "c1", "choices": [], "usage": {"prompt_tokens": 3, "completion_tokens": 5, "total_tokens": 8}}))
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"mock provider up\n")


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
