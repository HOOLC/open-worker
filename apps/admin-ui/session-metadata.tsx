import { classSafeValue } from "./session-formatters.js";

import React from "react";

export type SessionMetaItem = {
  readonly label: string;
  readonly value: string;
  readonly detail?: string | undefined;
  readonly title?: string | undefined;
  readonly tone?: string | undefined;
};

export function MetaLine({ label, value, detail, title, tone }: SessionMetaItem): React.JSX.Element {
  return (
    <div className={"meta-line " + classSafeValue(tone, "")}>
      <span>{label}</span>
      <strong title={title}>{value}</strong>
      {detail ? <em title={detail}>{detail}</em> : null}
    </div>
  );
}
