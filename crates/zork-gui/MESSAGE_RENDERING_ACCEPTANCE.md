# zork-gui message rendering acceptance

> Historical rendering evidence (superseded 2026-08-27): Markdown, Unicode,
> pagination, and visual findings remain useful, but the direct Agent message
> projection documented below has been removed. The current message semantics
> are fixed by `GATEWAY_IM_ENTRY_ACCEPTANCE.md`: only gateway-delivered user and
> assistant messages are transcript rows; tools, waits, and deltas are activity
> or internal Agent data.

## User request

> 消息渲染啥的也测测呗

This pass tests and completes the selected-task transcript against the real
`zork-agent` public HTTP/SSE projection and the already accepted Codex-style
shell/composer geometry.

## Baseline evidence — 2026-08-27

- `design-audit/67-message-audit-home-entry.jpeg`: task-flow entry state.
- `design-audit/68-message-audit-history-tool-wait.jpeg`: current user,
  assistant, tool-result, wait, pagination, and composer treatments.
- `design-audit/69-message-audit-unicode-long-input.jpeg`: multiline Chinese,
  English, emoji, decomposed accents, paths, URL-like text, and soft wrapping.
- `design-audit/70-message-audit-optimistic-thinking.jpeg`: the sent user
  message appears once while the agent is thinking.
- `design-audit/71-message-audit-stream-tool-complete.jpeg`: assistant,
  tool-result, completion, and second-wait sequence.
- `design-audit/72-message-audit-history-pagination.jpeg`: the viewport stays
  near the previous content after prepending history, but the visible task
  title incorrectly changes from `history user message 103` to
  `history user message 53` because a partial page is treated as the start of
  the task.

The baseline confirms that Unicode, multiline user prompts, the basic role
treatments, streaming, tool results, waiting, and one-copy optimistic display
are readable. It also confirms these defects and untested gaps:

1. Assistant messages are plain text; Markdown syntax and fenced code are not
   rendered. This is still listed as a limitation in `README.md`.
2. A partial history page supplies a false task title, so loading an older page
   visibly renames both the header and selected sidebar row.
3. A persisted wait row and the live waiting footer repeat the same state.
4. Failed and interrupted status projections have labels in code but no
   end-to-end fake-agent scenario or native evidence.
5. User and tool content are silently cut at fixed character counts by the
   renderer, with no indication or way to read the omitted text.

## Required message behavior

### Content and roles

1. Mailbox/user content remains a right-aligned prompt surface. Unicode,
   grapheme clusters, explicit newlines, long unbroken tokens, and ordinary
   URL/code-like text must remain intact; rendering must not silently discard
   content.
2. Assistant content uses the existing quiet prose treatment and renders the
   useful Markdown subset used by coding agents: paragraphs, headings,
   emphasis, strong text, strike-through, inline code, fenced code, ordered and
   unordered lists, task-list markers, block quotes, horizontal rules, and
   links. Unsupported constructs must degrade to readable text rather than
   disappear or expose raw parser errors.
3. Streaming assistant deltas remain one provisional assistant item. A final
   assistant message replaces that preview rather than duplicating it, and
   incomplete Markdown received mid-stream must remain readable.
4. Tool results remain visually distinct, preserve whitespace/newlines, use a
   code-oriented treatment, and do not silently remove the tail of the result.
5. Empty assistant tool-call rounds remain hidden. Empty tool, wait, or failure
   reasons must still produce an understandable state label.

### State and reconciliation

1. Sending inserts one immediate local user line and clears the composer.
   Whether the SSE echo arrives before or after the mailbox HTTP response, only
   one user line remains. A failed send removes that local line, restores the
   exact composer text, and shows the error.
2. `thinking`, `tools_started`, `tool_finished`, `waiting`, `failed`,
   `finished`, and `interrupted` remain distinguishable. A live wait footer is
   suppressed when an identical persisted wait row is already the last item;
   `clear` does not create a noisy transcript footer.
3. Reconnect catch-up cannot duplicate a pending user line or a finalized
   assistant line, and it cannot leave a stale streaming preview next to the
   same final message.

### History and title

1. Initial and older pages remain oldest-first after filtering empty assistant
   rounds. Repeated pages or overlapping boundary items must not create
   duplicate rows.
2. Prepending older history preserves the reader's visible neighborhood rather
   than jumping to the oldest item or the bottom.
3. A partial page must never masquerade as the start of the task. Until the
   true oldest page is known, the task uses its honest stable `Task <id>`
   fallback; once the oldest page is loaded, the first non-empty user line may
   become the title and must remain stable.

## Component source and compatibility rule

Longbridge GPUI Component's `TextView` is the behavioral reference for the
Markdown block/inline model. The installed `gpui-component 0.5.1` uses a
different `gpui` package than zork's `gpui-unofficial 1.17.0-pre`, so this pass
ports only the needed Markdown parsing/rendering ideas into a small local
component. It must not import a second GPUI runtime, Longbridge theme/root, HTML
viewer, editor, image loader, or the full syntax-highlighting dependency set.

## Regression strategy

Before implementation, regressions must fail against the current code for:

- Markdown block/inline structure and readable malformed/incomplete input;
- full Unicode/long-content preservation;
- wait/live-status deduplication and visible failure/interruption labels;
- optimistic user reconciliation in both event orders;
- stable partial-history titles and ordered, duplicate-free prepending;
- fake-agent Markdown, Unicode/tool, failed, interrupted, and completion
  sequences over the existing public endpoints/event names.

After implementation:

- inspect native screenshots for the baseline history, rich Markdown/code,
  streaming, tool/wait, failure, interrupted, and paginated-history states;
- run `cargo fmt -p zork-gui -- --check`;
- run `cargo test --locked -p zork-gui`;
- run `cargo clippy --locked -p zork-gui --all-targets -- -D warnings`;
- run `uv run crates/zork-gui/tests/test_fake_agent.py`;
- run `cargo build -p zork-gui` and the repository `vp run build` gate;
- update `README.md` and `design-qa.md` only after the implemented behavior and
  native evidence satisfy this document.

## Constraints and non-goals

- Do not modify Surge.
- Do not attach to, restart, or modify Codex on this machine or mini2.
- Do not change the public `zork-agent` API or add a production-only mock path.
- Do not regress the accepted shell, composer, selector, or native window
  chrome.
- Do not add remote-image fetching, embedded HTML/CSS, Mermaid, math layout,
  or a full code editor in this pass.

## Completion self-check — 2026-08-27

- Content and roles: passed. Assistant messages now use the local GFM block
  and inline model; streamed/incomplete Markdown stays readable; user and tool
  rows no longer have fixed silent character cutoffs; the native long-tool
  fixture visibly reaches `tool tail sentinel`.
- State and reconciliation: passed. Send inserts one immediate user row, SSE
  echo and reconnect history consume the pending row without duplication, and
  request failure rolls back/restores the prompt. Clear and identical
  persisted/live waits are suppressed; thinking, failed, completed, and
  interrupted states remain distinct.
- History and title: passed. Pages remain oldest-first, exact retry overlap is
  removed, prepend preserves the visible neighborhood, and a partial page uses
  `Task <id>` until the oldest page is known.
- Fake agent: passed. `fixture:markdown` covers streamed/final Markdown,
  Unicode, completion, and a tool result longer than 1,500 characters;
  `fixture:failed` projects a reason; `fixture:interruptible` proves cancel
  emits interrupted and prevents the background run from writing later rows.

Final native evidence:

- `design-audit/73-message-rich-home-stable-title.jpeg` — stable partial title.
- `design-audit/74-message-rich-history-stable-no-clear-footer.jpeg` — history
  roles without a noisy clear or duplicated wait footer.
- `design-audit/75-message-rich-optimistic-thinking.jpeg` — one immediate user
  row while thinking.
- `design-audit/76-message-rich-markdown-code.jpeg` — heading, emphasis,
  strike-through, inline code, list/task marker, quote, and fenced code.
- `design-audit/77-message-rich-full-tool-tail-wait.jpeg` — full Unicode tool
  tail, completion, and one persisted wait.
- `design-audit/78-message-rich-failed.jpeg` — visible failure reason.
- `design-audit/79-message-rich-pagination-stable-title.jpeg` — older history
  prepended while the partial title remains stable.
- `design-audit/80-message-rich-interrupted.jpeg` — interrupted header/footer;
  the native view remained unchanged after the fixture's full delay.
- `design-audit/81-message-rich-final-bundle.jpeg` — rebuilt, re-signed final
  app bundle showing Markdown, a clickable link, fenced code, and tool output.

Final gates:

- `cargo fmt -p zork-gui -- --check`: passed.
- `cargo test --locked -p zork-gui`: 35 passed after the later composer-focus
  color regression was added.
- `cargo clippy --locked -p zork-gui --all-targets -- -D warnings`: passed.
- `uv run crates/zork-gui/tests/test_fake_agent.py`: 7 passed.
- `cargo build --locked -p zork-gui`: passed.
- `vp run build`: passed.

The accepted composer, shell, selector, and native-window contracts remain
green. Surge, local/mini2 Codex, and the public agent API were not modified.
