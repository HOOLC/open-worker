#!/usr/bin/env python3
"""End-to-end contract checks for the standalone fake zork-agent."""

from __future__ import annotations

import http.client
import json
import socket
import subprocess
import sys
import time
import unittest
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


HERE = Path(__file__).resolve().parent
FAKE_AGENT = HERE / "fake_agent.py"


class FakeAgentContractTest(unittest.TestCase):
    process: subprocess.Popen[bytes]
    base_url: str
    port: int
    session_id: str

    @classmethod
    def setUpClass(cls) -> None:
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            cls.port = probe.getsockname()[1]
        cls.base_url = f"http://127.0.0.1:{cls.port}"
        cls.process = subprocess.Popen(
            [sys.executable, str(FAKE_AGENT), "--port", str(cls.port)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            try:
                status, body = cls.request("GET", "/v1/sessions")
                if status == 200:
                    cls.session_id = body["items"][0]["session_id"]
                    return
            except (ConnectionError, URLError):
                pass
            time.sleep(0.05)
        cls.process.terminate()
        raise RuntimeError("fake agent did not become ready")

    @classmethod
    def tearDownClass(cls) -> None:
        cls.process.terminate()
        try:
            cls.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            cls.process.kill()
            cls.process.wait(timeout=3)

    @classmethod
    def request(
        cls, method: str, path: str, body: dict[str, object] | None = None
    ) -> tuple[int, dict[str, object]]:
        encoded = None if body is None else json.dumps(body).encode()
        request = Request(
            f"{cls.base_url}{path}",
            data=encoded,
            method=method,
            headers={"content-type": "application/json"},
        )
        try:
            response = urlopen(request, timeout=2)
        except HTTPError as error:
            response = error
        with response:
            payload = response.read()
            return response.status, json.loads(payload) if payload else {}

    def test_profiles_and_sessions_are_available(self) -> None:
        profile_status, profiles = self.request("GET", "/v1/profiles")
        session_status, sessions = self.request("GET", "/v1/sessions")

        self.assertEqual(profile_status, 200)
        self.assertEqual(profiles["items"][0]["profile_id"], "dev")
        self.assertEqual(session_status, 200)
        self.assertEqual(sessions["items"][0]["session_id"], self.session_id)

    def test_create_get_and_delete_session(self) -> None:
        create_status, created = self.request(
            "POST",
            "/v1/sessions",
            {
                "profile_id": "dev",
                "model": "fake-1",
                "thinking": "low",
                "workspace": "/tmp/fake-agent-contract",
            },
        )
        created_id = created["session_id"]
        get_status, fetched = self.request("GET", f"/v1/sessions/{created_id}")
        delete_status, delete_body = self.request(
            "DELETE", f"/v1/sessions/{created_id}"
        )
        missing_status, _ = self.request("GET", f"/v1/sessions/{created_id}")

        self.assertEqual(create_status, 201)
        self.assertEqual(get_status, 200)
        self.assertEqual(fetched, created)
        self.assertEqual(delete_status, 204)
        self.assertEqual(delete_body, {})
        self.assertEqual(missing_status, 404)

    def test_message_pages_use_the_real_mailbox_role(self) -> None:
        status, page = self.request(
            "GET", f"/v1/sessions/{self.session_id}/messages?limit=2"
        )

        self.assertEqual(status, 200)
        self.assertEqual([item["role"] for item in page["items"]], ["mailbox", "assistant"])
        self.assertIsNotNone(page["older_cursor"])

        all_items: list[dict[str, object]] = []
        cursor: str | None = None
        while True:
            suffix = "" if cursor is None else f"&before={cursor}"
            page_status, current = self.request(
                "GET",
                f"/v1/sessions/{self.session_id}/messages?limit=47{suffix}",
            )
            self.assertEqual(page_status, 200)
            all_items[0:0] = current["items"]
            cursor = current["older_cursor"]
            if cursor is None:
                break
        self.assertEqual(len(all_items), 300)
        self.assertEqual(all_items[0]["content"], "history user message 0")
        self.assertEqual(all_items[-1]["content"], "history assistant reply 149")

    def test_selection_and_mailbox_match_real_response_contracts(self) -> None:
        selection_status, selected = self.request(
            "PUT",
            f"/v1/sessions/{self.session_id}/selection",
            {"profile_id": "dev", "model": "fake-2", "thinking": "off"},
        )
        mailbox_status, _ = self.request(
            "POST",
            f"/v1/sessions/{self.session_id}/mailbox",
            {"content": "hello from contract test"},
        )

        self.assertEqual(selection_status, 200)
        self.assertEqual(selected["model"], "fake-2")
        self.assertEqual(selected["thinking"], "off")
        self.assertEqual(mailbox_status, 202)

    def test_sse_starts_with_status_and_cancel_returns_no_content(self) -> None:
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=2)
        try:
            connection.request("GET", f"/v1/sessions/{self.session_id}/events")
            response = connection.getresponse()
            self.assertEqual(response.status, 200)
            self.assertEqual(response.readline().decode().strip(), "event: status")
            data = response.readline().decode().strip()
            self.assertTrue(data.startswith("data: "))
            self.assertIn(json.loads(data.removeprefix("data: "))["state"], {"clear", "thinking"})
        finally:
            connection.close()

        cancel_status, cancel_body = self.request(
            "POST", f"/v1/sessions/{self.session_id}/cancel"
        )
        self.assertEqual(cancel_status, 204)
        self.assertEqual(cancel_body, {})

    def test_interruptible_fixture_stays_stopped_after_cancel(self) -> None:
        session_id = self._create_scenario_session()
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=2)
        try:
            connection.request("GET", f"/v1/sessions/{session_id}/events")
            response = connection.getresponse()
            self.assertEqual(response.status, 200)
            self._read_sse_event(response)  # initial clear status

            mailbox_status, _ = self.request(
                "POST",
                f"/v1/sessions/{session_id}/mailbox",
                {"content": "fixture:interruptible"},
            )
            self.assertEqual(mailbox_status, 202)

            working_status, working = self.request(
                "GET", f"/v1/sessions/{session_id}"
            )
            self.assertEqual(working_status, 200)
            self.assertEqual(working["status"], "working")

            cancel_status, _ = self.request(
                "POST", f"/v1/sessions/{session_id}/cancel"
            )
            self.assertEqual(cancel_status, 204)

            interrupted = None
            for _ in range(6):
                event = self._read_sse_event(response)
                if (
                    event["name"] == "status"
                    and event["data"].get("state") == "interrupted"
                ):
                    interrupted = event
                    break
            self.assertIsNotNone(interrupted)

            time.sleep(0.8)
            page_status, page = self.request(
                "GET", f"/v1/sessions/{session_id}/messages?limit=50"
            )
            self.assertEqual(page_status, 200)
            self.assertEqual(
                [(item.get("type"), item.get("role")) for item in page["items"]],
                [("message", "mailbox")],
            )
        finally:
            connection.close()

    def test_message_rendering_scenarios_cover_markdown_unicode_and_failure(
        self,
    ) -> None:
        markdown_session = self._create_scenario_session()
        markdown_events = self._run_scenario(markdown_session, "fixture:markdown")
        messages = [
            event["data"]
            for event in markdown_events
            if event["name"] == "message"
            and event["data"].get("type") == "message"
        ]
        assistant = [item["content"] for item in messages if item.get("role") == "assistant"]
        tools = [item["content"] for item in messages if item.get("role") == "tool"]

        self.assertTrue(any("# Markdown fixture" in content for content in assistant))
        self.assertTrue(any("```rust" in content for content in assistant))
        self.assertTrue(any("你好 😀" in content for content in assistant))
        self.assertTrue(any("tool tail sentinel" in content for content in tools))
        self.assertTrue(any(len(content) > 1_500 for content in tools))

        failed_session = self._create_scenario_session()
        failed_events = self._run_scenario(failed_session, "fixture:failed")
        failed = [
            event["data"]
            for event in failed_events
            if event["name"] == "status" and event["data"].get("state") == "failed"
        ]
        self.assertEqual(failed, [{"state": "failed", "reason": "fixture model failure"}])

    def _create_scenario_session(self) -> str:
        status, created = self.request(
            "POST",
            "/v1/sessions",
            {
                "profile_id": "dev",
                "model": "fake-1",
                "thinking": "low",
                "workspace": "/tmp/message-rendering-scenario",
            },
        )
        self.assertEqual(status, 201)
        return str(created["session_id"])

    def _run_scenario(
        self, session_id: str, content: str
    ) -> list[dict[str, object]]:
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        events: list[dict[str, object]] = []
        try:
            connection.request("GET", f"/v1/sessions/{session_id}/events")
            response = connection.getresponse()
            self.assertEqual(response.status, 200)
            self._read_sse_event(response)  # initial clear status

            mailbox_status, _ = self.request(
                "POST",
                f"/v1/sessions/{session_id}/mailbox",
                {"content": content},
            )
            self.assertEqual(mailbox_status, 202)

            for _ in range(32):
                event = self._read_sse_event(response)
                events.append(event)
                data = event["data"]
                if event["name"] == "status" and (
                    data.get("state") == "failed"
                    or (
                        data.get("state") == "waiting"
                        and data.get("reason") == "job completion"
                    )
                ):
                    break
            return events
        finally:
            connection.close()

    def _read_sse_event(
        self, response: http.client.HTTPResponse
    ) -> dict[str, object]:
        name = "message"
        data: dict[str, object] = {}
        while True:
            line = response.readline().decode().rstrip("\r\n")
            if not line:
                return {"name": name, "data": data}
            if line.startswith("event: "):
                name = line.removeprefix("event: ")
            elif line.startswith("data: "):
                data = json.loads(line.removeprefix("data: "))


if __name__ == "__main__":
    unittest.main()
