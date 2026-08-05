#!/usr/bin/env python3
"""Minimal MCP stdio server for Agent 08's wire-protocol audit.

Implements the MCP JSON-RPC 2.0 subset needed for a client round trip:
  initialize, notifications/initialized, ping, tools/list, tools/call,
  prompts/list, resources/list.
All lines received on stdin are echoed (prefixed with "C ") to a log file so
the client's exact wire bytes can be inspected.
"""
import sys, json, os

LOG = os.environ.get("MCP08_LOG")

def log(msg):
    if LOG:
        with open(LOG, "a") as f:
            f.write(msg + "\n")

def send(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()

def respond(msg_id, result):
    send({"jsonrpc": "2.0", "id": msg_id, "result": result})

def error(msg_id, code, message, data=None):
    err = {"code": code, "message": message}
    if data is not None:
        err["data"] = data
    send({"jsonrpc": "2.0", "id": msg_id, "error": err})

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    log("C " + line)
    try:
        msg = json.loads(line)
    except Exception as e:
        log("unparseable: %r" % (line,))
        continue
    if "method" not in msg:
        log("CLIENT_RESPONSE " + line)
        continue
    method = msg.get("method")
    params = msg.get("params", {}) or {}
    if method == "initialize":
        respond(msg["id"], {
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {"listChanged": True}, "prompts": {}, "resources": {}},
            "serverInfo": {"name": "audit-server", "version": "0.0.1"},
            "instructions": "Agent 08 mock MCP server.",
        })
    elif method == "notifications/initialized":
        send({"jsonrpc": "2.0", "method": "notifications/initialized"})  # server->client notify (allowed)
    elif method == "ping":
        respond(msg["id"], {})
    elif method == "tools/list":
        respond(msg["id"], {
            "tools": [{
                "name": "audit_echo",
                "description": "Echo back the text argument",
                "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
            }],
        })
    elif method == "tools/call":
        name = params.get("name")
        args = params.get("arguments", {}) or {}
        if name == "audit_echo":
            respond(msg["id"], {"content": [{"type": "text", "text": args.get("text", "")}]})
        else:
            respond(msg["id"], {"content": [{"type": "text", "text": "unknown tool"}], "isError": True})
    elif method == "prompts/list":
        respond(msg["id"], {"prompts": []})
    elif method == "resources/list":
        respond(msg["id"], {"resources": []})
    else:
        error(msg["id"], -32601, "Method not found", {"method": method})
