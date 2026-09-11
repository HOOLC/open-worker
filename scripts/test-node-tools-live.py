#!/usr/bin/env python3
"""Opt-in real-LLM acceptance of device/MCP/skill tools on two isolated Gateways.

Build current binaries first. Pass an existing profile and exact model explicitly.
The model receives ordinary user requests; the harness only prepares fixtures and
checks independent filesystem, MCP and durable-event evidence. Credentials are
never written to reports, and temporary profile copies are removed on every exit.
"""
import argparse
import importlib.util
import json
import os
import re
from pathlib import Path
import secrets
import shutil
import sqlite3
import subprocess
import tempfile
import time
import traceback

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("mcp_fixture", ROOT / "scripts/test-mcp.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
f = m.f


def ok(response):
    assert response[0] in (200, 201, 202), response
    return response[1]


def events(node, session):
    result = []
    for path in sorted((node.root / "sessions" / session / "segments").glob("*.jsonl")):
        for line in path.read_text().splitlines():
            try:
                result.append(json.loads(line)["event"])
            except json.JSONDecodeError:
                pass  # Writer may be in the middle of its next durable append.
    return result


class LiveNode(f.Node):
    def start(self):
        self.log = (self.root / "supervisor.log").open("ab")
        self.process = subprocess.Popen(
            [str(f.TARGET / "zork"), "start", "--data", str(self.root)],
            stdout=self.log, stderr=self.log, start_new_session=True,
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--thinking")
    parser.add_argument("--context-tokens", type=int)
    parser.add_argument("--output-tokens", type=int)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--phase-timeout", type=int, default=900)
    parser.add_argument("--expect-call-wait", action="store_true")
    args = parser.parse_args()
    profile = json.loads(args.profile.read_text())
    model = next(v for v in profile["models"] if v["id"] == args.model)
    thinking = args.thinking or model.get("default_thinking", "off")
    if not model.get("limits"):
        assert args.context_tokens and args.output_tokens, "profile needs explicit model limits"
        model["limits"] = {"context_window_tokens": args.context_tokens, "max_output_tokens": args.output_tokens}
    # Existing access credentials suffice for this bounded run; do not rotate the
    # original account's refresh token from an isolated test profile.
    if profile.get("auth", {}).get("type") == "oauth":
        expiry = profile["auth"].get("expires", 0)
        assert expiry > time.time() * 1000 + args.phase_timeout * 3000 + 300000, "subscription access expires too soon"
        profile["auth"].pop("refresh", None)
    redactions = [str(v) for k, v in profile.get("auth", {}).items()
                  if k in ("access", "refresh", "key", "email") and v]

    def redact(value):
        text = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False, indent=2)
        for secret in redactions:
            text = text.replace(secret, "[REDACTED]")
        return text

    args.output.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="zork-node-tools-live-"))
    os.chmod(scratch, 0o700)
    nodes, sessions, checks, phases = [], [], [], []
    report = {"model": args.model, "thinking": thinking, "provider": profile.get("provider"),
              "billing": profile.get("billing"), "fake_agent": False, "model_limits": model["limits"],
              "topology": "two isolated Gateways on one host", "phases": phases, "checks": checks}
    failure = None
    counter = 0
    started = time.monotonic()
    print(json.dumps({"fixture": str(scratch), "model": args.model, "thinking": thinking}), flush=True)
    try:
        a = LiveNode(scratch / "coordinator"); nodes.append(a)
        b = LiveNode(scratch / "test-device"); nodes.append(b)
        a.pair(b); b.pair(a)
        for node in nodes:
            node.config["mesh"]["peers"][0]["client"] = True
            node.config["mesh"]["name"] = node.root.name
            node.config["admin"] = {"token": "mcp-fixture"}
            for workspace in node.config["mesh"]["workspaces"]:
                workspace.update(profile_id="live", model=args.model, thinking=thinking)
            (node.root / "profiles/fixture.json").unlink()
            path = node.root / "profiles/live.json"
            path.write_text(json.dumps(profile)); path.chmod(0o600)
            (node.root / "config.json").write_text(json.dumps(node.config))
            node.request = lambda method, path, body=None, n=node: m.request(n, method, path, body)
            node.start()
        for node in nodes:
            f.wait(lambda n=node: n.request("GET", "/readyz")[0] == 200, "ready")
            f.wait(lambda n=node: n.get("/v1/mesh").get("origin") == n.origin, "Mesh identity")
        created = ok(a.request("POST", "/v1/im/sessions", {
            "profile_id": "live", "model": args.model, "thinking": thinking, "workspace": str(a.workspace)}))
        coordinator = created["session_id"]; sessions.append((a, coordinator))
        seed = b.workspace / "seed"; seed.mkdir()
        (seed / "echo-server.py").write_text(m.FIXTURE)
        proof = "skill-proof-" + secrets.token_hex(8)
        guide = seed / "guide"; (guide / "scripts").mkdir(parents=True)
        (guide / "SKILL.md").write_text(
            "---\nname: live-echo-guide\ndescription: Process live acceptance echo requests using the installed live-echo MCP and its packaged helper.\n---\n"
            "For an echo acceptance request, run scripts/helper.py from this skill directory with the user's input as one argument. "
            "The helper adds the installation proof tag; do not invent the tag. Find the live-echo MCP, inspect echo, "
            "call it with the helper output as text, await completion, and report the exact returned text.\n")
        (guide / "scripts/helper.py").write_text(f"import sys\nprint({proof!r} + ':' + sys.argv[1])\n")
        old = b.workspace / "existing-guide"; old.mkdir()
        (old / "SKILL.md").write_text("---\nname: existing-guide\ndescription: Existing skill that must survive management operations\n---\nPreserve this source.\n")
        ok(b.request("POST", "/v1/node/agents", {"id": "research", "name": "Research", "role": "leader",
            "profile_id": "live", "model": args.model, "thinking": thinking, "skill_paths": [str(old)]}))
        researcher = ok(b.request("POST", "/v1/node/agents/research/open", {}))["session_id"]
        sessions.append((b, researcher))

        def run_phase(label, node, sid, prompt):
            nonlocal counter
            counter += 1
            if args.expect_call_wait:
                prompt += " 调用工具时，请自行预估多久后值得查看该工具进度或结果，把预估秒数填写在统一 call 外层的 wait 参数中；具体时长由你判断。"
            before = len(events(node, sid)); phase_start = time.monotonic()
            ok(node.request("POST", f"/v1/im/sessions/{sid}/messages", {"content": prompt, "request_id": f"live-{counter}"}))
            phase = {"name": label, "session_id": sid, "prompt": prompt}
            phases.append(phase)
            print("START " + label, flush=True)
            seen = set()
            while time.monotonic() - phase_start < args.phase_timeout:
                current = events(node, sid)[before:]
                for event in current:
                    if event["kind"] == "tool_result":
                        result = event["result"]
                        if result["invocation_id"] not in seen:
                            seen.add(result["invocation_id"])
                            print(json.dumps({"phase": label, "tool": result["tool"], "outcome": result["outcome"]}), flush=True)
                terminal = next((e for e in reversed(current) if e["kind"] == "turn_finished"), None)
                if terminal:
                    phase["wait_estimates"] = [{"tool":call["arguments"].get("tool"),"wait":call["arguments"]["wait"]} for event in current if event["kind"] == "step_completed" for call in event.get("provider_calls",[]) if isinstance(call.get("arguments",{}).get("wait"),(int,float))]
                    if args.expect_call_wait: assert phase["wait_estimates"], "model did not provide a call.wait estimate"
                    phase["outcome"] = terminal["outcome"]
                    phase["duration_seconds"] = round(time.monotonic() - phase_start, 2)
                    phase["tools"] = [e["result"]["tool"] for e in current if e["kind"] == "tool_result"]
                    forbidden={"device.jobs","mcp","mcp.setup","mcp.status","mcp.read","mcp.cancel","mcp.recover","device.status","device.read","device.cancel","device.recover"}
                    assert not forbidden.intersection(phase["tools"]), phase["tools"]
                    phase["failures"] = [e["result"] for e in current if e["kind"] == "tool_result" and e["result"]["outcome"] != "succeeded"]
                    assert terminal["outcome"] == "finished", terminal
                    phase["outstanding"] = terminal.get("outstanding", [])
                    assert not phase["outstanding"], terminal
                    invocations = [i for e in current if e["kind"] == "step_completed" for i in e.get("invocations", [])]
                    finals = [i for i in invocations if i["tool"] == "chat.post_message" and i["arguments"].get("kind") == "final"]
                    assert len(finals) == 1, "expected one conclusive user-visible final, without repeated closure"
                    phase["final_messages"] = len(finals)
                    for e in current:
                        if e["kind"] == "tool_result" and e["result"]["tool"] in ("mcp", "mcp.inspect"):
                            for item in e["result"]["data"].get("items", []):
                                if "definition" not in item:
                                    continue
                                definition = item["definition"]
                                assert "inputSchema" not in definition and "type Arguments" in definition["parameters"], definition
                    assert any(e["kind"] == "step_completed" and e.get("usage") for e in current), "missing real provider usage"
                    return current
                time.sleep(1)
            raise AssertionError("real-model phase timed out: " + label)

        def node_tool(name, arguments):
            nonlocal counter
            counter += 1
            return ok(a.request("POST", "/v1/node-tools", {"session_id": coordinator,
                "invocation_id": f"oracle-{counter}", "tool": name, "arguments": arguments}))

        def mcp_tool(op, **arguments):
            nonlocal counter
            counter += 1
            return ok(a.request("POST", "/v1/mcp", {"session_id": coordinator,
                "invocation_id": f"oracle-{counter}", "request": {"op": op, **arguments}}))

        run_phase("remote_install_and_bind", a, coordinator,
            "请实际完成这个测试环境的能力安装。先发现设备，目标名称是 test-device，确认它的系统和 Python。"
            f"它的测试工作区是 {b.workspace}。其中 seed/echo-server.py 是现成的 stdio MCP 程序，运行方式为 python3 <程序绝对路径> <调用日志绝对路径>。"
            "请在目标设备新建 prepared 目录，复制程序到 prepared/echo-server.py，写入 prepared/ready.txt 内容 READY，并把 MCP 调用日志设为 prepared/calls.jsonl。"
            "安装名为 live-echo 的 MCP，授权 Mesh 使用，实际 probe 确认 echo 工具可用。"
            f"然后把 {guide} 的 skill（包括 scripts/helper.py 资源）导入托管包，并绑定到该设备已有的 Research Agent。"
            "保留它已有的 skill 来源。检查最终安装和绑定结果，向聊天提交简短结果后结束。"
            "这是已授权的隔离测试，只操作这些测试目录；不要读取 profiles 或凭据，不要手工编辑 Zork 数据库或配置文件来绕过管理工具。")
        prepared = b.workspace / "prepared"
        assert (prepared / "ready.txt").read_text().strip() == "READY"
        servers = mcp_tool("installed", owner=b.origin)["items"]
        server = next(v["server"] for v in servers if v["server"].get("name") == "live-echo")
        assert re.fullmatch(r"[0-7][0-9A-HJKMNP-TV-Z]{25}", server["server_ref"]["server_id"]), server
        installed = node_tool("skill.installed", {"target": b.origin})["items"]
        skill = next(v for v in installed if v.get("name") == "live-echo-guide")
        skill_id = skill["skill_id"]
        assert re.fullmatch(r"[0-7][0-9A-HJKMNP-TV-Z]{25}", skill_id), skill
        assert (b.root / "managed-skills" / skill_id / "scripts/helper.py").read_text() == (guide / "scripts/helper.py").read_text()
        bindings = node_tool("skill.bindings", {"target": b.origin, "skill_id": skill_id})
        assert "research" in json.dumps(bindings), bindings
        checks.append("real_model_prepares_remote_device_installs_mcp_and_binds_resource_skill")

        user_input = "grok-live-" + secrets.token_hex(6)
        run_phase("research_uses_bound_skill", b, researcher,
            f"请用刚为你绑定的 live-echo-guide skill 处理这条 echo 验收输入：{user_input}。"
            "遵循 skill 的具体步骤，实际调用已安装的 live-echo MCP，拿到结果后将返回文本原样发到聊天，不能模拟结果。")
        calls = [json.loads(line) for line in (prepared / "calls.jsonl").read_text().splitlines()]
        expected = proof + ":" + user_input
        assert any(v.get("text") == expected for v in calls), calls
        messages = ok(b.request("GET", f"/v1/im/sessions/{researcher}/messages"))
        assert expected in json.dumps(messages, ensure_ascii=False), "exact MCP proof not delivered to user"
        checks.append("real_research_agent_loads_resource_helper_calls_mcp_and_delivers_verified_result")

        cleanup_events = run_phase("cancel_copy_and_cleanup", a, coordinator,
            "继续验收刚才的环境。先在 test-device 执行一个打印 LIVE-CANCEL-READY 后等待 120 秒的前台命令。"
            "用 file.read 读取普通工具的 live 日志，看到标记后用 tool.cancel 取消该 invocation，并等待它的普通工具完成结果确认进程终止，不要等待 120 秒自然结束。"
            "然后把刚才的 live-echo-guide 托管 skill 复制到 coordinator 设备，确认副本带有 scripts/helper.py。"
            "从 test-device 的 Research Agent 解绑原 skill，然后归档卸载原包；保留原有 existing-guide 来源和 coordinator 上的副本。"
            "最后卸载 test-device 上的 live-echo MCP。检查最终安装状态，向聊天提交实际结果后结束。"
            "仍然只使用正式工具，不手工编辑内部数据库或配置文件。确认终止不代表撤销已经发生的副作用，不要重跑被取消的命令。")
        copied = node_tool("skill.installed", {"target": a.origin})["items"]
        assert len(copied) == 1, copied
        assert (a.root / "managed-skills" / copied[0]["skill_id"] / "scripts/helper.py").read_text() == (guide / "scripts/helper.py").read_text()
        assert not node_tool("skill.installed", {"target": b.origin})["items"]
        assert (b.root / "managed-skills/.archive" / skill_id / "scripts/helper.py").exists()
        archived_bindings = node_tool("skill.bindings", {"target": b.origin, "skill_id": skill_id})
        assert archived_bindings["package_state"] == "archived" and not archived_bindings["agents"], archived_bindings
        report["archived_binding_query"] = archived_bindings["package_state"]
        assert not mcp_tool("installed", owner=b.origin)["items"]
        assert (old / "SKILL.md").exists()
        with sqlite3.connect(b.root / "state/gateway.sqlite") as db:
            agent = json.loads(db.execute("SELECT value FROM node_agents WHERE id='research'").fetchone()[0])
        remaining = {Path(p).resolve() for p in agent["skill_paths"]}
        assert old.resolve() in remaining, "existing skill lost its Agent binding"
        assert (b.root / "managed-skills" / skill_id).resolve() not in remaining, "archived skill still bound"
        report["existing_skill_binding_preserved"] = True
        cancellations = [e["result"] for e in cleanup_events if e["kind"] == "tool_result" and e["result"]["tool"] == "device.exec" and e["result"]["outcome"] == "cancelled"]
        assert cancellations and "tool.cancel" in phases[-1]["tools"], "model did not cancel an active invocation"
        cancelled=cancellations[0]["data"]
        assert cancelled["state"] == "cancelled" and cancelled["result"]["process_state"] == "exited", cancelled
        pid=cancelled["result"]["pid"]
        assert subprocess.run(["ps","-p",str(pid),"-o","pid="],capture_output=True).returncode != 0, "completion preceded process cleanup"
        assert cancelled["result"]["effects_may_have_occurred"] is True
        report["cancellation"] = {"process_exited": True, "receipt_state": cancelled["state"]}
        checks.append("real_model_cancels_device_process_copies_skill_unbinds_archives_and_removes_mcp")
        report["outcome"] = "passed"
    except BaseException as error:
        failure = error
        report["outcome"] = "failed"
        report["error"] = redact(traceback.format_exc())
        print(redact(traceback.format_exc()), flush=True)
    finally:
        for node in reversed(nodes):
            node.stop()
        all_events = []
        for node, sid in sessions:
            stream = events(node, sid)
            all_events.extend(stream)
            (args.output / f"{node.root.name}-{sid}-events.json").write_text(redact(stream))
        for node in nodes:
            log = node.root / "supervisor.log"
            if log.exists():
                (args.output / f"{node.root.name}.log").write_text(redact(log.read_text(errors="replace")))
        usage = [e["usage"] for e in all_events if e["kind"] == "step_completed" and e.get("usage")]
        report["usage"] = {key: sum(v.get(key) or 0 for v in usage) for key in
                           ("input_tokens", "output_tokens", "cached_input_tokens", "output_reasoning_tokens")}
        report["model_steps"] = len(usage)
        report["duration_seconds"] = round(time.monotonic() - started, 2)
        report["binaries"] = {p.name: subprocess.check_output(["shasum", "-a", "256", str(p)], text=True).split()[0]
                              for p in (f.TARGET / "zork", f.TARGET / "zork-station")}
        (args.output / "report.json").write_text(redact(report))
        shutil.rmtree(scratch)
        print(json.dumps({"outcome": report["outcome"], "report": str(args.output / "report.json"), "model_steps": len(usage)}), flush=True)
    if failure:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
