import { execFile, spawn } from "node:child_process";
import fs from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

function parseArguments(argv) {
  const values = {
    binary: "target/release/zork-agent",
    reasoningItems: 5_500,
    ciphertextBytes: 1_280,
    repetitions: 3,
  };
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    const value = argv[index + 1];
    if (value === undefined) throw new Error(`missing value for ${name}`);
    if (name === "--binary") values.binary = value;
    else if (name === "--reasoning-items") values.reasoningItems = Number(value);
    else if (name === "--ciphertext-bytes") values.ciphertextBytes = Number(value);
    else if (name === "--repetitions") values.repetitions = Number(value);
    else throw new Error(`unknown argument ${name}`);
  }
  for (const [name, value] of Object.entries(values)) {
    if (name !== "binary" && (!Number.isInteger(value) || value <= 0)) {
      throw new Error(`${name} must be a positive integer`);
    }
  }
  return values;
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function freePort() {
  const server = http.createServer();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("failed to allocate port");
  await new Promise((resolve) => server.close(resolve));
  return address.port;
}

function ciphertext(index, bytes) {
  const prefix = `cipher-${index}-`;
  if (prefix.length > bytes) throw new Error("ciphertext size is too small for the item id");
  return prefix + "x".repeat(bytes - prefix.length);
}

function responseDocument(reasoningItems, ciphertextBytes) {
  const output = [];
  for (let index = 0; index < reasoningItems; index += 1) {
    output.push({
      id: `rs_${index}`,
      type: "reasoning",
      status: "completed",
      encrypted_content: ciphertext(index, ciphertextBytes),
      summary: [],
    });
  }
  output.push({
    id: "fc_end",
    type: "function_call",
    status: "completed",
    arguments: "{}",
    call_id: "call_end",
    name: "end",
  });
  return {
    id: "resp_transport_benchmark",
    object: "response",
    created_at: 1,
    model: "transport-benchmark-model",
    status: "completed",
    incomplete_details: null,
    output,
    usage: {
      input_tokens: 100,
      input_tokens_details: { cached_tokens: 0 },
      output_tokens: reasoningItems + 1,
      output_tokens_details: { reasoning_tokens: reasoningItems },
    },
  };
}

function streamingResponse(document) {
  const chunks = [
    {
      type: "response.created",
      response: {
        id: document.id,
        created_at: document.created_at,
        model: document.model,
      },
    },
  ];
  for (let index = 0; index < document.output.length - 1; index += 1) {
    const item = document.output[index];
    chunks.push({
      type: "response.output_item.added",
      output_index: index,
      item: { id: item.id, type: "reasoning", status: "in_progress", summary: [] },
    });
    chunks.push({ type: "response.output_item.done", output_index: index, item });
  }
  const toolIndex = document.output.length - 1;
  const tool = document.output[toolIndex];
  chunks.push({
    type: "response.output_item.added",
    output_index: toolIndex,
    item: { ...tool, status: "in_progress", arguments: "" },
  });
  chunks.push({
    type: "response.function_call_arguments.delta",
    item_id: tool.id,
    output_index: toolIndex,
    delta: tool.arguments,
  });
  chunks.push({
    type: "response.function_call_arguments.done",
    item_id: tool.id,
    output_index: toolIndex,
    arguments: tool.arguments,
  });
  chunks.push({ type: "response.output_item.done", output_index: toolIndex, item: tool });
  chunks.push({
    type: "response.completed",
    response: {
      id: document.id,
      created_at: document.created_at,
      model: document.model,
      incomplete_details: null,
      usage: document.usage,
    },
  });
  return `${chunks.map((event) => `data: ${JSON.stringify(event)}\n\n`).join("")}data: [DONE]\n\n`;
}

function cpuMilliseconds(value) {
  const fields = value.trim().split(":").map(Number);
  if (fields.some(Number.isNaN)) throw new Error(`invalid process time ${value}`);
  if (fields.length === 2) return (fields[0] * 60 + fields[1]) * 1_000;
  if (fields.length === 3) return (fields[0] * 3_600 + fields[1] * 60 + fields[2]) * 1_000;
  throw new Error(`invalid process time ${value}`);
}

async function processSample(processId) {
  const { stdout } = await execFileAsync("ps", ["-p", String(processId), "-o", "rss=,utime=,stime="]);
  const fields = stdout.trim().split(/\s+/);
  if (fields.length !== 3) throw new Error(`process ${processId} disappeared`);
  return {
    rssKiB: Number(fields[0]),
    cpuMs: cpuMilliseconds(fields[1]) + cpuMilliseconds(fields[2]),
  };
}

async function waitForReady(baseUrl, child) {
  const deadline = performance.now() + 20_000;
  while (performance.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`zork-agent exited with ${child.exitCode}`);
    try {
      const response = await fetch(`${baseUrl}/readyz`);
      if (response.ok) return;
    } catch {}
    await delay(20);
  }
  throw new Error("zork-agent did not become ready");
}

async function waitForWait(baseUrl, sessionId) {
  const deadline = performance.now() + 30_000;
  while (performance.now() < deadline) {
    const response = await fetch(`${baseUrl}/v1/sessions/${sessionId}`);
    if (!response.ok) throw new Error(`session status failed with ${response.status}`);
    const document = await response.json();
    if (document.status === "wait") return;
    await delay(10);
  }
  throw new Error("session did not finish");
}

async function stopChild(child) {
  if (child.exitCode !== null) return;
  child.kill("SIGTERM");
  await Promise.race([new Promise((resolve) => child.once("exit", resolve)), delay(2_000).then(() => child.kill("SIGKILL"))]);
}

async function runOnce({ binary, mode, reasoningItems, ciphertextBytes, sequence }) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), `zork-transport-${mode}-`));
  const dataRoot = path.join(root, "data");
  const profileRoot = path.join(dataRoot, "profiles");
  const workspace = path.join(root, "workspace");
  const agentPort = await freePort();
  const document = responseDocument(reasoningItems, ciphertextBytes);
  const responseBody = mode === "streaming" ? streamingResponse(document) : JSON.stringify(document);
  let capturedRequest;
  let resolveRequest;
  const requestPromise = new Promise((resolve) => {
    resolveRequest = resolve;
  });
  const provider = http.createServer((request, response) => {
    if (request.method === "GET" && request.url === "/v1/models") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ object: "list", data: [{ id: "transport-benchmark-model", object: "model" }] }));
      return;
    }
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => {
      capturedRequest = { request, response, body: Buffer.concat(chunks) };
      resolveRequest(capturedRequest);
    });
  });
  await new Promise((resolve) => provider.listen(0, "127.0.0.1", resolve));
  const providerAddress = provider.address();
  if (!providerAddress || typeof providerAddress === "string") throw new Error("provider did not bind");
  await Promise.all([fs.mkdir(profileRoot, { recursive: true }), fs.mkdir(workspace)]);
  await fs.writeFile(path.join(dataRoot, "config.json"), `${JSON.stringify({ bind: { agent: `127.0.0.1:${agentPort}` } })}\n`);
  await fs.writeFile(
    path.join(profileRoot, "transport.json"),
    `${JSON.stringify({
      provider: "openai",
      billing: "usage",
      base_url: `http://127.0.0.1:${providerAddress.port}/v1`,
      headers: {},
      auth: { type: "api_key", key: "transport-benchmark-secret" },
      models: [
        {
          id: "transport-benchmark-model",
          api: "openai-responses",
          streaming: true,
          thinking: ["high"],
          default_thinking: "high",
          capabilities: { input: ["text"] },
          limits: { context_window_tokens: 1_000_000, max_output_tokens: 100_000 },
          default: true,
        },
      ],
    })}\n`,
  );

  const agentArguments = ["--data", dataRoot];
  if (mode === "non_streaming") agentArguments.push("--no-streaming");
  const child = spawn(path.resolve(binary), agentArguments, { stdio: ["ignore", "pipe", "pipe"] });
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += chunk.toString();
  });
  const baseUrl = `http://127.0.0.1:${agentPort}`;
  try {
    await waitForReady(baseUrl, child);
    const baseline = await processSample(child.pid);
    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        profile_id: "transport",
        model: "transport-benchmark-model",
        thinking: "high",
        system_prompt: "Call end immediately.",
        workspace,
      }),
    });
    if (created.status !== 201) throw new Error(`session creation failed: ${created.status} ${await created.text()}`);
    const session = await created.json();
    const mailbox = await fetch(`${baseUrl}/v1/sessions/${session.session_id}/mailbox`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ content: "finish" }),
    });
    if (mailbox.status !== 202) throw new Error(`mailbox failed with ${mailbox.status}`);
    const providerRequest = await requestPromise;
    const requestDocument = JSON.parse(providerRequest.body.toString("utf8"));
    const expectedStream = mode === "streaming";
    if ((requestDocument.stream === true) !== expectedStream) {
      throw new Error(`profile ${mode} produced stream=${String(requestDocument.stream)}`);
    }

    const samples = [baseline, await processSample(child.pid)];
    let sampling = true;
    const sampler = (async () => {
      while (sampling) {
        try {
          samples.push(await processSample(child.pid));
        } catch {
          break;
        }
        await delay(5);
      }
    })();
    const startSample = await processSample(child.pid);
    const startedAt = performance.now();
    providerRequest.response.writeHead(200, {
      "content-type": mode === "streaming" ? "text/event-stream" : "application/json",
      "content-length": Buffer.byteLength(responseBody),
    });
    providerRequest.response.end(responseBody);
    await waitForWait(baseUrl, session.session_id);
    const finishedAt = performance.now();
    const endSample = await processSample(child.pid);
    sampling = false;
    await sampler;
    samples.push(endSample);
    return {
      sequence,
      mode,
      reasoning_items: reasoningItems,
      ciphertext_bytes_per_item: ciphertextBytes,
      request_bytes: providerRequest.body.length,
      response_bytes: Buffer.byteLength(responseBody),
      wall_ms: Number((finishedAt - startedAt).toFixed(3)),
      cpu_ms: endSample.cpuMs - startSample.cpuMs,
      baseline_rss_kib: baseline.rssKiB,
      peak_rss_kib: Math.max(...samples.map((sample) => sample.rssKiB)),
      peak_rss_delta_kib: Math.max(...samples.map((sample) => sample.rssKiB)) - baseline.rssKiB,
      samples: samples.length,
    };
  } catch (error) {
    throw new Error(`${error instanceof Error ? error.message : String(error)}\nzork-agent stderr:\n${stderr}`);
  } finally {
    await stopChild(child);
    await new Promise((resolve) => provider.close(resolve));
    await fs.rm(root, { recursive: true, force: true });
  }
}

function median(values) {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
}

function summarize(results, mode) {
  const selected = results.filter((result) => result.mode === mode);
  return {
    mode,
    runs: selected.length,
    median_wall_ms: median(selected.map((result) => result.wall_ms)),
    median_cpu_ms: median(selected.map((result) => result.cpu_ms)),
    median_peak_rss_kib: median(selected.map((result) => result.peak_rss_kib)),
    median_peak_rss_delta_kib: median(selected.map((result) => result.peak_rss_delta_kib)),
    response_bytes: selected[0].response_bytes,
  };
}

const options = parseArguments(process.argv.slice(2));
const results = [];
for (let sequence = 0; sequence < options.repetitions; sequence += 1) {
  const modes = sequence % 2 === 0 ? ["streaming", "non_streaming"] : ["non_streaming", "streaming"];
  for (const mode of modes) {
    const result = await runOnce({ ...options, mode, sequence: sequence + 1 });
    results.push(result);
    process.stderr.write(`${mode} ${sequence + 1}/${options.repetitions}: ${result.wall_ms} ms, ${result.peak_rss_kib} KiB peak RSS\n`);
  }
}
process.stdout.write(`${JSON.stringify({ options, results, summary: [summarize(results, "streaming"), summarize(results, "non_streaming")] }, null, 2)}\n`);
