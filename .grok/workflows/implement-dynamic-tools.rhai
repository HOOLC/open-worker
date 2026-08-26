let meta = #{
    name: "implement-dynamic-tools",
    description: "Implement the Codex dynamicTools injection refactor end to end via staged subagents",
    phases: [
        #{ title: "Implement", detail: "sequential implementation agents on one shared tree" },
        #{ title: "Verify", detail: "independent verification agents" },
    ],
};

// Shared design contract given verbatim to every implementation agent.
let design = "DESIGN CONTRACT (authoritative, verified by live protocol probe on codex 0.147/0.148):
- Broker is a Slack+Feishu to Codex app-server bridge. Goal: replace curl-recipe tools in thread base instructions with native Codex dynamicTools.
- Protocol facts (verified): initialize already sends capabilities.experimentalApi=true. thread/start accepts a `dynamicTools` array: [{type:\"namespace\", name, description, tools:[{type:\"function\", name, description, inputSchema(JSONSchema)}]}]. When the model calls a tool, the app-server sends a JSON-RPC REQUEST to the client: {method:\"item/tool/call\", id, params:{threadId, turnId, callId, namespace, tool, arguments}}. Client must respond synchronously: {id, result:{contentItems:[{type:\"inputText\", text}], success:true}}. item/started + item/completed notifications with item.type=\"dynamicToolCall\" wrap the call. dynamicTools declarations persist across thread/resume (resume without the field keeps them; do NOT resend on resume).
- Reserved namespace names to avoid: functions, multi_tool_use, file_search, web, browser, image_gen, computer, container, terminal, python, python_user_visible, api_tool, tool_search, submodel_delegator. Use namespaces: chat, coauthor, job, integration.
- Tool calls must be answered within a per-call timeout; on timeout/error answer {success:false, contentItems:[{type:\"inputText\", text:<reason>}]} so the turn never blocks forever.
- Dependency boundary (docs/agent-runtime-protocol.md): src/services/codex/** must NOT import src/services/slack/** or feishu services. Tool backend is injected as an interface defined inside src/services/codex/dynamic-tools.ts.
- Feature flag: config field agentDynamicTools, env AGENT_DYNAMIC_TOOLS, default false. Flag off = byte-identical current behavior (no dynamicTools in thread/start, curl prompt stays).
- Repo state: e2e is the ONLY test tier (vitest, test/*.e2e.test.ts + e2e-broker-part*.test.ts). Mock infra: test/helpers/mock-codex-app-server.ts (WS app-server), test/manual/mock-slack-server.ts, test/helpers/mock-feishu-server.ts. Validate with: npx tsc -p tsconfig.json --noEmit && npx pnpm lint && npx vitest run --no-file-parallelism (full suite ~3min, spawn-heavy).";

phase("Implement");

// Stage 1: protocol layer (already partially started - see repo diff).
let s1 = design + "
YOUR TASK (stage 1 of 4, sequential - earlier stages are already committed to the tree when you start):
The tree already contains a partial protocol-layer start: app-server-client-base.ts has setServerRequestHandler/sendServerResponse/sendServerError, and app-server-client-layer1.ts privateHandleMessage dispatches server requests (id+method) through that handler map, replying -32601 when unregistered. Verify that start compiles and is correct, then complete the rest of stage 1:
1. In src/services/codex/app-server-client-layer1.ts ensureThread: maintain a thread-coordinate map threadId -> {platform, channelId, conversationId, conversationKind, rootThreadTs, rootMessageId, platformThreadId, workspacePath, sessionKey} written on BOTH thread/start and thread/resume paths. Expose lookup via a public method (e.g. getThreadSession(threadId)).
2. thread/start: when a new options.dynamicTools flag/provider is set on the client, include the dynamicTools declaration array in the thread/start RPC params. Add a public setter (e.g. setDynamicToolsProvider(provider: () => DynamicToolsDeclaration[])).
3. Do NOT change thread/resume params (probe verified declarations persist; resending is wrong).
4. Types: define the DynamicToolsDeclaration/DynamicToolFunction shapes in src/services/codex/dynamic-tools.ts (create it in this stage) and import into layer1.
Run: npx tsc -p tsconfig.json --noEmit and npx pnpm lint. Fix what you break. Report: files touched, API added, validation status.";

let r1 = agent(s1, #{ label: "impl:protocol-layer", capability_mode: "read-write" });
if !r1.success { complete(#{ summary: "stage 1 protocol layer failed", detail: r1.output }); }
log("stage 1 done: " + r1.output);

// Stage 2: tool registry + backend interface.
let s2 = design + "
CONTEXT FROM STAGE 1 (already in tree): " + r1.output + "
YOUR TASK (stage 2 of 4):
Fully build out src/services/codex/dynamic-tools.ts:
1. Tool catalog as data. Namespaces: chat(post_message, post_state, post_file, thread_history), coauthor(status, configure), job(register), integration(list_tools, call). Each function: name, description, JSON inputSchema. Tool arguments must NOT include channel/thread coordinates - coordinates are injected server-side from the thread map.
2. interface BrokerToolBackend { postMessage(args, coords); postState(args, coords); postFile(args, coords); threadHistory(args, coords); coauthorStatus(args, coords); coauthorConfigure(args, coords); registerJob(args, coords); listIntegrationTools(args, coords); callIntegration(args, coords); } - all async, args are the model-supplied arguments, coords the resolved thread coordinates.
3. A handleToolCall(params, coordsResolver, backend) dispatcher for item/tool/call params: validate namespace/tool against the catalog, validate arguments against inputSchema (reject unknown/missing), resolve coords by params.threadId (reject unknown thread), execute backend with a 30s timeout, map result to {contentItems:[{type:\"inputText\",text:JSON.stringify(result)}], success:true}; map errors/timeouts to {success:false, contentItems:[{type:\"inputText\",text:reason}]}. String results serialize compactly; keep text under ~100KB.
4. Export buildDynamicToolsDeclaration() returning the wire declaration array from the catalog.
Run: npx tsc -p tsconfig.json --noEmit && npx pnpm lint. Report: catalog surface, dispatcher behavior, validation status.";

let r2 = agent(s2, #{ label: "impl:tool-registry", capability_mode: "read-write" });
if !r2.success { complete(#{ summary: "stage 2 tool registry failed", detail: r2.output }); }
log("stage 2 done: " + r2.output);

// Stage 3: wiring, config flag, prompt dual-version, event normalization.
let s3 = design + "
CONTEXT FROM STAGES 1-2 (already in tree): " + r1.output + " ||| " + r2.output + "
YOUR TASK (stage 3 of 4):
1. src/config.ts: add agentDynamicTools (env AGENT_DYNAMIC_TOOLS, default false) via existing getBoolean.
2. Wiring: in src/services/codex/codex-broker.ts accept an optional toolBackend (the BrokerToolBackend interface); when present AND dynamic tools enabled, wire setServerRequestHandler(\"item/tool/call\", dispatch) and setDynamicToolsProvider on the AppServerClient after connect. Constructor order is codex->runtime->bridge (bridge depends on runtime) so use late binding: CodexBroker.setToolBackend(backend) callable after construction; handlers must resolve backend lazily per call. Wire BOTH construction sites: service-components.ts createCodexBroker path and session-auth-profile-runtime.ts createDefaultProfileRuntime path. The concrete backend implementation lives outside src/services/codex (e.g. src/services/broker-tool-backend.ts) bridging to SlackAgentBridge/JobManager/IsolatedMcpService, constructed in worker-index.ts and index.ts after bridge+jobManager exist.
3. Prompt dual version: src/services/codex/slack-thread-base-instructions.ts + prompts/slack-thread-base-instructions.md - add a tools variant. When dynamic tools enabled, the generated baseInstructions describe the injected tools (chat.post_message etc., including kind=final/block/wait semantics and the existing turn-stopping contract) INSTEAD of the curl recipes for post-message/post-state/post-file/thread-history/coauthor/job-register/integration-mcp. Keep everything else (repo workflow, memory contract, Kimi-free UI contract, linear/notion isolation notes) intact. Both platforms (Slack+Feishu) use the same tool names.
4. Event normalization: src/services/agent-runtime/codex-app-server-runtime.ts - map item/started + item/completed with item.type=\"dynamicToolCall\" to agent.tool.started/agent.tool.completed events (mirror the existing commandExecution branch; name = namespace.tool; input/output = arguments/result).
5. Flag off must equal current behavior exactly.
Run: npx tsc -p tsconfig.json --noEmit && npx pnpm lint. Report: wiring points, prompt variant switch, validation status.";

let r3 = agent(s3, #{ label: "impl:wiring-prompt", capability_mode: "read-write" });
if !r3.success { complete(#{ summary: "stage 3 wiring failed", detail: r3.output }); }
log("stage 3 done: " + r3.output);

// Stage 4: mock infra + e2e coverage.
let s4 = design + "
CONTEXT FROM STAGES 1-3 (already in tree): " + r1.output + " ||| " + r2.output + " ||| " + r3.output + "
YOUR TASK (stage 4 of 4):
1. test/helpers/mock-codex-app-server.ts: record dynamicTools from thread/start; add an invokeTool(threadId, namespace, tool, args) test helper that emits item/started (item.type=dynamicToolCall), sends a real JSON-RPC item/tool/call REQUEST to the connected client, awaits its response (resolve/reject with a 35s timeout), then emits item/completed with the returned contentItems+success.
2. New e2e file test/dynamic-tools.e2e.test.ts (follow e2e-codex-runtime.test.ts patterns: startBrokerProcess or in-process client against MockCodexAppServer, AGENT_DYNAMIC_TOOLS=true): cover (a) mock invokes chat.post_message -> MockSlack receives the posted message with correct thread coordinates; (b) chat.thread_history returns history text in contentItems; (c) job.register persists a job; (d) backend never resolves -> broker answers success:false within timeout and turn continues; (e) item/tool/call for an unknown threadId is rejected; (f) baseInstructions with flag on contain tool names and NOT the migrated curl commands, flag off keep curl commands.
3. Full regression: npx vitest run --no-file-parallelism must be green (including all e2e-broker-part* suites with flag off).
Run the full suite at least once and fix failures you introduced. Report: coverage added, full-suite result.";

let r4 = agent(s4, #{ label: "impl:mock-e2e", capability_mode: "read-write" });
if !r4.success { complete(#{ summary: "stage 4 e2e failed", detail: r4.output }); }
log("stage 4 done: " + r4.output);

phase("Verify");
let vf = design + "
FOUR IMPLEMENTATION AGENTS JUST MODIFIED THE TREE. Their reports: " + r1.output + " ||| " + r2.output + " ||| " + r3.output + " ||| " + r4.output + "
VERIFY INDEPENDENTLY - do not trust the reports; read the actual code:
1. npx tsc -p tsconfig.json --noEmit; npx pnpm lint; npx pnpm format:check - all clean?
2. Flag-off regression: with AGENT_DYNAMIC_TOOLS unset, does thread/start send NO dynamicTools and does the prompt keep curl recipes? Prove by reading the code path, then run npx vitest run test/e2e-broker.test.ts test/e2e-broker-part2.test.ts test/e2e-broker-part3.test.ts test/e2e-broker-part4.test.ts test/e2e-broker-part5.test.ts --no-file-parallelism.
3. Flag-on: npx vitest run test/dynamic-tools.e2e.test.ts --no-file-parallelism.
4. Boundary: grep that src/services/codex/** does not import src/services/slack or feishu; timeout path exists; unknown-thread rejection exists; resume does not resend dynamicTools.
Report pass/fail per check with evidence. Fix ONLY trivial breakage (imports/format); report anything structural.";

let verdict = agent(vf, #{ label: "verify:full", capability_mode: "read-write" });
let verdict_text = if verdict.success { verdict.output } else { "VERIFICATION AGENT FAILED: " + verdict.output };

complete(#{ summary: "dynamicTools implementation finished", stage1: r1.output, stage2: r2.output, stage3: r3.output, stage4: r4.output, verification: verdict_text });
