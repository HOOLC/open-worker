import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import type { PersistedAgentTraceEvent, PersistedInboundMessage, SlackSessionRecord } from "../src/types.js";

import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";

import { MockSlackServer } from "./manual/mock-slack-server.js";

import {
  brokerRoot,
  DEFAULT_E2E_TIMEOUT_MS,
  DAY_MS,
  startBrokerProcess,
  waitForHttpReady,
  waitFor,
  isTransientSqliteLock,
  waitForSessionIdle,
  waitForSessionActive,
  readSessionRecord,
  readInboundMessages,
  readAgentTraceEvents,
  delay,
  pathExists,
  removeTempRoot,
  getFreePort,
  collectTextInput,
  findStartedTurnTextContaining,
  postJson,
  createDeferred,
} from "./e2e-broker-helpers.js";

describe.sequential("slack-codex-broker e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      const cleanup = cleanups.pop();
      await cleanup?.();
    }
  });

  it("delivers idle input and active follow-up input through one broker agent input contract", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const releaseTurn = createDeferred<void>();
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        await releaseTurn.promise;
        context.complete("CONTRACT_DONE");
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      releaseTurn.resolve();
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const sessionKey = "C123:445.220";
    const broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
      extraEnv: {
        SLACK_ACTIVE_TURN_RECONCILE_INTERVAL_MS: "100",
      },
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-contract-initial", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "445.220",
      ts: "445.221",
      text: "<@UBOT> INITIAL_CONTRACT_INPUT",
    });

    await waitFor(() => mockCodex.turnsStarted.length === 1, "initial input starts one turn");
    await waitForSessionActive(tempRoot, sessionKey);

    await mockSlack.sendEvent("evt-contract-follow-up", {
      type: "message",
      user: "U234",
      channel: "C123",
      thread_ts: "445.220",
      ts: "445.222",
      text: "FOLLOW_UP_ACTIVE_INPUT",
    });

    await waitFor(() => mockCodex.steers.some((steer) => collectTextInput(steer.input).includes("FOLLOW_UP_ACTIVE_INPUT")), "active follow-up delivered immediately");

    expect(mockCodex.turnsStarted).toHaveLength(1);
    expect(mockCodex.interrupts).toHaveLength(0);
    const inflightBeforeCompletion = await readInboundMessages(tempRoot, sessionKey);
    expect(inflightBeforeCompletion.find((message) => message.messageTs === "445.222")?.status).toBe("inflight");

    releaseTurn.resolve();
    await waitForSessionIdle(tempRoot, sessionKey);

    const traceEvents = await readAgentTraceEvents(tempRoot, sessionKey);
    const deliveredEvents = traceEvents.filter((event) => event.type === "agent_input_delivered");
    expect(deliveredEvents).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          status: "started_turn",
          metadata: expect.objectContaining({
            delivery: "started_turn",
          }),
        }),
        expect.objectContaining({
          status: "joined_active_turn",
          metadata: expect.objectContaining({
            delivery: "joined_active_turn",
          }),
        }),
      ]),
    );
    expect(deliveredEvents.filter((event) => event.status === "joined_active_turn")).toHaveLength(1);
    expect(traceEvents.map((event) => event.type)).toEqual(expect.arrayContaining(["agent_input_received", "agent_input_delivered", "agent_turn_started", "agent_turn_completed"]));
  }, 90_000);

  it("queues active Slack follow-up input when immediate active delivery fails", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const releaseInitialTurn = createDeferred<void>();
    const releaseFollowUpTurn = createDeferred<void>();
    let steerFailureInjected = false;
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    const mockCodex = new MockCodexAppServer({
      onTurnSteerRequest: (request) => {
        if (!steerFailureInjected && collectTextInput(request.input).includes("FOLLOW_UP_QUEUED_AFTER_ACTIVE_DELIVERY_FAILURE")) {
          steerFailureInjected = true;
          return "temporary active input delivery failure";
        }
        return undefined;
      },
      onTurnStart: async (context) => {
        if (mockCodex.turnsStarted.length === 1) {
          await releaseInitialTurn.promise;
          context.complete("INITIAL_DONE");
          return;
        }

        await releaseFollowUpTurn.promise;
        context.complete("FOLLOW_UP_DONE");
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      releaseInitialTurn.resolve();
      releaseFollowUpTurn.resolve();
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const sessionKey = "C123:446.220";
    const broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-active-delivery-fallback-initial", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "446.220",
      ts: "446.221",
      text: "<@UBOT> INITIAL_ACTIVE_DELIVERY_FAILURE_TEST",
    });

    await waitFor(() => mockCodex.turnsStarted.length === 1, "initial turn before active delivery failure");
    await waitForSessionActive(tempRoot, sessionKey);

    await mockSlack.sendEvent("evt-active-delivery-fallback-follow-up", {
      type: "message",
      user: "U234",
      channel: "C123",
      thread_ts: "446.220",
      ts: "446.222",
      text: "FOLLOW_UP_QUEUED_AFTER_ACTIVE_DELIVERY_FAILURE",
    });

    await waitFor(() => steerFailureInjected, "active delivery failure injected");
    expect(mockCodex.steers).toHaveLength(0);

    releaseInitialTurn.resolve();
    await waitFor(() => mockCodex.turnsStarted.some((turn) => collectTextInput(turn.input).includes("FOLLOW_UP_QUEUED_AFTER_ACTIVE_DELIVERY_FAILURE")), "follow-up starts as queued turn after active delivery failure");

    const followUpTurn = mockCodex.turnsStarted.find((turn) => collectTextInput(turn.input).includes("FOLLOW_UP_QUEUED_AFTER_ACTIVE_DELIVERY_FAILURE"));
    expect(followUpTurn).toBeTruthy();
    expect(mockSlack.postedMessages.map((message) => message.text)).not.toContain("I hit an internal issue while working on this thread. Send a quick follow-up and I will continue from the latest state.");
  }, 90_000);

  it("wakes a turn that ends without an explicit final, block, or wait state", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    let turnCount = 0;
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        turnCount += 1;
        context.complete("");
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "666.220",
      ts: "666.221",
      text: "<@UBOT> 继续把这个做完",
    });

    let wakeText = "";
    await waitFor(
      () => {
        wakeText = findStartedTurnTextContaining(mockCodex, "explicit final, block, or wait state") ?? "";
        return Boolean(wakeText);
      },
      "unexpected stop wake turn",
      120_000,
    );
    expect(wakeText).toContain("The previous run for this Slack thread appears to have stopped unexpectedly.");
    expect(wakeText).toContain("unexpected_turn_stop_json");
    expect(wakeText).toContain('"source": "unexpected_turn_stop"');
    expect(wakeText).toContain('"turn_id":');
    expect(wakeText).toContain("explicit final, block, or wait state");
    expect(wakeText).toContain("kind=block");
    expect(wakeText).toContain("kind=wait");
    expect(wakeText).toContain("zork-call chat post-state");
    expect(wakeText).toContain("silent block state");
    expect(wakeText).toContain("silent final state");
    expect(wakeText).toContain("Do not send a normal Slack reply and then a second '[block]' or '[wait]' line");
    await waitForSessionIdle(tempRoot, "C123:666.220");
    expect(turnCount).toBeGreaterThanOrEqual(2);
  }, 150_000);

  it("wakes a wait turn when no running async job backs that wait state", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
    const brokerBaseUrl = `http://127.0.0.1:${brokerPort}`;
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    let turnCount = 0;
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        turnCount += 1;
        if (turnCount === 1) {
          await waitForSessionActive(tempRoot, "C123:777.220");
          await postJson(`${brokerBaseUrl}/slack/post-state`, {
            channel_id: "C123",
            thread_ts: "777.220",
            kind: "wait",
            reason: "waiting for async job",
          });
          context.complete("");
        }
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const broker = await startBrokerProcess({
      port: brokerPort,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "777.220",
      ts: "777.221",
      text: "<@UBOT> 盯一下这个",
    });

    let wakeText = "";
    await waitFor(() => {
      wakeText = findStartedTurnTextContaining(mockCodex, "there is no running broker-managed async job") ?? "";
      return Boolean(wakeText);
    }, "wait-without-job wake turn");
    expect(wakeText).toContain("unexpected_turn_stop_json");
    expect(wakeText).toContain("there is no running broker-managed async job");
    await waitForSessionIdle(tempRoot, "C123:777.220");
    expect(turnCount).toBeGreaterThanOrEqual(2);
  }, 60_000);

  it("does not wake a silent wait turn when a running async job backs that wait state", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
    const brokerBaseUrl = `http://127.0.0.1:${brokerPort}`;
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    let turnCount = 0;
    const firstTurnCompleted = createDeferred<void>();
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        turnCount += 1;
        if (turnCount === 1) {
          try {
            await waitForSessionActive(tempRoot, "C123:778.220");
            const registerResponse = await fetch(`${brokerBaseUrl}/jobs/register`, {
              method: "POST",
              headers: {
                "content-type": "application/json",
              },
              body: JSON.stringify({
                channel_id: "C123",
                thread_ts: "778.220",
                kind: "watch_ci",
                script: "#!/bin/sh\nsleep 30",
              }),
            });
            expect(registerResponse.ok).toBe(true);

            await postJson(`${brokerBaseUrl}/slack/post-state`, {
              channel_id: "C123",
              thread_ts: "778.220",
              kind: "wait",
              reason: "waiting for async job",
            });
            context.complete("");
            firstTurnCompleted.resolve();
          } catch (error) {
            firstTurnCompleted.reject(error);
            throw error;
          }
        }
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const broker = await startBrokerProcess({
      port: brokerPort,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "778.220",
      ts: "778.221",
      text: "<@UBOT> 盯一下这个",
    });

    await firstTurnCompleted.promise;
    await waitForSessionIdle(tempRoot, "C123:778.220", 60_000);
    const postedMessageCountAfterIdle = mockSlack.postedMessages.length;
    await delay(1_000);
    expect(turnCount).toBe(1);
    expect(mockSlack.postedMessages).toHaveLength(postedMessageCountAfterIdle);
  }, 90_000);

  it("does not wake a silent block turn that already recorded its blocker", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
    const brokerBaseUrl = `http://127.0.0.1:${brokerPort}`;
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    let turnCount = 0;
    const firstTurnCompleted = createDeferred<void>();
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        turnCount += 1;
        if (turnCount === 1) {
          try {
            await waitForSessionActive(tempRoot, "C123:779.220");
            await postJson(`${brokerBaseUrl}/slack/post-state`, {
              channel_id: "C123",
              thread_ts: "779.220",
              kind: "block",
              reason: "waiting for user approval",
            });
            context.complete("");
            firstTurnCompleted.resolve();
          } catch (error) {
            firstTurnCompleted.reject(error);
            throw error;
          }
        }
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const broker = await startBrokerProcess({
      port: brokerPort,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "779.220",
      ts: "779.221",
      text: "<@UBOT> 这步先停住",
    });

    await firstTurnCompleted.promise;
    await waitForSessionIdle(tempRoot, "C123:779.220", 60_000);
    const postedMessageCountAfterIdle = mockSlack.postedMessages.length;
    await delay(1_000);
    expect(turnCount).toBe(1);
    expect(mockSlack.postedMessages).toHaveLength(postedMessageCountAfterIdle);
  }, 90_000);
});
