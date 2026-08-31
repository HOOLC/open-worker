# zork-gui Codex visual QA

> Visual captures remain valid. Direct-Agent tool/wait/delta transcript states
> are historical; current messages come from the gateway contract in
> `GATEWAY_IM_ENTRY_ACCEPTANCE.md`, with Agent state rendered only as activity.

**Comparison target**

- Source visual truth: live Codex on mini2, inspected through loopback-only CDP.
- Source home screenshot: `design-audit/44-codex-cdp-shell-empty-full.png`.
- Source selected-task screenshot: `design-audit/47-codex-cdp-shell-task-first-message.png`.
- Source composer screenshot: `design-audit/33-codex-cdp-source-composer.png`.
- Source computed styles and box model: `design-audit/codex-composer-cdp-computed.json`.
- Final complete native home implementation: `design-audit/61-zork-native-window-home-titlebar-exact-active.jpg`.
- Final complete native selected-task implementation: `design-audit/60-zork-native-window-task-titlebar-exact.jpg`.
- Final native-window comparison: `design-audit/62-window-chrome-comparison.png`.
- Final native Unicode input: `design-audit/63-component-input-unicode.jpg`.
- Final fixed-height soft wrap: `design-audit/64-component-input-soft-wrap.jpg`.
- Final keyboard-highlighted selector menu and exact choice: `design-audit/65-selector-menu-open.jpg` and `design-audit/66-selector-menu-choice.jpg`.
- Final message-rendering matrix: `design-audit/73-message-rich-home-stable-title.jpeg` through `design-audit/81-message-rich-final-bundle.jpeg`.
- Final composer-focus color correction: `design-audit/82-composer-blue-user-report.png` through `design-audit/86-composer-focus-crop-before-after.png`.
- Final implementation composer crop: `design-audit/37-zork-cdp-corrected-composer.jpg`.
- Final proportional shell comparisons: `design-audit/54-shell-comparison-home.png` and `design-audit/55-shell-comparison-task.png`.
- States: light-theme new-task shell and selected-task transcript, `open-worker` workspace, local fake agent connected.

**Viewport and normalization**

- Source viewport: 1479 × 826 CSS px at device pixel ratio 1.
- Earlier native shell captures: 1356 × 768 pixels. The comparison page gives
  source and native captures equal visible widths while preserving both source
  aspect ratios; no one-axis stretching is used.
- Final complete zork window: 1280 × 800 native bounds; the capture service
  returns 1229 × 768 pixels at the current display scale. Window Server and
  macOS Accessibility both report an empty native title.
- Source sidebar: 275 px. Native sidebar contract: 275 logical px.
- Source home suggestion row: 710 × 104 px with a 12 px gap. Native contract:
  the same values.
- Source selected-task column: x = 351, width = 736. Native task content and
  follow-up composer start 76 logical px inside the 275 px main boundary and
  share the same 736 px width.
- Source composer box: x 509, y 669, 736 × 141 CSS px, with a 16 px viewport-bottom inset.
- Source project utility: 710 × 61 px, inset 13 px horizontally and 4 px from the composer top.
- Source main surface: 736 × 98 px, beginning 43 px below the composer top, with a 25 px radius.
- Implementation pixels: 1182 × 768. GPUI requested a 1280 × 800 logical window; the macOS native capture contains the 1182 × 768 on-screen window including titlebar at the current display scale.
- The 736 × 141 source and 682 × 130 native crops are each rendered at the same 572 px visible width. Their heights remain automatic (109.58 and 109.03 px); no single-axis stretching is used.
- Native chrome is no longer excluded from fidelity findings. Codex uses
  Electron `hiddenInset` with source-defined traffic lights at 16 × 16; zork
  now uses transparent GPUI titlebar options, the same inset, and no title.

**Findings**

- Resolved [P1] Native titlebar scope mismatch.
  Location: complete macOS window / `src/main.rs`.
  Before evidence: mini2 Codex exposed an empty native title and a 1479 × 826
  native window whose bounds exactly equaled its 1479 × 826 CDP viewport;
  zork exposed title `zork` and reserved an opaque AppKit titlebar.
  Fix: the shared GPUI options now use full-size transparent content, no title,
  and AppKit-owned traffic lights at the packaged Codex source's exact 16 × 16
  inset. The sidebar reserves 46 app-owned pixels beneath the overlay controls.
  Verification: the rebuilt app reports native title `""`, Window Server
  bounds 1280 × 800, and complete home/task captures show no separate title
  row or divider. The regression was captured failing with a default traffic
  light position before passing with 16 × 16.

- Resolved [P2] Coding messages were rendered as raw plain text and long user
  or tool content was silently cut by the view.
  Location: selected-task transcript / `src/components/message.rs` and
  `src/views.rs`.
  Fix: added a local GFM block/inline renderer compatible with the existing
  GPUI runtime, preserved full user/tool content, and kept streamed incomplete
  Markdown readable. Native evidence shows heading/emphasis/strike/inline
  code, lists/task markers, quote, fenced code, link treatment, Unicode, and
  the long tool tail sentinel.

- Resolved [P2] Partial history supplied a false task title and repeated live
  wait/clear activity.
  Location: selected-task header/sidebar and transcript activity footer.
  Fix: use the stable task id until the true oldest page is loaded, reconcile
  pending user echoes across SSE/history, remove retried page overlap, and hide
  clear or an identical persisted/live wait. Failure and interruption reasons
  remain explicit.

- Resolved [P2] Focus recolored the entire editor pale blue.
  Location: follow-up composer / `src/components/text_input.rs`.
  Before evidence: the user screenshot and native reproduction both sample
  `(248, 251, 255)` across the empty editor because a `#339cff` wash was
  applied at 4% opacity.
  Fix: removed that full-surface focus fill. The rebuilt focused editor samples
  `(255, 255, 255)`, while its caret, focus handle, and text-selection highlight
  remain intact.

- No actionable P0, P1, or P2 findings remain in the live-source/native comparison.
- [P3] Home recommendations use zork-owned actions and Phosphor glyphs.
  Location: home suggestion row.
  Evidence: the source uses Codex-specific localized prompts and a proprietary
  product mark; zork uses four real prompt-fill actions and an embedded
  terminal glyph while preserving measured size, rhythm, type, and elevation.
  Fix: none; counterfeiting the source mark or dead recommendation copy would
  violate the goal.
- [P3] The selected-task right rail remains visually empty.
  Location: selected-task right side.
  Evidence: Codex shows source/output controls that zork cannot back with real
  behavior. The 76 px transcript inset and reading column remain measured, but
  the panel itself is not fabricated.
  Fix: none until the agent exposes equivalent data and actions.
- [P3] Composer control copy remains product-specific.
  Location: bottom control row.
  Evidence: Codex shows add, approval, and GPT controls; zork shows its real
  profile, reasoning, and model selectors with matching compact icon treatment.
  Impact: labels differ, while density, alignment, hierarchy, and interaction
  placement now match. Copying the Codex labels would create fake controls.
  Fix: none for this scope.
- [P3] Product-scope navigation is intentionally smaller.
  Location: left task rail.
  Evidence: Codex shows product-only destinations, folders, icons, and relative-time metadata; zork shows the working new-task action, workspace grouping, task rows, model label, and agent health only.
  Impact: the rail is a little less information-dense, but adding unavailable Codex destinations would create dead controls and violate the product goal.
  Fix: none for this scope; add matching iconography and time metadata only when backed by real zork capabilities.

**Required fidelity surfaces**

- Fonts and typography: live computed CSS establishes 28/33.6 weight-400 home type, 14/21 navigation, 14/22 assistant prose, and 16/24 user prompts. The native system text uses those measured sizes and remains readable without clipping.
- Spacing and layout rhythm: the quiet left rail, large white work canvas, centered prompt heading, bottom composer, soft selected surfaces, thin borders, restrained radius, and generous negative space match the source structure. The 1280 × 800 geometry contract prevents sidebar, transcript, and composer overlap.
- Colors and visual tokens: white canvas, `#fbfbfb` rail, `#1a1c1f` primary text, `#f2f2f2` selected/prompt fills, black primary action, and restrained agent-state color map to the live computed values. Native window chrome is explicitly forced light.
- Image quality and asset fidelity: the target screen has no required product photography or decorative imagery. No logo, illustration, custom SVG, emoji, CSS drawing, or placeholder image was fabricated. Codex-only logo and icons were omitted rather than approximated.
- Copy and content: the prompt-led empty state is coherent on its own and uses zork-specific language. Workspace, task, model, reasoning, connection, and action copy correspond to real data or real actions.
- Icons and affordances: embedded MIT-licensed Phosphor assets provide folder, profile, reasoning, model, disclosure, send, and stop glyphs. The 28 px circular action matches the source size and swaps from send to stop while working.
- Accessibility: light-theme contrast is strong for primary content; muted labels remain readable; Enter sends, Shift+Enter inserts a newline, working state exposes Stop, and text focus remains visible through the insertion caret and selection highlight without recoloring the editor. GPUI's custom content is not exposed by the current macOS capture service, so screen-reader semantics remain a residual evidence gap rather than a visual blocker.
- Responsiveness: the supported native desktop target is 1280 × 800. Contract tests resolve the primary regions without overlap and reject unsupported sub-900 × 600 geometry rather than silently collapsing the layout.

**Primary interactions tested**

- Create session with the chosen profile/model/reasoning/workspace, then append the initial prompt.
- Select a recent session, load paginated transcript history, and map mailbox messages to user prompts.
- Update model selection before sending, append a mailbox follow-up, render SSE status/deltas, and cancel a running session.
- Enter/Shift+Enter behavior, new-task validation, duplicate suppression, streaming UTF-8 chunk handling, and reconnect-safe status projection.
- Render streamed/final Markdown, Unicode, a tool result longer than 1,500
  characters, completion, failure, and interruption; verify cancellation does
  not allow the background fixture to resume.
- The rebuilt and re-signed native app bundle was exercised against the local
  fake agent with coordinate-based interaction where GPUI custom controls are
  absent from the Accessibility tree. Rust and fake-agent regressions cover
  the same state/reconciliation rules independently.

**Comparison history**

1. Baseline — `04-zork-before-1280x800.jpg` and `05-zork-task-before-1280x800.jpg`.
   Earlier findings: P1 dark admin-dashboard shell; P1 generic bordered session list; P1 edge-to-edge transcript/global diagnostics; P1 thin toolbar composer; P2 tiny low-contrast labels and weak targets.
   Fixes: deleted the old visual branch; introduced the light Codex palette and geometry contract, grouped task rail, centered transcript, inline activity, prompt/prose treatments, and fixed rounded composer.
   Post-fix evidence: `06-zork-after-pass1-1280x800.jpg`.
2. Pass 1 — `06-zork-after-pass1-1280x800.jpg`.
   Earlier findings: P2 duplicated new-task/connection header competed with the empty state; P2 workspace-only composer did not support the actual initial task prompt; P2 disconnected placeholder state dominated the first impression.
   Fixes: removed the redundant top status/header, placed workspace context inside the composer, added multiline initial prompt and real create-then-append behavior, and connected the seeded fake-agent state.
   Post-fix evidence: `07-zork-after-pass2-1280x800.jpg`.
3. Pass 2 — `07-zork-after-pass2-1280x800.jpg` and `10-zork-final-1280x800.jpg`.
   Earlier findings: P2 native titlebar could follow the OS dark appearance while the app canvas remained light; P2 keyboard focus was not visually strong enough.
   Fixes: forced GPUI/macOS window chrome to light, added the light-chrome contract regression, and added explicit focused-composer border treatment.
   Post-fix evidence: `11-zork-final-light-1280x800.jpg`, `12-side-by-side-final.jpg`, and `17-focused-comparison.jpg`.
4. User review — `17-focused-comparison.jpg`.
   Earlier finding: P1 composer silhouette, height, divider, path emphasis,
   selector distribution, and text-pill action remain too far from the source.
   Fixes: replaced the text action and divided form with a shared tray/surface
   composer and real icon-library assets.
   Post-fix evidence: `18-zork-composer-pass1-1280x800.jpg` and
   `19-implementation-composer-pass1-focus.jpg`.
5. Composer pass 1 — `18-zork-composer-pass1-1280x800.jpg` and
   `19-implementation-composer-pass1-focus.jpg`.
   Earlier finding: the structure, density, path emphasis, selector split, and
   icon action are substantially closer, but a P2 clipped outer outline and
   flat tray/input seam still make the component read as one bordered form;
   placeholder contrast is also too dark and the bottom inset is too tight.
   Fixes: separated the overlapping tray/input surfaces, lightened the prompt,
   softened the border, and corrected bottom spacing.
   Post-fix evidence: `20-zork-composer-pass2-1280x800.jpg`,
   `21-implementation-composer-pass2-focus.jpg`, and
   `22-composer-side-by-side-pass2.jpg`.
6. Composer pass 2 — `22-composer-side-by-side-pass2.jpg`.
   Earlier finding: the tray was measured only by its exposed strip, producing
   a P1 tray that was too shallow and an input surface that began too high. The
   600 × 120 implementation crop was also stretched on one axis in the QA page,
   making the comparison itself unreliable.
   Fixes: corrected the logical geometry to a 48 px tray, 80 px input surface,
   8 px overlap, and 120 px total; aligned prompt/control padding; removed the
   tray outline; added compact selector icons; and normalized the 600 × 112
   native crop uniformly.
   Post-fix evidence: `29-zork-composer-final-1280x800.jpg`,
   `30-implementation-composer-final-focus.jpg`, and
   `31-composer-side-by-side-final.jpg`.
7. Live CDP correction — `33-codex-cdp-source-composer.png` and
   `codex-composer-cdp.json`.
   Earlier findings: the screenshot-derived 640 × 120 contract remained a P1
   mismatch against the live 736 × 141 component, and its radius, shadow,
   editor, and control metrics were approximate.
   Fixes: replaced the stale geometry with the measured 61 px tray, 98 px
   surface, 18 px overlap, 25 px radius, four-layer shadow, 44 px editor,
   28 px controls, and live typography/color tokens.
   Post-fix evidence: `35-zork-cdp-corrected-full.png`,
   `37-zork-cdp-corrected-composer.jpg`, and
   `42-cdp-native-side-by-side-final-focus.jpg`. No actionable P0, P1, or P2
   visual findings remain.
8. Live shell correction — `44-codex-cdp-shell-empty-full.png`,
   `47-codex-cdp-shell-task-first-message.png`, and the three shell CDP JSON
   captures.
   Earlier findings: P1 224 px rail and 38 px task rows; P1 two-line 52 px task
   header; P1 generic subtitle-only home state; P1 selected-task transcript and
   composer centered against the wrong region; P2 `/` workspace when launched
   as a macOS app bundle.
   Fixes: replaced those values with the measured 275/30/46 px shell, added
   four real prompt suggestions, adopted the agent workspace only for bundle
   root cwd, and aligned the 736 px task column/composer at the measured 76 px
   inset.
   Post-fix evidence: `52-zork-shell-home-final.jpg`,
   `53-zork-shell-task-final.jpg`, `54-shell-comparison-home.png`, and
   `55-shell-comparison-task.png`. No actionable P0, P1, or P2 visual findings
   remain.
9. Complete native-window correction — `59-zork-native-window-titlebar-exact.jpg`,
   `60-zork-native-window-task-titlebar-exact.jpg`, and
   `62-window-chrome-comparison.png`.
   Earlier finding: the QA compared Codex CDP content pixels against a complete
   zork macOS window, hiding a P1 outer-window mismatch: zork had an opaque
   title row and native title `zork`.
   Fixes: read the mini2 Window Server properties and packaged Electron config,
   replaced the default GPUI chrome with transparent full-size content, removed
   the title, moved system traffic lights to the exact 16 × 16 inset, and added
   the 46 px sidebar overlay clearance.
   Post-fix evidence: Window Server and AX both report title `""`; the full
   native app begins at y = 0 and keeps system-owned traffic lights. No
   actionable P0, P1, or P2 visual findings remain.
10. Message rendering correction — `67-message-audit-home-entry.jpeg` through
    `72-message-audit-history-pagination.jpeg` established the live baseline.
    Earlier findings: P2 raw Markdown; P2 silent user/tool truncation; P2 false
    title changes when older history loaded; P2 duplicated wait/clear activity;
    and missing failure/interruption native coverage.
    Fixes: introduced the local Markdown component and pure transcript
    reconciliation module, added deterministic fake-agent fixtures, preserved
    full content, stabilized titles/history, and made cancel generation-aware.
    Post-fix evidence: `73-message-rich-home-stable-title.jpeg` through
    `81-message-rich-final-bundle.jpeg`. No actionable message-rendering P0,
    P1, or P2 findings remain.
11. Composer focus-color correction — `82-composer-blue-user-report.png`,
    `83-composer-blue-current-focus.jpeg`, and
    `84-composer-neutral-fixed-focus.jpeg`, with the same-scale composer crop
    comparison in `86-composer-focus-crop-before-after.png`.
    Earlier finding: P2 the focused editor used an unintended low-opacity blue
    fill even though the accepted source editor is transparent over a neutral
    near-white surface.
    Fix: deleted the full-editor focus wash while retaining caret and selection
    feedback. Empty editor samples changed from `(248, 251, 255)` to pure white
    in the rebuilt signed app. No actionable focus-color finding remains.

**Open Questions**

- Exact assistive-technology labeling should be verified when GPUI exposes the custom control tree to macOS Accessibility APIs.
- Codex-only destinations, review/diff surfaces, and proprietary assets remain intentionally out of scope until zork has real backing behavior.

**Implementation Checklist**

- [x] Replace the dashboard shell with the Codex task hierarchy.
- [x] Preserve real agent API/SSE behavior and new-task initial prompt delivery.
- [x] Add visible keyboard focus and light native window chrome.
- [x] Implement the live-CDP composer correction.
- [x] Implement the live-CDP sidebar, home, header, task-row, and transcript correction.
- [x] Verify the macOS app-bundle cwd path and clickable home suggestions.
- [x] Replace manual composer keys and click-to-cycle selectors with native
      input and exact menu components.
- [x] Recapture full-view and focused side-by-side evidence with proportional normalization.
- [x] Pass zork-gui formatting, all 35 Rust tests, clippy, and all 7 fake-agent E2E checks.
- [x] Attempt repository-wide gates and record unrelated failures without modifying their files.

**Follow-up Polish**

- Add real icon-library glyphs and relative task times if the agent later exposes corresponding metadata and destinations.
- Add richer custom-control accessibility semantics when GPUI exposes the
  component tree to macOS assistive technologies.

**Repository gate note — 2026-08-27**

- `vp run build` passes.
- Workspace tests are currently blocked outside `crates/zork-gui` by the dirty
  gateway worktree: `AppState.slack`/`post_message` test-build errors, two
  pre-existing TypeScript format failures, one `max-lines` lint failure, and
  five related repository E2E failures (75 of 80 pass). Those files were not
  changed as part of this composer correction.

**Component port verification — 2026-08-27**

- The composer now registers `EntityInputHandler`/`ElementInputHandler` and no
  longer has a `RootView::composer_key_down` or `self.composing` mutation path.
  Native QA covered Unicode clipboard input, visible caret/selection,
  Shift+Enter, explicit newlines, soft wrapping, and fixed-height scrolling.
- Profile, reasoning, and model controls now open one anchored menu with an
  active marker and exact item selection. The menu takes keyboard focus;
  Up/Down changes highlight, Enter selected `fake-2` without submitting a task,
  and Escape dismisses before composer focus is restored.
- The final implementation preserves the measured 736 × 141 composer and the
  existing shell/window chrome. Evidence is stored in `design-audit/63` through
  `design-audit/66`.
- All 35 zork-gui Rust tests, formatting, clippy with warnings denied, all seven
  fake-agent E2E checks, `cargo build --locked -p zork-gui`, and `vp run build`
  pass.
- Surge, Codex on this machine, Codex on mini2, and the public agent API were
  not modified.

**Message rendering verification — 2026-08-27**

- Native health: passed. User, assistant, tool, wait, thinking, failed,
  finished, and interrupted treatments are readable without shell/composer
  regressions. The final bundle remains open on the rich Markdown/tool view.
- Content health: passed. GFM coding constructs render with clear hierarchy;
  Unicode and line breaks survive; the tool fixture reaches its explicit tail
  sentinel with no silent cutoff.
- State health: passed. Optimistic send is one copy, reconnect pages consume
  pending echoes, duplicate wait/clear footers are absent, and an interrupted
  fixture cannot later append completion rows.
- History health: passed. Older pages prepend in order and the selected task
  keeps `Task <id>` while history remains partial.
- Automated health: passed. Formatting, 35 Rust regressions, denied-warning
  clippy, seven HTTP/SSE fake-agent E2E checks, crate build, and repository
  build are green.

**Composer focus-color verification — 2026-08-27**

- Focused surface health: passed. The native follow-up editor is neutral white
  instead of pale blue.
- Input feedback health: passed. Focus tracking, caret painting, text selection,
  IME/editing bindings, and send behavior are unchanged.
- Fidelity health: passed. Composer geometry, tray, radius, shadow, controls,
  transcript, and window chrome did not move.
- Automated health: passed. Formatting, 35 Rust regressions, denied-warning
  clippy, seven fake-agent E2E checks, crate build, and repository build are
  green.

final result: passed
