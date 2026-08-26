import { statusTone } from "./admin-formatters.js";

import { Tone } from "./admin-types.js";

import { statusLabel } from "./timeline-display";

import React from "react";

export function Badge({ label, tone = "" }: { readonly label: string; readonly tone?: Tone }): React.JSX.Element {
  return <span className={"badge " + (tone || statusTone(label))}>{statusLabel(label)}</span>;
}
