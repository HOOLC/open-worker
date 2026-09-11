import { useState } from "react";
import type { Catalog, Story } from "../workbench/types";
import { Avatar, Button, Empty, Field, Glyph, Help, Modal, Notice, ProviderIcon, Select } from "./controls";
export const protocolOptions = [
  { value: "openai-completions", label: "OpenAI Chat Completions", provider: "openai" },
  { value: "openai-responses", label: "OpenAI Responses", provider: "openai" },
  { value: "openai-codex-responses", label: "Codex Responses", provider: "openai" },
  { value: "anthropic-messages", label: "Anthropic Messages", provider: "anthropic" },
];
export function ProfilesPage({ catalog, story }: { catalog: Catalog; story: Story }) {
  const fixture = catalog.fixture,
    scene = story.state.replace(/-(compact|wide)$/, ""),
    profile = fixture.profile;
  const [modal, setModal] = useState(
    story.family === "connection" ? (scene === "list" ? "" : "connection") : scene === "detail" ? "detail" : "model",
  );
  const [models, setModels] = useState(profile.models),
    [connectionName, setConnectionName] = useState(profile.profile_id),
    [hasProfile, setHasProfile] = useState(true);
  const [provider, setProvider] = useState(profile.provider),
    [subscription, setSubscription] = useState(true),
    [name, setName] = useState(""),
    [base, setBase] = useState(""),
    [secret, setSecret] = useState("");
  const [modelId, setModelId] = useState(""),
    [api, setApi] = useState(fixture.model_form.api),
    [context, setContext] = useState(String(fixture.model_form.context_window)),
    [output, setOutput] = useState(String(fixture.model_form.max_output_tokens)),
    [thinking, setThinking] = useState(fixture.model_form.thinking),
    [defaultThinking, setDefaultThinking] = useState(fixture.model_form.default_thinking);
  const [editing, setEditing] = useState<string | null>(null),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [busy, setBusy] = useState(false),
    [attempt, setAttempt] = useState(false);
  const [modelAttempted, setModelAttempted] = useState(false);
  const modelErrors: Record<string, string> = {};
  const c = Number(context),
    o = Number(output);
  const levels = thinking
    .split(",")
    .map((v) => v.trim())
    .filter(Boolean);
  if (!modelId.trim()) modelErrors.id = "填写供应商提供的模型 ID。";
  else if (models.some((m) => m.id === modelId.trim() && m.id !== editing))
    modelErrors.id = "此 ID 已存在，请使用其他模型 ID。";
  if (!Number.isInteger(c) || c <= 0) modelErrors.context = "请输入大于 0 的整数。";
  if (!Number.isInteger(o) || o <= 0) modelErrors.output = "请输入大于 0 的整数。";
  else if (!modelErrors.context && o >= c) modelErrors.output = `需小于上下文上限（${c}）。`;
  if (!levels.includes(defaultThinking.trim()))
    modelErrors.defaultThinking = levels.length ? "填写左侧已有的推理级别。" : "请先在左侧填写可用推理级别。";
  const fieldError = (key: string) => (modelAttempted ? modelErrors[key] : undefined);
  const choices = catalog.providers
    .filter((p) => p.billing.some((b) => (b.id === "subscription") === subscription))
    .map((p) => ({ value: p.id, label: p.label, provider: p.id }));
  const providerName = catalog.providers.find((p) => p.id === provider)?.label ?? provider;
  const billing =
    catalog.providers.find((p) => p.id === provider)?.billing.find((b) => (b.id === "subscription") === subscription)
      ?.label ?? "";
  const close = () => {
    setModelAttempted(false);
    setError("");
    setAttempt(false);
    setBusy(false);
    setModal(modal === "model" ? "detail" : "");
  };
  const edit = (model?: Record<string, unknown>) => {
    setModelAttempted(false);
    setEditing(model ? String(model.id) : null);
    setModelId(model ? String(model.id) : "");
    setApi(model ? String(model.api ?? fixture.model_form.api) : fixture.model_form.api);
    setContext(String(model?.context_window ?? fixture.model_form.context_window));
    setOutput(String(model?.max_output_tokens ?? fixture.model_form.max_output_tokens));
    setThinking(Array.isArray(model?.thinking) ? model.thinking.join(", ") : fixture.model_form.thinking);
    setDefaultThinking(String(model?.default_thinking ?? fixture.model_form.default_thinking));
    setError("");
    setModal("model");
  };
  const saveModel = () => {
    setModelAttempted(true);
    if (Object.keys(modelErrors).length) {
      requestAnimationFrame(() =>
        document.querySelector<HTMLInputElement>('.ref-modal input[aria-invalid="true"]')?.focus(),
      );
      return;
    }
    const c = Number(context),
      o = Number(output),
      levels = thinking
        .split(",")
        .map((v) => v.trim())
        .filter(Boolean);
    const model = {
      id: modelId.trim(),
      api,
      context_window: c,
      max_output_tokens: o,
      limits: { context_window_tokens: c, max_output_tokens: o },
      thinking: levels,
      default_thinking: defaultThinking.trim(),
    };
    setModels((old) => (editing ? old.map((m) => (m.id === editing ? model : m)) : [...old, model]));
    setError("");
    setModal("detail");
  };
  const saveConnection = () => {
    if (!name.trim()) {
      setError("请输入连接名称。");
      return;
    }
    if (!subscription && !secret.trim()) {
      setError("请输入 API Key。");
      return;
    }
    if (!subscription && provider === "openai-compatible" && !/^https?:\/\//.test(base)) {
      setError("请输入有效的接口地址。");
      return;
    }
    setError("");
    if (subscription && !attempt) {
      setAttempt(true);
      return;
    }
    setConnectionName(name.trim());
    setHasProfile(true);
    setModal("detail");
  };
  return (
    <section className="ref-settings" data-reference-target={modal ? undefined : ""}>
      <header className="ref-page-heading">
        <div>
          <h1>大模型</h1>
          <small className="ref-list-subtitle">
            {fixture.device.name} · {hasProfile ? 1 : 0} 个连接
          </small>
        </div>
        <Button
          className="small"
          onClick={() => {
            setModal("connection");
            setError("");
          }}
        >
          <Glyph name="plus" size={13} />
          添加连接
        </Button>
      </header>
      <div className="ref-profile-list">
        {hasProfile ? (
          <button className="ref-setting-row" onClick={() => setModal("detail")}>
            <span className="ref-provider-tile">
              <ProviderIcon id={profile.provider} size={24} />
            </span>
            <div>
              <b>{connectionName}</b>
              <small>
                {catalog.providers.find((p) => p.id === profile.provider)?.label} ·{" "}
                {
                  catalog.providers
                    .find((p) => p.id === profile.provider)
                    ?.billing.find((b) => b.id === profile.billing)?.label
                }
              </small>
            </div>
            <span className="meta">{models.length} 个模型</span>
            <span className="ref-verified" data-verified="false">
              待验证
            </span>
            <Glyph name="arrow-right" size={14} />
          </button>
        ) : (
          <Empty title="还没有模型连接">先连接一个供应商账号，再为小伙伴 配置模型。</Empty>
        )}
      </div>
      <Help agents={fixture.agents} />
      {modal === "connection" && (
        <Modal
          title="添加模型连接"
          onClose={close}
          error={error}
          footer={
            <>
              <Button disabled={busy} onClick={close}>
                取消
              </Button>
              <Button primary disabled={busy} onClick={saveConnection}>
                {subscription ? (attempt ? "完成连接" : "登录并连接") : "保存连接"}
              </Button>
            </>
          }
        >
          <div className="ref-form">
            <p className="ref-note ref-inline">
              <Glyph name="node" size={14} />
              {fixture.device.name} · 连接保存在此设备
            </p>
            <div className="ref-field">
              <span>接入方式</span>
              <div className="ref-radio-row" data-active={subscription ? "0" : "1"}>
                {[
                  [true, "订阅账号"],
                  [false, "API 接入"],
                ].map(([value, label]) => (
                  <button
                    key={String(value)}
                    type="button"
                    aria-pressed={subscription === value}
                    onClick={() => {
                      const sub = Boolean(value);
                      setSubscription(sub);
                      const allowed = catalog.providers.filter((p) =>
                        p.billing.some((b) => (b.id === "subscription") === sub),
                      );
                      if (!allowed.some((p) => p.id === provider)) setProvider(allowed[0]?.id ?? "");
                      setAttempt(false);
                      setError("");
                    }}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
            <Select
              label="供应商"
              testId="provider"
              options={choices}
              value={provider}
              onChange={(value) => {
                setProvider(value);
                setSecret("");
                setBase("");
                setAttempt(false);
              }}
              initiallyOpen={scene === "provider"}
            />
            <Field
              label="连接名称"
              testId="connection-name"
              value={name}
              onChange={setName}
              placeholder={`例如：my-${provider}`}
            />
            {!subscription && provider === "openai-compatible" && (
              <Field label="接口地址" value={base} onChange={setBase} placeholder="https://api.example.com/v1" />
            )}
            {!subscription && (
              <Field label="API Key" secret value={secret} onChange={setSecret} placeholder="输入 API Key" />
            )}
            {attempt && (
              <Notice>
                请在浏览器完成 {providerName} 授权，再点击“完成连接”。
                <p className="ref-note">{billing} · 此处为交互设计示例。</p>
              </Notice>
            )}
          </div>
        </Modal>
      )}
      {modal === "detail" && (
        <Modal title={connectionName} onClose={close}>
          <p className="ref-note ref-inline" style={{ height: 18 }}>
            <ProviderIcon id={profile.provider} />
            {fixture.device.name} · {catalog.providers.find((p) => p.id === profile.provider)?.label} · 未验证
          </p>
          <div className="ref-detail-toolbar">
            <strong className="ref-section-title" style={{ margin: 0 }}>
              模型
            </strong>
            <div>
              <Button
                className="small"
                disabled={busy}
                onClick={() => {
                  setBusy(true);
                  setNotice("");
                  setTimeout(() => {
                    setBusy(false);
                    setNotice("已完成查询。未列出的模型可手动添加。");
                  }, 400);
                }}
              >
                {busy ? "正在获取…" : "获取模型"}
              </Button>
              <Button className="small" onClick={() => edit()}>
                <Glyph name="plus" size={13} />
                手动添加
              </Button>
            </div>
          </div>
          {notice && <Notice>{notice}</Notice>}
          {models.length ? (
            models.map((model) => (
              <div className="ref-model-row" key={String(model.id)}>
                <button type="button" onClick={() => edit(model)}>
                  <b>{String(model.id)}</b>
                  <small>
                    手动添加 · 未验证 · 上下文{" "}
                    {String(
                      model.context_window ??
                        (model.limits as Record<string, unknown>)?.context_window_tokens ??
                        "未设置",
                    )}{" "}
                    / 输出{" "}
                    {String(
                      model.max_output_tokens ??
                        (model.limits as Record<string, unknown>)?.max_output_tokens ??
                        "未设置",
                    )}
                  </small>
                </button>
                <Button
                  className="small remove"
                  aria-label={`移除 ${model.id}`}
                  onClick={() => setModels((old) => old.filter((m) => m !== model))}
                >
                  移除
                </Button>
              </div>
            ))
          ) : (
            <Empty title="此连接尚未添加模型" action={<Button onClick={() => edit()}>手动添加</Button>} />
          )}
          <p className="ref-detail-note">在 Agent 设置中选择此连接，再选择其中的模型。</p>
        </Modal>
      )}
      {modal === "model" && (
        <Modal
          title={editing ? "编辑模型" : "添加模型"}
          onClose={close}
          footer={
            <>
              <Button onClick={close}>取消</Button>
              <Button primary onClick={saveModel}>
                保存模型
              </Button>
            </>
          }
        >
          <div className="ref-form">
            <Field
              label="模型 ID"
              testId="model-id"
              error={fieldError("id")}
              autoFocus={scene === "create"}
              value={modelId}
              onChange={setModelId}
              placeholder="供应商提供的模型标识"
            />
            <Select
              label="接口协议"
              testId="protocol"
              options={protocolOptions}
              value={api}
              onChange={setApi}
              initiallyOpen={scene === "protocol"}
            />
            <div className="ref-columns">
              <Field
                error={fieldError("context")}
                label="上下文 token 上限"
                testId="context-window"
                value={context}
                onChange={setContext}
              />
              <Field
                error={fieldError("output")}
                label="输出 token 上限"
                testId="output-limit"
                value={output}
                onChange={setOutput}
              />
            </div>
            <div className="ref-columns">
              <Field label="推理级别，用逗号分隔" value={thinking} onChange={setThinking} />
              <Field
                error={fieldError("defaultThinking")}
                label="默认推理级别"
                value={defaultThinking}
                onChange={setDefaultThinking}
              />
            </div>
            <p className="ref-note">填写该模型实际支持的容量和推理级别。</p>
          </div>
        </Modal>
      )}
    </section>
  );
}
