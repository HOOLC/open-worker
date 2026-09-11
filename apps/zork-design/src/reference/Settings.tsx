import { HoverHint } from "./HoverDetails";
import { useRef, useState } from "react";
import { asset } from "../content/assets";
import type { Catalog, Story } from "../workbench/types";
import { Button, Field, Glyph, Help, Modal, Notice, Switch } from "./controls";
export function ClientPage({ catalog, story }: { catalog: Catalog; story: Story }) {
  const scene = story.state.replace(/-(compact|wide)$/, ""),
    [signedIn, setSignedIn] = useState(scene === "signed-in"),
    [busy, setBusy] = useState(scene === "loading"),
    [notice, setNotice] = useState(scene === "error" ? "登录未完成。请检查连接后重试。" : "");
  const loginGeneration = useRef(0);
  const login = () => {
    const generation = ++loginGeneration.current;
    setBusy(true);
    setNotice("");
    setTimeout(() => {
      if (generation === loginGeneration.current) {
        setBusy(false);
        setSignedIn(true);
      }
    }, 400);
  };
  return (
    <section className="ref-settings" data-reference-target>
      <header className="ref-page-heading">
        <h1>客户端设置</h1>
      </header>
      <p className="ref-note">账号与此客户端的连接身份。</p>
      <div className="ref-summary-row">
        <div>
          <b>账号</b>
          <small>
            {signedIn
              ? `${catalog.fixture.account.name} · ${catalog.fixture.account.email}`
              : "登录后，在你的设备间识别同一个账号。"}
          </small>
        </div>
        <div className="ref-actions">
          <Button disabled={busy} onClick={login}>
            {busy ? "等待登录…" : signedIn ? "重新验证" : "登录账号"}
          </Button>
          {busy && (
            <Button
              onClick={() => {
                loginGeneration.current++;
                setBusy(false);
              }}
            >
              取消
            </Button>
          )}
          {signedIn && (
            <Button disabled={busy} onClick={() => setSignedIn(false)}>
              退出账号
            </Button>
          )}
        </div>
      </div>
      <div className="ref-summary-row">
        <div>
          <b>客户端身份</b>
          <small>{catalog.fixture.account.identity}</small>
        </div>
        <Button
          onClick={() => {
            void navigator.clipboard.writeText(catalog.fixture.account.identity).then(
              () => setNotice("客户端身份已复制。"),
              () => setNotice("请选中身份后手动复制。"),
            );
          }}
        >
          复制身份
        </Button>
      </div>
      <div className="ref-summary-row">
        <div>
          <b>配置归属</b>
          <small>小伙伴和模型连接分别保存在所属设备中。</small>
        </div>
      </div>
      {notice && (
        <div style={{ marginTop: 16 }}>
          <Notice>{notice}</Notice>
        </div>
      )}
    </section>
  );
}
export function DevicePage({ catalog, story }: { catalog: Catalog; story: Story }) {
  const scene = story.state.replace(/-(compact|wide)$/, ""),
    [name, setName] = useState(catalog.fixture.device.name),
    [draft, setDraft] = useState(""),
    [renaming, setRenaming] = useState(false),
    [nameError, setNameError] = useState(""),
    [running, setRunning] = useState(scene !== "stopped"),
    [background, setBackground] = useState(true),
    [login, setLogin] = useState(false),
    [busy, setBusy] = useState(scene === "loading"),
    [version, setVersion] = useState("0.1.30"),
    [latest, setLatest] = useState(""),
    [notice, setNotice] = useState(scene === "error" ? "暂时无法读取设备状态，请重试。" : "");
  const changeMode = (value: boolean) => {
    setBackground(value);
    if (!value) setLogin(false);
  };
  return (
    <section className="ref-settings ref-device-settings" data-reference-target>
      <div className="ref-device-runtime">
        <span className="ref-device-icon">
          <span
            className="ref-device-node-glyph"
            aria-hidden="true"
            style={{ maskImage: `url("${asset("assets/native/current/icons/node.svg")}")` }}
          />
        </span>
        <div className="ref-device-identity">
          <div className="ref-device-name-line">
            <h1>{name}</h1>
            <HoverHint text="修改设备名称">
              <Button
                className="ref-device-edit"
                aria-label="修改设备名称"
                disabled={busy}
                onClick={() => {
                  setDraft(name);
                  setNameError("");
                  setRenaming(true);
                }}
              >
                <Glyph name="edit" size={14} />
              </Button>
            </HoverHint>
          </div>
          <div className="ref-device-metadata">
            <span className="ref-device-status" data-online={running}>
              <i />
              {running ? "在线" : "离线"}
            </span>
            <span>·</span>
            <span>{busy ? "正在处理…" : running ? "运行中" : "已停止"}</span>
            <span>·</span>
            <small className="ref-device-release">v{version}</small>
          </div>
        </div>
        <div className="ref-device-card-actions">
          <HoverHint text="刷新设备状态">
            <Button
              className="ref-device-refresh"
              aria-label="刷新设备状态"
              disabled={busy}
              onClick={() => {
                setBusy(false);
                setNotice("");
              }}
            >
              <Glyph name="reload" size={14} />
            </Button>
          </HoverHint>
          <Button primary={!running} disabled={busy} onClick={() => setRunning(!running)}>
            {running ? "停止设备" : "启动设备"}
          </Button>
        </div>
      </div>
      <section className="ref-device-mode">
        <h2>运行方式</h2>
        <div
          className="ref-radio-row"
          role="group"
          aria-label="运行方式"
          data-active={background ? "1" : "0"}
          onKeyDown={(e) => {
            if (busy) return;
            if (["ArrowLeft", "Home", "ArrowRight", "End"].includes(e.key)) {
              e.preventDefault();
              changeMode(e.key === "ArrowRight" || e.key === "End");
            }
          }}
        >
          <HoverHint text="关闭客户端时，设备一同停止。">
            <button type="button" aria-pressed={!background} disabled={busy} onClick={() => changeMode(false)}>
              随客户端
            </button>
          </HoverHint>
          <HoverHint text="退出客户端后，设备继续运行。">
            <button type="button" aria-pressed={background} disabled={busy} onClick={() => changeMode(true)}>
              后台运行
            </button>
          </HoverHint>
        </div>
        {background && (
          <div className="ref-device-login">
            <span>登录系统后自动启动</span>
            <Switch label="登录系统后自动启动" checked={login} onChange={setLogin} disabled={busy} />
          </div>
        )}
      </section>
      {background && (
        <div className="ref-device-version">
          {latest && latest !== version && <small className="ref-device-update">可升级至 {latest} · 将重启设备</small>}
          <div className="ref-actions">
            <Button disabled={busy} onClick={() => setLatest("0.1.31")}>
              检查更新
            </Button>
            {latest && latest !== version && (
              <Button
                primary
                disabled={busy}
                onClick={() => {
                  setVersion(latest);
                  setLatest("");
                  setNotice("版本升级完成");
                }}
              >
                升级并重启
              </Button>
            )}
          </div>
        </div>
      )}
      {renaming && (
        <Modal
          title="修改设备名称"
          onClose={() => setRenaming(false)}
          error={nameError}
          footer={
            <>
              <Button onClick={() => setRenaming(false)}>取消</Button>
              <Button
                primary
                onClick={() => {
                  const value = draft.trim();
                  if (
                    !value ||
                    [...value].length > 64 ||
                    [...value].some((char) => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127)
                  ) {
                    setNameError("请输入 1–64 个字符的设备名称");
                    return;
                  }
                  setName(value);
                  setRenaming(false);
                }}
              >
                保存
              </Button>
            </>
          }
        >
          <div className="ref-form">
            <Field label="名称" value={draft} onChange={setDraft} />
            <p className="ref-note">连接此设备的小伙伴都会看到新名称。</p>
          </div>
        </Modal>
      )}
      {notice && (
        <div style={{ marginTop: 12 }}>
          <Notice>{notice}</Notice>
        </div>
      )}
    </section>
  );
}
export function Enrollment({ catalog, initial = "" }: { catalog: Catalog; initial?: string }) {
  const [status, setStatus] = useState(initial === "command" ? "waiting" : initial === "expired" ? "expired" : ""),
    [busy, setBusy] = useState(initial === "loading"),
    [notice, setNotice] = useState(initial === "error" ? "获取加入命令失败，请重试。" : "");
  const active = status === "waiting" || status === "connecting";
  return (
    <div className="ref-form">
      <span className="ref-label">手动添加</span>
      <p className="ref-note">复制加入命令，在新设备的终端执行。连接后会出现在设备列表中。</p>
      {status && (
        <p style={{ fontSize: 12 }}>
          {active
            ? "等待目标设备执行 · 10 分 0 秒后过期"
            : status === "revoked"
              ? "加入命令已撤销。"
              : "加入命令已过期，请重新生成。"}
        </p>
      )}
      {active ? (
        <>
          <textarea className="ref-command" readOnly aria-label="加入命令" value={catalog.fixture.mesh.invitation} />
          <div className="ref-actions">
            <Button
              disabled={busy}
              onClick={() => {
                setStatus("revoked");
                setNotice("");
              }}
            >
              撤销命令
            </Button>
            <Button
              primary
              onClick={() => {
                void navigator.clipboard.writeText(catalog.fixture.mesh.invitation).then(
                  () => setNotice("加入命令已复制。"),
                  () => setNotice("请选中命令后手动复制。"),
                );
              }}
            >
              复制命令
            </Button>
          </div>
        </>
      ) : (
        <Button
          primary
          disabled={busy}
          onClick={() => {
            setBusy(false);
            setStatus("waiting");
            setNotice("");
          }}
        >
          {busy ? "正在获取…" : status ? "重新生成" : "生成加入命令"}
        </Button>
      )}
      {notice && <Notice>{notice}</Notice>}
    </div>
  );
}
export function MeshPage({ catalog, story }: { catalog: Catalog; story: Story }) {
  const scene = story.state.replace(/-(compact|wide)$/, ""),
    [peers, setPeers] = useState(scene === "empty" ? [] : catalog.fixture.mesh.peers),
    [open, setOpen] = useState(scene === "manual"),
    [enabled, setEnabled] = useState(true),
    [name, setName] = useState(""),
    [origin, setOrigin] = useState(""),
    [address, setAddress] = useState(""),
    [manage, setManage] = useState(false),
    [error, setError] = useState(""),
    [notice, setNotice] = useState("");
  return (
    <section className="ref-settings" data-reference-target={open ? undefined : ""}>
      <header className="ref-page-heading">
        <h1>设备连接</h1>
        <Button
          className="small"
          onClick={() => {
            setOpen(true);
            setError("");
          }}
        >
          <Glyph name="plus" size={13} />
          手动连接
        </Button>
      </header>
      <p className="ref-note">管理已配对设备及其访问权限。</p>
      <div className="ref-summary-row">
        <div>
          <b>允许设备连接</b>
          <small>{enabled ? "已启用" : "已关闭"}</small>
        </div>
        <Switch label="允许设备连接" checked={enabled} onChange={setEnabled} />
      </div>
      <div className="ref-inline" style={{ padding: "16px 0", gap: 8 }}>
        <Button
          onClick={() => {
            void navigator.clipboard.writeText(catalog.fixture.account.identity).then(
              () => setNotice("设备身份已复制。"),
              () => setNotice("请手动复制设备身份。"),
            );
          }}
        >
          复制设备身份
        </Button>
        <Button onClick={() => setNotice("连接信息已刷新。")}>刷新</Button>
      </div>
      <h2 className="ref-section-title">已配对设备</h2>
      {peers.length ? (
        peers.map((peer) => (
          <div className="ref-summary-row" key={peer.id}>
            <div>
              <b>{peer.name}</b>
              <small>
                {peer.online ? "已连接" : "暂时无法连接"} ·{" "}
                {peer.can_manage ? "客户端 · 可管理此设备" : "设备 · 按 队员 授权协作"}
              </small>
            </div>
            <Button onClick={() => setPeers((old) => old.filter((p) => p.id !== peer.id))}>移除</Button>
          </div>
        ))
      ) : (
        <p className="ref-note" style={{ padding: "20px 0", fontSize: 12 }}>
          还没有配对设备。通过加入命令或手动连接添加设备。
        </p>
      )}
      {notice && !open && (
        <div style={{ marginTop: 16 }}>
          <Notice>{notice}</Notice>
        </div>
      )}
      <div style={{ marginTop: 24, paddingTop: 20, borderTop: "1px solid var(--ref-border)" }}>
        <Enrollment catalog={catalog} />
      </div>
      {open && (
        <Modal
          title="手动连接设备"
          onClose={() => {
            setOpen(false);
            setError("");
          }}
          error={error}
          footer={
            <>
              <Button
                onClick={() => {
                  setOpen(false);
                  setError("");
                }}
              >
                取消
              </Button>
              <Button
                primary
                onClick={() => {
                  if (!name.trim()) {
                    setError("请填写设备名称");
                    return;
                  }
                  if (!origin.trim()) {
                    setError("请填写设备身份");
                    return;
                  }
                  setPeers((old) => [...old, { id: origin, name: name.trim(), online: true, can_manage: manage }]);
                  setOpen(false);
                  setName("");
                  setOrigin("");
                  setAddress("");
                  setManage(false);
                  setError("");
                }}
              >
                保存配对
              </Button>
            </>
          }
        >
          <div className="ref-form">
            <Field label="设备名称" value={name} onChange={setName} placeholder="设备名称" />
            <Field label="设备身份" value={origin} onChange={setOrigin} placeholder="key: 设备身份" />
            <Field label="局域网地址 · 可选" value={address} onChange={setAddress} placeholder="局域网地址（可选）" />
            <div className="ref-summary-row">
              <div>
                <b>允许作为客户端管理</b>
                <small>可管理此设备的模型连接、小伙伴和任务。</small>
              </div>
              <Switch label="客户端权限" checked={manage} onChange={setManage} />
            </div>
          </div>
        </Modal>
      )}
    </section>
  );
}
export function AddDevicePage({ catalog, story }: { catalog: Catalog; story: Story }) {
  const scene = story.state.replace(/-(compact|wide)$/, ""),
    [open, setOpen] = useState(true);
  if (!open)
    return (
      <section className="ref-settings" data-reference-target>
        <Button onClick={() => setOpen(true)}>添加设备</Button>
      </section>
    );
  return (
    <Modal title="添加设备" onClose={() => setOpen(false)}>
      <div className="ref-form">
        <p className="ref-note">在新设备上执行加入命令，连接后它会出现在设备列表中。</p>
        <span className="ref-label">交给领队</span>
        <div className="ref-inline" style={{ gap: 8 }}>
          {catalog.fixture.agents.map((agent) => (
            <button
              className="ref-leader-help"
              aria-label={"交给" + agent.name}
              key={agent.id}
              onClick={() => setOpen(false)}
            >
              <img src={asset(`assets/avatars/${agent.avatar}.svg`)} width={24} height={24} />
            </button>
          ))}
        </div>
        <div style={{ paddingTop: 16, borderTop: "1px solid var(--ref-border)" }}>
          <Enrollment catalog={catalog} initial={scene} />
        </div>
      </div>
    </Modal>
  );
}
