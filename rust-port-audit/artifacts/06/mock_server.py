#!/usr/bin/env python3
"""Mock opencode HTTP server for auditing the Rust `opencode run --attach` path.

Implements just enough of the reference server REST + SSE surface that the Rust
AttachClient (crates/oc-cli/src/cli/cmd/run/client.rs) talks to, and logs every
request to a trace file so the auditor can reconstruct the session lifecycle.
"""
import json
import os
import sys
import time
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = "/root/opencode-rs/rust-port-audit/artifacts/06/mock_trace.log"
log_lock = threading.Lock()


def log(line):
    with log_lock:
        with open(LOG, "a") as f:
            f.write(f"{time.time():.3f} {line}\n")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _send(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _read_body(self):
        length = int(self.headers.get("Content-Length") or 0)
        if length == 0:
            return {}
        return json.loads(self.rfile.read(length) or b"{}")

    def _route(self, method):
        path = self.path.split("?")[0]
        log(f"{method} {self.path}")
        if path == "/config" and method == "GET":
            return self._send(200, {"data": {"share": "off"}})
        if path == "/app/agents" and method == "GET":
            return self._send(200, {"data": [{"name": "build", "mode": "primary"},
                                             {"name": "helper", "mode": "subagent"}]})
        if path == "/path" and method == "GET":
            return self._send(200, {"data": {"directory": "/tmp/oc-audit-06"}})
        if path == "/session" and method == "GET":
            return self._send(200, {"data": []})
        if path == "/session" and method == "POST":
            body = self._read_body()
            sid = "ses_mock000000000000000000000001"
            log(f"POST /session body={json.dumps(body)}")
            return self._send(200, {"data": {
                "id": sid,
                "title": body.get("title"),
                "directory": "/tmp/oc-audit-06",
                "parentID": None,
                "time": {"created": int(time.time() * 1000)},
            }})
        if path.startswith("/session/") and path.endswith("/fork") and method == "POST":
            return self._send(200, {"data": {
                "id": "ses_mockfork000000000000000000001",
                "title": "forked",
                "directory": "/tmp/oc-audit-06",
                "parentID": "ses_mock000000000000000000000001",
                "time": {"created": int(time.time() * 1000)},
            }})
        if path.startswith("/session/") and path.endswith("/message") and method == "POST":
            body = self._read_body()
            log(f"POST {path} body={json.dumps(body)}")
            return self._send(200, {"data": {"id": "msg_mock1"}})
        if path.startswith("/session/") and path.endswith("/command") and method == "POST":
            body = self._read_body()
            log(f"POST {path} body={json.dumps(body)}")
            return self._send(200, {"data": {"id": "msg_mock1"}})
        if path.startswith("/session/") and path.endswith("/share") and method == "POST":
            return self._send(200, {"data": {"url": "https://opncd.ai/share/mock123"}})
        if path == "/event" and method == "GET":
            return self._stream_events()
        if path.startswith("/session/"):
            return self._send(200, {"data": {
                "id": path.split("/")[2],
                "title": "mock",
                "directory": "/tmp/oc-audit-06",
                "parentID": None,
            }})
        log(f"UNHANDLED {method} {path}")
        return self._send(404, {"error": {"message": f"unhandled {path}"}})

    def _stream_events(self):
        sid = "ses_mock000000000000000000000001"
        now = int(time.time() * 1000)
        events = [
            {"type": "session.status", "properties": {"sessionID": sid, "status": {"type": "running"}}},
            {"type": "message.updated", "properties": {
                "sessionID": sid,
                "info": {"role": "assistant", "agent": "build", "modelID": "gpt-4o"}}},
            {"type": "message.part.updated", "properties": {
                "sessionID": sid,
                "part": {"id": "prt_t1", "sessionID": sid, "messageID": "msg_mock1",
                         "type": "text", "text": "Hello from mock", "time": {"start": now, "end": now + 1}}}},
            {"type": "session.status", "properties": {"sessionID": sid, "status": {"type": "idle"}}},
        ]
        stream_delay = float(self.headers.get("X-Stream-Delay", "0") or 0) or (
            0.4 if os.environ.get("MOCK_STREAM_DELAY") else 0.0
        )
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        for e in events:
            self.wfile.write(b"data: " + json.dumps(e).encode() + b"\n\n")
            self.wfile.flush()
            time.sleep(stream_delay)

    def do_GET(self):
        self._route("GET")

    def do_POST(self):
        self._route("POST")

    def log_message(self, *args):
        pass


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 4199
    srv = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"mock opencode server on 127.0.0.1:{port}", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
