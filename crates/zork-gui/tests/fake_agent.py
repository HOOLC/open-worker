#!/usr/bin/env python3
"""Fake zork-agent HTTP/SSE server for zork-gui verification.

Implements the subset of the agent API that zork-gui uses:
  GET  /v1/profiles
  GET  /v1/sessions
  POST /v1/sessions
  GET  /v1/sessions/{id}
  PUT  /v1/sessions/{id}/selection
  POST /v1/sessions/{id}/mailbox
  GET  /v1/sessions/{id}/messages?before=&limit=
  GET  /v1/sessions/{id}/events   (SSE: message / wait / assistant_delta / status)
  POST /v1/sessions/{id}/cancel
  DELETE /v1/sessions/{id}

Run: uv run fake_agent.py [--port 3010]
"""

import argparse
import json
import queue
import threading
import time
import uuid
from urllib.parse import parse_qs, urlsplit

SESSIONS = {}
LOCK = threading.RLock()
PUBSUB = {}  # session_id -> list[queue.Queue]

MARKDOWN_FIXTURE = """# Markdown fixture

你好 😀 — **bold**, *italic*, ~~removed~~, and `inline_code()`.

- first item
- [x] completed item

> Streaming and final Markdown should use the same readable treatment.

```rust
fn main() {
    println!("你好");
}
```

[Fixture documentation](https://example.com/zork-gui)
"""

LONG_TOOL_FIXTURE = (
    "$ render --all\n"
    + "".join(f"line {index:03d}: 输出 😀 /tmp/世界.rs\n" for index in range(90))
    + "tool tail sentinel"
)


def new_session(profile_id, model, thinking, workspace):
    sid = uuid.uuid4().hex[:12]
    with LOCK:
        SESSIONS[sid] = {
            "session_id": sid,
            "profile_id": profile_id,
            "model": model,
            "thinking": thinking,
            "workspace": workspace,
            "status": "wait",
            "messages": [],
            "seq": 0,
            "run_generation": 0,
        }
        PUBSUB[sid] = []
    return SESSIONS[sid]


def session_summary(session):
    return {
        "session_id": session["session_id"],
        "profile_id": session["profile_id"],
        "model": session["model"],
        "thinking": session["thinking"],
        "workspace": session["workspace"],
        "status": session["status"],
    }


def emit(session_id, event_name, data):
    with LOCK:
        queues = list(PUBSUB.get(session_id, []))
    payload = json.dumps(data)
    for q in queues:
        q.put(("event", event_name, payload))


def public_msg(session, role, content):
    session["seq"] += 1
    msg = {"type": "message", "role": role, "content": content}
    session["messages"].append((f"e{session['seq']:08d}", msg))
    return msg


def public_wait(session, reason):
    session["seq"] += 1
    item = {"type": "wait", "reason": reason}
    session["messages"].append((f"e{session['seq']:08d}", item))
    return item


def emit_public_message(session_id, item):
    event_name = "wait" if item["type"] == "wait" else "message"
    emit(session_id, event_name, item)


def run_is_current(session_id, generation):
    with LOCK:
        session = SESSIONS.get(session_id)
        return session is not None and session["run_generation"] == generation


def emit_for_run(session_id, generation, event_name, data):
    with LOCK:
        if not run_is_current(session_id, generation):
            return False
        emit(session_id, event_name, data)
        return True


def simulate_run(session_id, requested_content, generation):
    """Mimic one agent activation after a mailbox append."""

    def run():
        with LOCK:
            session = SESSIONS[session_id]
            if session["run_generation"] != generation:
                return
            session["status"] = "working"
        if requested_content == "fixture:failed":
            time.sleep(0.4)
            with LOCK:
                if not run_is_current(session_id, generation):
                    return
                session["status"] = "wait"
            emit_for_run(
                session_id,
                generation,
                "status",
                {"state": "failed", "reason": "fixture model failure"},
            )
            return

        time.sleep(15.0 if requested_content == "fixture:interruptible" else 0.4)
        with LOCK:
            if not run_is_current(session_id, generation):
                return
            waiting = public_wait(session, "model response")
        emit_public_message(session_id, waiting)
        emit_for_run(
            session_id,
            generation,
            "status",
            {"state": "waiting", "reason": "model response", "deadline_ms": 0},
        )
        time.sleep(0.6)
        if not emit_for_run(session_id, generation, "status", {"state": "thinking"}):
            return
        if requested_content == "fixture:markdown":
            chunks = [
                "# Markdown fixture\n\n你好 😀 — **bold**, ",
                "*italic*, ~~removed~~, and `inline_code()`.\n\n",
                "- first item\n- [x] completed item\n\n",
                "> Streaming and final Markdown should use the same readable treatment.\n\n",
                "```rust\nfn main() {\n    println!(\"你好\");\n}\n```\n",
            ]
            assistant_content = MARKDOWN_FIXTURE
            tool_content = LONG_TOOL_FIXTURE
            final_content = "Rendering fixture complete."
        else:
            chunks = ["Running ", "tools in ", "the fake ", "agent… "]
            assistant_content = "Running tools in the fake agent…"
            tool_content = "$ ls -la\n  drwxr-xr-x  .\n  -rw-r--r--  main.rs"
            final_content = "Done. Files listed."
        for chunk in chunks:
            time.sleep(0.15)
            if not emit_for_run(
                session_id, generation, "assistant_delta", {"text": chunk}
            ):
                return
        with LOCK:
            session = SESSIONS[session_id]
            if not run_is_current(session_id, generation):
                return
            assistant = public_msg(session, "assistant", assistant_content)
            tool = public_msg(session, "tool", tool_content)
            final = public_msg(session, "assistant", final_content)
            session["status"] = "wait"
        emit_public_message(session_id, assistant)
        emit_for_run(
            session_id,
            generation,
            "status",
            {
                "state": "tools_started",
                "calls": [{"tool_call_id": "call-fake-1", "tool_name": "bash"}],
            },
        )
        emit_public_message(session_id, tool)
        emit_for_run(
            session_id,
            generation,
            "status",
            {"state": "tool_finished", "tool_call_id": "call-fake-1"},
        )
        emit_public_message(session_id, final)
        emit_for_run(session_id, generation, "status", {"state": "finished"})
        time.sleep(0.2)
        with LOCK:
            if not run_is_current(session_id, generation):
                return
            waiting = public_wait(session, "job completion")
        emit_public_message(session_id, waiting)
        emit_for_run(
            session_id,
            generation,
            "status",
            {"state": "waiting", "reason": "job completion", "deadline_ms": 0},
        )

    threading.Thread(target=run, daemon=True).start()


class Handler:
    def log_message(self, *a):
        pass

    def _json(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _no_content(self):
        self.send_response(204)
        self.end_headers()

    def _route(self):
        parsed = urlsplit(self.path)
        return [part for part in parsed.path.split("/") if part], parse_qs(
            parsed.query
        )

    def _session(self, session_id):
        with LOCK:
            return SESSIONS.get(session_id)

    def do_GET(self):
        parts, query = self._route()
        if parts == ["v1", "profiles"]:
            return self._json(
                200,
                {
                    "items": [
                        {
                            "profile_id": "dev",
                            "provider": "fake",
                            "billing": "personal",
                            "auth_configured": True,
                            "account": {},
                            "rateLimits": {},
                            "models": [
                                {
                                    "id": "fake-1",
                                    "api": "openai-completions",
                                    "streaming": True,
                                    "parallel_tool_calls": False,
                                    "thinking": ["low", "high"],
                                    "default_thinking": "low",
                                    "capabilities": {"input": ["text"]},
                                    "default": False,
                                },
                                {
                                    "id": "fake-2",
                                    "api": "anthropic-messages",
                                    "streaming": True,
                                    "parallel_tool_calls": False,
                                    "thinking": ["off"],
                                    "default_thinking": "off",
                                    "capabilities": {"input": ["text"]},
                                    "default": False,
                                },
                            ],
                        },
                        {
                            "profile_id": "prod",
                            "provider": "fake",
                            "billing": "team",
                            "auth_configured": True,
                            "account": {},
                            "rateLimits": {},
                            "models": [
                                {
                                    "id": "big-1",
                                    "api": "openai-responses",
                                    "streaming": True,
                                    "parallel_tool_calls": False,
                                    "thinking": ["medium"],
                                    "default_thinking": "medium",
                                    "capabilities": {"input": ["text"]},
                                    "default": False,
                                }
                            ],
                        },
                    ]
                },
            )
        if parts == ["v1", "sessions"]:
            with LOCK:
                items = [session_summary(session) for session in SESSIONS.values()]
            return self._json(200, {"items": items})
        if len(parts) not in (3, 4) or parts[:2] != ["v1", "sessions"]:
            return self._json(
                404,
                {
                    "error": {
                        "code": "not_found",
                        "message": f"route {self.path} not found",
                    }
                },
            )
        session = self._session(parts[2])
        if session is None:
            return self._json(
                404,
                {
                    "error": {
                        "code": "session_not_found",
                        "message": "session not found",
                    }
                },
            )
        if len(parts) == 3:
            return self._json(200, session_summary(session))
        if parts[3] == "messages":
            return self._messages(session, query)
        if parts[3] == "events":
            return self._events(session)
        return self._json(
            404,
            {
                "error": {
                    "code": "not_found",
                    "message": f"route {self.path} not found",
                }
            },
        )

    def _messages(self, session, query):
        try:
            limit = int(query.get("limit", ["50"])[0])
        except ValueError:
            return self._json(
                400, {"error": {"code": "invalid_request", "message": "invalid limit"}}
            )
        if not 1 <= limit <= 200:
            return self._json(
                422,
                {
                    "error": {
                        "code": "invalid_request",
                        "message": "limit must be between 1 and 200",
                    }
                },
            )
        before = query.get("before", [None])[0]
        with LOCK:
            msgs = session["messages"]  # list[(event_id, dict)]
            idx = len(msgs)
            if before:
                if not before.startswith("m."):
                    return self._json(
                        422,
                        {
                            "error": {
                                "code": "invalid_request",
                                "message": "invalid message cursor",
                            }
                        },
                    )
                eid = before.removeprefix("m.")
                for i, (mid, _) in enumerate(msgs):
                    if mid == eid:
                        idx = i
                        break
                else:
                    return self._json(
                        422,
                        {
                            "error": {
                                "code": "invalid_request",
                                "message": "invalid message cursor",
                            }
                        },
                    )
            start = max(0, idx - limit)
            page = [message for _, message in msgs[start:idx]]
            older_cursor = f"m.{msgs[start][0]}" if start > 0 else None
        return self._json(200, {"items": page, "older_cursor": older_cursor})

    def _events(self, session):
        q: "queue.Queue" = queue.Queue()
        with LOCK:
            PUBSUB.setdefault(session["session_id"], []).append(q)
            initial = (
                {"state": "thinking"}
                if session["status"] == "working"
                else {"state": "clear"}
            )
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        try:
            self.wfile.write(
                f"event: status\ndata: {json.dumps(initial)}\n\n".encode()
            )
            self.wfile.flush()
            while True:
                item = q.get()
                kind = item[0]
                if kind == "ping":
                    self.wfile.write(b": ping\n\n")
                else:
                    _, name, data = item
                    self.wfile.write(f"event: {name}\ndata: {data}\n\n".encode())
                self.wfile.flush()
        except (BrokenPipeError, OSError):
            pass
        finally:
            with LOCK:
                PUBSUB[session["session_id"]].remove(q)

    def do_POST(self):
        parts, _ = self._route()
        length = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(length) or b"{}")
        if parts == ["v1", "sessions"]:
            s = new_session(
                body.get("profile_id", "dev"),
                body.get("model", "fake-1"),
                body.get("thinking", "low"),
                body.get("workspace", "/tmp"),
            )
            return self._json(
                201,
                {
                    "session_id": s["session_id"],
                    "profile_id": s["profile_id"],
                    "model": s["model"],
                    "thinking": s["thinking"],
                    "workspace": s["workspace"],
                    "status": "wait",
                },
            )
        if len(parts) != 4 or parts[:2] != ["v1", "sessions"]:
            return self._json(
                404,
                {
                    "error": {
                        "code": "not_found",
                        "message": f"route {self.path} not found",
                    }
                },
            )
        session = self._session(parts[2])
        if session is None:
            return self._json(
                404,
                {
                    "error": {
                        "code": "session_not_found",
                        "message": "session not found",
                    }
                },
            )
        if parts[3] == "mailbox":
            content = body.get("content", "")
            if not content:
                return self._json(
                    422,
                    {
                        "error": {
                            "code": "invalid_request",
                            "message": "content is required",
                        }
                    },
                )
            with LOCK:
                item = public_msg(session, "mailbox", content)
                session["status"] = "working"
                session["run_generation"] += 1
                run_generation = session["run_generation"]
            emit_public_message(parts[2], item)
            emit(parts[2], "status", {"state": "thinking"})
            simulate_run(parts[2], content, run_generation)
            self.send_response(202)
            self.end_headers()
            return
        if parts[3] == "cancel":
            if length:
                return self._json(
                    422,
                    {
                        "error": {
                            "code": "invalid_request",
                            "message": "cancel does not accept a request body",
                        }
                    },
                )
            with LOCK:
                session["status"] = "wait"
                session["run_generation"] += 1
            emit(parts[2], "status", {"state": "interrupted"})
            return self._no_content()
        return self._json(
            404,
            {
                "error": {
                    "code": "not_found",
                    "message": f"route {self.path} not found",
                }
            },
        )

    def do_PUT(self):
        parts, _ = self._route()
        length = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(length) or b"{}")
        if len(parts) == 4 and parts[:2] == ["v1", "sessions"] and parts[3] == "selection":
            with LOCK:
                session = SESSIONS.get(parts[2])
                if session is None:
                    return self._json(
                        404,
                        {
                            "error": {
                                "code": "session_not_found",
                                "message": "session not found",
                            }
                        },
                    )
                required = ("profile_id", "model", "thinking")
                if any(not body.get(field) for field in required):
                    return self._json(
                        422,
                        {
                            "error": {
                                "code": "invalid_request",
                                "message": "profile_id, model, and thinking are required",
                            }
                        },
                    )
                session["profile_id"] = body["profile_id"]
                session["model"] = body["model"]
                session["thinking"] = body["thinking"]
                response = session_summary(session)
            return self._json(200, response)
        return self._json(
            404, {"error": {"code": "not_found", "message": "route not found"}}
        )

    def do_DELETE(self):
        parts, _ = self._route()
        if len(parts) == 3 and parts[:2] == ["v1", "sessions"]:
            with LOCK:
                session = SESSIONS.get(parts[2])
                if session is None:
                    return self._json(
                        404,
                        {
                            "error": {
                                "code": "session_not_found",
                                "message": "session not found",
                            }
                        },
                    )
                SESSIONS.pop(parts[2], None)
                PUBSUB.pop(parts[2], None)
            return self._no_content()
        return self._json(
            404, {"error": {"code": "not_found", "message": "route not found"}}
        )


def seed():
    """Seed one session with a long history so 'load older' can be tested."""
    s = new_session("dev", "fake-1", "low", "/workspace/open-worker")
    for i in range(150):
        with LOCK:
            public_msg(s, "mailbox", f"history user message {i}")
            public_msg(s, "assistant", f"history assistant reply {i}")
    emit(s["session_id"], "wait", {"type": "wait", "reason": "idle"})
    print(f"seeded session {s['session_id']} with 300 messages", flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=3010)
    args = ap.parse_args()

    from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler

    class H(Handler, BaseHTTPRequestHandler):
        pass

    seed()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), H)
    # ping thread
    def pings():
        while True:
            time.sleep(10)
            with LOCK:
                queues = [q for qs in PUBSUB.values() for q in qs]
            for q in queues:
                q.put(("ping", None, None))

    threading.Thread(target=pings, daemon=True).start()
    print(f"fake zork-agent listening on 127.0.0.1:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
