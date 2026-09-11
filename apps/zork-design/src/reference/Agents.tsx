import { useState } from "react";
import type { Catalog, Story } from "../workbench/types";
import { avatars } from "../content/assets";
import { Avatar, Button, Empty, Field, Glyph, Help, Modal, Notice, Select } from "./controls";
export function AvatarPicker({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  return (
    <div className="ref-field">
      <span>头像</span>
      <div className="ref-avatar-picker" role="group" aria-label="选择头像">
        {avatars.map((name) => (
          <button
            key={name}
            type="button"
            aria-label={name}
            aria-pressed={value === name}
            onClick={() => onChange(name)}
          >
            <Avatar name={name} size={24} />
          </button>
        ))}
      </div>
    </div>
  );
}
export function AgentsPage({ catalog, story }: { catalog: Catalog; story: Story }) {
  const fixture = catalog.fixture,
    scene = story.state.replace(/-(compact|wide)$/, ""),
    source = fixture.agents[0];
  const [agents, setAgents] = useState(fixture.agents),
    [modal, setModal] = useState(scene === "list" ? "" : scene === "create" ? "create" : "edit"),
    [name, setName] = useState(""),
    [role, setRole] = useState("leader"),
    [avatar, setAvatar] = useState(scene === "create" ? "cat" : source.avatar),
    [instructions, setInstructions] = useState("");
  const [profile, setProfile] = useState(fixture.profile.profile_id),
    [model, setModel] = useState(source.model),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [grants, setGrants] = useState<string[]>([]);
  const profileOptions = [
    { value: fixture.profile.profile_id, label: fixture.profile.profile_id, provider: fixture.profile.provider },
  ];
  const modelOptions = profile
    ? fixture.profile.models.map((m) => ({
        value: String(m.id),
        label: String(m.id),
        provider: fixture.profile.provider,
      }))
    : [];
  const close = () => {
    setModal("");
    setError("");
    setNotice("");
  };
  const save = () => {
    if (!name.trim()) {
      setError("请输入 小伙伴 名称。");
      return;
    }
    if (!profile || !model) {
      setError("请选择模型连接和模型。");
      return;
    }
    setAgents((old) => [
      ...old,
      {
        ...source,
        id: "created",
        prototype_id: "created",
        name: name.trim(),
        role,
        avatar,
        profile_id: profile,
        model,
      },
    ]);
    close();
  };
  return (
    <section className="ref-settings" data-reference-target={modal ? undefined : ""}>
      <header className="ref-page-heading">
        <div>
          <h1>队员</h1>
          <small className="ref-list-subtitle">
            {fixture.device.name} · {agents.length} 位小伙伴
          </small>
        </div>
        <Button
          className="small"
          onClick={() => {
            setName("");
            setRole("leader");
            setAvatar("cat");
            setModal("create");
          }}
        >
          <Glyph name="plus" size={13} />
          添加小伙伴
        </Button>
      </header>
      {agents.length ? (
        ["leader", "worker"].map((group) => {
          const rows = agents.filter((a) => a.role === group);
          return rows.length ? (
            <div key={group}>
              <h2 className="ref-role-label">
                {group === "leader" ? "领队" : "队员"} <small>{rows.length}</small>
              </h2>
              <p className="ref-role-description">
                {group === "leader" ? "与你沟通，安排任务与队员" : "接受领队安排，专注完成任务"}
              </p>
              {rows.map((a) => (
                <button
                  className="ref-agent-row"
                  key={a.id}
                  onClick={() => {
                    setAvatar(a.avatar);
                    setProfile(a.profile_id);
                    setModel(a.model);
                    setModal("edit");
                  }}
                >
                  <Avatar name={a.avatar} size={36} />
                  <div className="ref-agent-identity">
                    <b>{a.name}</b>
                    <small>
                      {a.model} · {a.profile_id}
                    </small>
                  </div>
                  <Glyph name="arrow-right" size={14} />
                </button>
              ))}
            </div>
          ) : null;
        })
      ) : (
        <Empty title="这台设备还没有 小伙伴" />
      )}
      <Help agents={agents.filter((a) => a.role === "leader")} />
      {modal === "create" && (
        <Modal
          title="添加 小伙伴"
          onClose={close}
          error={error}
          footer={
            <>
              <Button onClick={close}>取消</Button>
              <Button primary onClick={save}>
                创建小伙伴
              </Button>
            </>
          }
        >
          <div className="ref-form">
            <div className="ref-role-options" role="group" aria-label="小伙伴 角色">
              {["leader", "worker"].map((value) => (
                <button key={value} aria-pressed={role === value} onClick={() => setRole(value)} type="button">
                  <span className="ref-inline">
                    <Glyph name={value === "leader" ? "sparkles" : "checklist"} size={16} />
                    {value === "leader" ? "领队" : "队员"}
                    {role === value && (
                      <span style={{ marginLeft: "auto" }}>
                        <Glyph name="check" size={14} />
                      </span>
                    )}
                  </span>
                  <small>{value === "leader" ? "长期对话，协调任务" : "接收任务，独立执行"}</small>
                </button>
              ))}
            </div>
            <Field label="名称" value={name} onChange={setName} placeholder="小伙伴 名称" />
            <AvatarPicker value={avatar} onChange={setAvatar} />
            <Field
              label="职责与偏好 · 可选"
              value={instructions}
              onChange={setInstructions}
              placeholder="职责和偏好（可选）"
            />
            <div className="ref-inline" style={{ justifyContent: "space-between" }}>
              <span className="ref-label">模型连接</span>
              <Button className="small" onClick={() => setNotice("模型连接已刷新。")}>
                刷新
              </Button>
            </div>
            <Select
              testId="agent-profile"
              options={profileOptions}
              value={profile}
              onChange={(value) => {
                setProfile(value);
                setModel("");
              }}
              placeholder="选择模型连接"
            />
            <Select
              label="模型"
              testId="agent-model"
              options={modelOptions}
              value={model}
              onChange={setModel}
              placeholder={profile ? "选择模型" : "先选择模型连接"}
              disabled={!profile}
            />
            {role === "worker" && (
              <div className="ref-field">
                <span>单独授权领队</span>
                <p className="ref-note">同一 mesh 内的 领队默认可以指派任务。此列表用于其余单独授权。</p>
                {fixture.agents
                  .filter((a) => a.role === "leader")
                  .map((a) => (
                    <label key={a.id} className="ref-inline">
                      <input
                        type="checkbox"
                        checked={grants.includes(a.id)}
                        onChange={(e) =>
                          setGrants((old) => (e.target.checked ? [...old, a.id] : old.filter((id) => id !== a.id)))
                        }
                      />
                      <Avatar name={a.avatar} size={20} />
                      {a.name}
                    </label>
                  ))}
              </div>
            )}
            {notice && <Notice>{notice}</Notice>}
          </div>
        </Modal>
      )}
      {modal === "edit" && (
        <Modal
          title="编辑 小伙伴"
          onClose={close}
          error={error}
          footer={
            <>
              <Button onClick={close}>取消</Button>
              <Button
                primary
                onClick={() => {
                  setAgents((old) =>
                    old.map((a) => (a.id === source.id ? { ...a, avatar, profile_id: profile, model } : a)),
                  );
                  close();
                }}
              >
                保存修改
              </Button>
            </>
          }
        >
          <div className="ref-form">
            <span className="ref-label">{source.name}</span>
            <AvatarPicker value={avatar} onChange={setAvatar} />
            <Select
              label="模型连接"
              testId="agent-profile"
              options={profileOptions}
              value={profile}
              onChange={(value) => {
                setProfile(value);
                setModel("");
              }}
              initiallyOpen={scene === "dropdown"}
            />
            <Select
              label="模型"
              testId="agent-model"
              options={modelOptions}
              value={model}
              onChange={setModel}
              placeholder="选择模型"
            />
            {notice && <Notice>{notice}</Notice>}
          </div>
        </Modal>
      )}
    </section>
  );
}
