import { cloneElement, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { ReactElement } from "react";
import { createPortal } from "react-dom";
import { Avatar, Glyph } from "./controls";
export function HoverDetails({
  title,
  kind,
  avatar,
  description,
  rows,
  children,
  compact = false,
}: {
  title: string;
  kind: string;
  avatar?: string;
  description?: string;
  rows: Array<[string, string]>;
  compact?: boolean;
  children: ReactElement<{ "aria-describedby"?: string }>;
}) {
  const id = useId(),
    trigger = useRef<HTMLSpanElement>(null),
    panel = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [open, setOpen] = useState(false),
    [position, setPosition] = useState({ left: 12, top: 12 });
  const cancel = () => {
    clearTimeout(timer.current);
  };
  const hide = () => {
    cancel();
    timer.current = setTimeout(() => setOpen(false), 80);
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  useEffect(() => {
    if (!open) return;
    const close = () => {
      clearTimeout(timer.current);
      setOpen(false);
    };
    const scroll = (e: Event) => {
      if (!panel.current?.contains(e.target as Node)) close();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("resize", close);
    document.addEventListener("scroll", scroll, true);
    document.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("resize", close);
      document.removeEventListener("scroll", scroll, true);
      document.removeEventListener("keydown", key, true);
    };
  }, [open]);
  useLayoutEffect(() => {
    if (!open || !panel.current) return;
    const anchor = trigger.current?.querySelector("button")?.getBoundingClientRect();
    if (!anchor) return;
    const rect = panel.current.getBoundingClientRect();
    const preferred =
      anchor.right + 4 + rect.width <= innerWidth - 12 ? anchor.right + 4 : anchor.left - rect.width - 4;
    if (compact) {
      setPosition({
        left: Math.max(12, Math.min(anchor.left + (anchor.width - rect.width) / 2, innerWidth - rect.width - 12)),
        top:
          anchor.bottom + 8 + rect.height <= innerHeight - 12
            ? anchor.bottom + 8
            : Math.max(12, anchor.top - rect.height - 8),
      });
      return;
    }
    setPosition({
      left: Math.max(12, Math.min(preferred, innerWidth - rect.width - 12)),
      top: Math.max(12, Math.min(anchor.top, innerHeight - rect.height - 12)),
    });
  }, [open, compact]);
  return (
    <span
      className={compact ? "ref-tooltip-anchor ref-hint-anchor" : "ref-tooltip-anchor"}
      ref={trigger}
      onPointerEnter={() => {
        cancel();
        setOpen(true);
      }}
      onPointerLeave={hide}
      onFocus={() => {
        cancel();
        setOpen(true);
      }}
      onBlur={hide}
    >
      {cloneElement(children, { "aria-describedby": open ? id : undefined })}
      {open &&
        createPortal(
          <div
            ref={panel}
            id={id}
            className={compact ? "ref-control-hint" : "ref-details-tooltip"}
            role="tooltip"
            style={position}
            onPointerEnter={cancel}
            onPointerLeave={hide}
          >
            {!compact && (
              <header>
                {avatar ? <Avatar name={avatar} size={32} /> : <Glyph name="checklist" size={24} />}
                <div>
                  <b>{title}</b>
                  <small>{kind}</small>
                </div>
              </header>
            )}
            {description && <p>{description}</p>}
            {!compact && (
              <dl>
                {rows
                  .filter(([, value]) => value.trim())
                  .map(([label, value]) => (
                    <div key={label}>
                      <dt>{label}</dt>
                      <dd>{value}</dd>
                    </div>
                  ))}
              </dl>
            )}
          </div>,
          document.body,
        )}
    </span>
  );
}

export function HoverHint({
  text,
  children,
}: {
  text: string;
  children: ReactElement<{ "aria-describedby"?: string }>;
}) {
  return (
    <HoverDetails title="" kind="" rows={[]} description={text} compact>
      {children}
    </HoverDetails>
  );
}
