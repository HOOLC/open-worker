import { statusTone } from "./session-formatters.js";

import { statusLabel } from "./timeline-display";

import React from "react";

export function Badge({ label, tone, title }: { readonly label: unknown; readonly tone?: string; readonly title?: string }): React.JSX.Element {
  return (
    <span className={"badge " + (tone || statusTone(label))} title={title}>
      {statusLabel(label)}
    </span>
  );
}
