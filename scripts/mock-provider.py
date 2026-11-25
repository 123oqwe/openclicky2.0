#!/usr/bin/env python3
"""A local OpenAI-compatible endpoint, so the whole ask path can be proven
without a real key, a real model, or network access.

    python3 scripts/mock-provider.py --port 8799 --log /tmp/req.jsonl \
        --reply '在这儿 [POINT:120,240:提交按钮:screen0]'

Serves GET /v1/models, POST /v1/chat/completions (JSON and SSE),
POST /v1/audio/transcriptions. Every request is appended to --log as one JSON
object per line, so a test can assert what the app actually sent: whether
images were attached, what the system prompt contained, which model was asked.
"""
import argparse
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ARGS = None
LOCK = threading.Lock()


def record(kind, path, body):
    if not ARGS.log:
        return
    with LOCK, open(ARGS.log, "a", encoding="utf-8") as fh:
        fh.write(json.dumps({"kind": kind, "path": path, "body": body}) + "\n")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def _send(self, code, payload, content_type="application/json"):
        raw = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        if self.path.rstrip("/").endswith("/models"):
            record("models", self.path, {})
            self._send(200, {"data": [{"id": m} for m in ARGS.models]})
            return
        self._send(404, {"error": {"message": "not found"}})

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""

        if self.path.rstrip("/").endswith("/chat/completions"):
            body = json.loads(raw or b"{}")
            record("chat", self.path, body)
            reply = ARGS.reply
            if body.get("stream"):
                self._stream(reply)
            else:
                self._send(
                    200,
                    {
                        "choices": [
                            {
                                "message": {"role": "assistant", "content": reply},
                                "finish_reason": "stop",
                            }
                        ],
                        "usage": {"prompt_tokens": 11, "completion_tokens": 7},
                    },
                )
            return

        if self.path.rstrip("/").endswith("/audio/transcriptions"):
            record("stt", self.path, {"bytes": len(raw)})
            self._send(200, {"text": ARGS.transcript})
            return

        record("unknown", self.path, {"bytes": len(raw)})
        self._send(404, {"error": {"message": "not found"}})

    def _stream(self, reply):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        step = max(1, len(reply) // 3)
        for i in range(0, len(reply), step):
            chunk = {
                "choices": [{"delta": {"content": reply[i : i + step]}, "finish_reason": None}]
            }
            self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
            self.wfile.flush()
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()


def main():
    global ARGS
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8799)
    ap.add_argument("--log")
    ap.add_argument("--reply", default="这是一条 mock 回答。")
    ap.add_argument("--transcript", default="mock transcript")
    ap.add_argument("--models", default="mock-chat,mock-fast,mock-deep")
    ARGS = ap.parse_args()

    server = ThreadingHTTPServer(("127.0.0.1", ARGS.port), Handler)
    print(f"mock provider on http://127.0.0.1:{ARGS.port}/v1", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
