import { mountModalScrim } from "./modalScrim";
import { createPortal } from "react-dom";
import { useEffect, useLayoutEffect, useId, useRef, useState } from "react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { asset } from "../content/assets";
export const providerAsset = (id = "") =>
  ({
    openai: "openai",
    anthropic: "anthropic",
    "github-copilot": "githubcopilot",
    kimi: "kimi",
    "kimi-coding": "kimi",
    openrouter: "openrouter",
    "opencode-go": "opencode",
    xai: "xai",
  })[id] ?? "compatible";
export function ProviderIcon({ id, size = 18 }: { id: string; size?: number }) {
  return (
    <img
      className="ref-provider"
      width={size}
      height={size}
      src={asset(`assets/providers/${providerAsset(id)}.svg`)}
      alt=""
    />
  );
}
export function Glyph({ name, size = 16 }: { name: string; size?: number }) {
  return (
    <img
      className="ref-glyph"
      width={size}
      height={size}
      src={asset(`assets/native/current/icons/${name}.svg`)}
      alt=""
    />
  );
}
export function Avatar({ name = "cat", size = 28 }: { name?: string; size?: number }) {
  return <img className="ref-avatar" width={size} height={size} src={asset(`assets/avatars/${name}.svg`)} alt="" />;
}
export function Button({
  primary = false,
  children,
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { primary?: boolean }) {
  return (
    <button type="button" {...props} className={`ref-button ${primary ? "primary" : ""} ${className}`}>
      {children}
    </button>
  );
}
export function Field({
  label,
  value,
  onChange,
  placeholder,
  secret = false,
  disabled = false,
  error,
  testId,
  autoFocus = false,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  secret?: boolean;
  disabled?: boolean;
  error?: string;
  testId?: string;
  autoFocus?: boolean;
}) {
  const id = useId();
  return (
    <label className="ref-field" htmlFor={id}>
      <span id={id + "-label"}>{label}</span>
      <input
        id={id}
        aria-labelledby={id + "-label"}
        data-field={testId}
        data-autofocus={autoFocus ? "" : undefined}
        type={secret ? "password" : "text"}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        aria-invalid={Boolean(error)}
        aria-describedby={error ? id + "-error" : undefined}
      />
      {error && (
        <small id={id + "-error"} role="alert">
          {error}
        </small>
      )}
    </label>
  );
}
export interface Option {
  value: string;
  label: string;
  provider?: string;
  description?: string;
  disabled?: boolean;
}
export function Select({
  label,
  options,
  value,
  onChange,
  placeholder = "请选择",
  disabled = false,
  initiallyOpen = false,
  testId,
}: {
  label?: string;
  options: Option[];
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  initiallyOpen?: boolean;
  testId?: string;
}) {
  const [open, setOpen] = useState(initiallyOpen),
    [highlight, setHighlight] = useState(
      Math.max(
        0,
        options.findIndex((o) => o.value === value),
      ),
    );
  const root = useRef<HTMLDivElement>(null),
    trigger = useRef<HTMLButtonElement>(null),
    id = useId();
  const menu = useRef<HTMLDivElement>(null);
  const [placement, setPlacement] = useState({ left: 0, top: 0, width: 0, maxHeight: 218 });
  useLayoutEffect(() => {
    if (!open) return;
    const measure = () => {
      const rect = trigger.current?.getBoundingClientRect();
      if (!rect) return;
      const styleExample = document.documentElement.dataset.designStyle !== "legacy";
      const gap = styleExample ? 6 : 2;
      const desiredHeight = Math.min(218, options.length * 34 + (styleExample ? 16 : 8));
      const below = innerHeight - rect.bottom - 8;
      const top =
        below >= desiredHeight || rect.top < desiredHeight ? rect.bottom + gap : rect.top - desiredHeight - gap;
      setPlacement({
        left: rect.left - (styleExample ? 4 : 0),
        top,
        width: rect.width + (styleExample ? 8 : 0),
        maxHeight: Math.min(218, innerHeight - top - 8),
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    if (trigger.current) observer.observe(trigger.current);
    const dialog = root.current?.closest("dialog");
    if (dialog) observer.observe(dialog);
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  }, [open, options.length]);
  const selected = options.find((o) => o.value === value);
  useEffect(() => {
    if (!open) return;
    const close = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node) && !menu.current?.contains(e.target as Node)) setOpen(false);
    };
    const keyboard = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setOpen(false);
        trigger.current?.focus();
      } else if (e.key === "Tab") setOpen(false);
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", keyboard, true);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("keydown", keyboard, true);
    };
  }, [open]);
  const choose = (option: Option) => {
    if (option.disabled) return;
    onChange(option.value);
    setOpen(false);
    trigger.current?.focus();
  };
  return (
    <div className="ref-select-field" ref={root}>
      {label && <span id={id + "-label"}>{label}</span>}
      <div className="ref-select">
        <button
          ref={trigger}
          data-select={testId}
          className="ref-select-trigger"
          type="button"
          disabled={disabled}
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-controls={id}
          aria-label={label ?? placeholder}
          aria-activedescendant={open ? id + "-" + highlight : undefined}
          onClick={() => setOpen(!open)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && open) {
              e.preventDefault();
              setOpen(false);
              e.stopPropagation();
              return;
            }
            if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
              e.preventDefault();
              setOpen(true);
              setHighlight((h) =>
                e.key === "Home"
                  ? 0
                  : e.key === "End"
                    ? options.length - 1
                    : (h + (e.key === "ArrowDown" ? 1 : -1) + options.length) % options.length,
              );
            }
            if (open && ["Enter", " "].includes(e.key)) {
              e.preventDefault();
              if (options[highlight]) choose(options[highlight]);
            }
          }}
        >
          {selected?.provider && <ProviderIcon id={selected.provider} />}
          <span>{selected?.label ?? placeholder}</span>
          <Glyph name="chevron-down" size={12} />
        </button>
        {open &&
          !disabled &&
          placement.width > 0 &&
          createPortal(
            <div
              ref={menu}
              style={{
                position: "fixed",
                left: placement.left,
                top: placement.top,
                width: placement.width,
                maxHeight: placement.maxHeight,
                right: "auto",
              }}
              className="ref-menu"
              data-menu={testId}
              role="listbox"
              id={id}
              aria-labelledby={label ? id + "-label" : undefined}
            >
              {options.length ? (
                options.map((option, index) => (
                  <button
                    type="button"
                    role="option"
                    tabIndex={-1}
                    id={id + "-" + index}
                    key={option.value}
                    aria-selected={option.value === value}
                    disabled={option.disabled}
                    className={(option.value === value ? "selected " : "") + (highlight === index ? "highlighted" : "")}
                    onPointerMove={() => setHighlight(index)}
                    onClick={() => choose(option)}
                  >
                    {option.provider && <ProviderIcon id={option.provider} />}
                    <span>
                      {option.label}
                      {option.description && <small>{option.description}</small>}
                    </span>
                    {option.value === value && <Glyph name="check" size={12} />}
                  </button>
                ))
              ) : (
                <p className="ref-note">暂无可选项</p>
              )}
            </div>,
            root.current?.closest("dialog") ?? document.body,
          )}
      </div>
    </div>
  );
}
export function Switch({
  label,
  checked,
  onChange,
  disabled = false,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="ref-switch"
      aria-label={label}
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span>
        <i />
      </span>
    </button>
  );
}
export function Modal({
  title,
  children,
  footer,
  onClose,
  error,
}: {
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  onClose: () => void;
  error?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    const releaseScrim = mountModalScrim();
    (dialog?.querySelector<HTMLElement>("[data-autofocus]") ?? dialog)?.focus();
    return () => {
      dialog?.close();
      releaseScrim();
    };
  }, []);
  return (
    <dialog
      ref={ref}
      tabIndex={-1}
      className="ref-modal"
      data-reference-target
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        if (e.target === ref.current) {
          const r = ref.current.getBoundingClientRect();
          if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) onClose();
        }
      }}
    >
      <header>
        <h2>{title}</h2>
        <button className="ref-close" type="button" aria-label="关闭" onClick={onClose}>
          <Glyph name="x" />
        </button>
      </header>
      {error && (
        <div className="ref-modal-notice" role="alert">
          <div className="ref-notice">{error}</div>
        </div>
      )}
      <div className="ref-modal-body">{children}</div>
      {footer && <footer>{footer && <div className="ref-actions">{footer}</div>}</footer>}
    </dialog>
  );
}
export function Notice({ children, error = false }: { children: ReactNode; error?: boolean }) {
  return (
    <div className={"ref-notice " + (error ? "error" : "")} role={error ? "alert" : "status"}>
      {children}
    </div>
  );
}
export function Empty({ title, children, action }: { title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="ref-empty">
      <Glyph name="inbox" size={28} />
      <h3>{title}</h3>
      {children && <p>{children}</p>}
      {action}
    </div>
  );
}
export function Help({ agents }: { agents: Array<{ name: string; avatar: string }> }) {
  return (
    <div className="ref-help">
      <span>交给领队</span>
      {agents.map((a) => (
        <button key={a.name} title={a.name} aria-label={"交给" + a.name} type="button">
          <Avatar name={a.avatar} size={24} />
        </button>
      ))}
    </div>
  );
}
