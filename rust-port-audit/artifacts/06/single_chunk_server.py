#!/usr/bin/env python3
"""Definitive repro of the Rust run --attach SSE parser bug.

Sends TWO text parts in a single SSE chunk (no delay). If the Rust client's
sse_stream parser correctly buffered events, BOTH lines print. The observed
behavior: only the FIRST event in the chunk is processed; the rest are dropped.
"""
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        if length:
            self.rfile.read(length)
        path = self.path.split("?")[0]
        if path == "/session":
            sid = "ses_mock000000000000000000000001"
            session = {"id": sid, "title": "t", "directory": "/tmp/oc-audit-06",
                       "parentID": None, "time": {"created": int(time.time() * 1000)}}
            body = json.dumps({"data": session}).encode()
        else:
            body = b"{}"
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        sid = "ses_mock000000000000000000000001"
        now = int(time.time() * 1000)
        events = [
            {"type": "session.status", "properties": {"sessionID": sid, "status": {"type": "running"}}},
            {"type": "message.updated", "properties": {
                "sessionID": sid,
                "info": {"role": "assistant", "agent": "build", "modelID": "gpt-4o"}}},
            {"type": "message.part.updated", "properties": {
                "sessionID": sid,
                "part": {"id": "prt_a", "sessionID": sid, "messageID": "msg_mock1",
                         "type": "text", "text": "FIRST-EVENT", "time": {"start": now, "end": now + 1}}}},
            {"type": "message.part.updated", "properties": {
                "sessionID": sid,
                "part": {"id": "prt_b", "sessionID": sid, "messageID": "msg_mock1",
                         "type": "text", "text": "SECOND-EVENT", "time": {"start": now, "end": now + 1}}}},
            {"type": "session.status", "properties": {"sessionID": sid, "status": {"type": "idle"}}},
        ]
        body = b""
        for e in events:
            body += b"data: " + json.dumps(e).encode() + b"\n\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 4197
    srv = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"single-chunk SSE server on 127.0.0.1:{port}", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
