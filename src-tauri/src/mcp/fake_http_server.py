# Test fixture: a tiny MCP server speaking Streamable HTTP on /mcp and the
# legacy HTTP+SSE transport on /sse (POST /sse answers 405, like real
# SSE-only servers). Prints its port, then serves until killed.
import json, queue, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TOOLS = [{"name": "echo", "description": "Echo", "inputSchema": {"type": "object", "properties": {"message": {"type": "string"}}}}]
streams = {}


def answer(msg):
    mid, method = msg.get("id"), msg.get("method")
    if mid is None:
        return None
    if method == "initialize":
        res = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake-http", "version": "2"}}
    elif method == "tools/list":
        res = {"tools": TOOLS}
    elif method == "tools/call":
        res = {"content": [{"type": "text", "text": "echo: " + msg["params"]["arguments"].get("message", "")}]}
    else:
        return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "nope"}}
    return {"jsonrpc": "2.0", "id": mid, "result": res}


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def body(self):
        return json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"null")

    def send(self, code, ctype=None, data=b"", headers=None):
        self.send_response(code)
        if ctype:
            self.send_header("Content-Type", ctype)
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        if self.path == "/mcp":
            msg = self.body()
            if msg.get("method") != "initialize" and self.headers.get("Mcp-Session-Id") != "sess-1":
                return self.send(400, "text/plain", b"missing session")
            reply = answer(msg)
            if reply is None:
                return self.send(202)
            if msg.get("method") == "tools/list":
                return self.send(200, "application/json", json.dumps(reply).encode())
            note = {"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info", "data": "working"}}
            sse = "event: message\ndata: " + json.dumps(note) + "\n\nevent: message\ndata: " + json.dumps(reply) + "\n\n"
            return self.send(200, "text/event-stream", sse.encode(), {"Mcp-Session-Id": "sess-1"})
        if self.path == "/sse":
            self.body()
            return self.send(405, "text/plain", b"use GET")
        if self.path.startswith("/messages"):
            sid = self.path.split("sid=")[-1]
            reply = answer(self.body())
            if reply is not None:
                streams[sid].put(reply)
            return self.send(202)
        self.send(404)

    def do_GET(self):
        if self.path != "/sse":
            return self.send(404)
        sid = str(len(streams) + 1)
        q = streams[sid] = queue.Queue()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.write(("event: endpoint\ndata: /messages?sid=" + sid + "\n\n").encode())
        self.wfile.flush()
        while True:
            m = q.get()
            self.wfile.write(("event: message\ndata: " + json.dumps(m) + "\n\n").encode())
            self.wfile.flush()


srv = ThreadingHTTPServer(("127.0.0.1", 0), H)
print(srv.server_address[1], flush=True)
srv.serve_forever()
