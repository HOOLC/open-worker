# zork-gui native window chrome acceptance

## User request

> 你没发现窗口 title 不一样吗

The prior shell comparison was invalid at the outer-window boundary: the Codex
reference was a CDP content capture while the zork implementation was a full
macOS window capture. This correction compares complete window geometry and
removes the extra zork-native title row instead of cropping it out of QA.

## Native source of truth — mini2, 2026-08-27

- Window Server owner: `ChatGPT` (the Codex Desktop process on mini2).
- Window Server window number: `875`.
- Native window title (`kCGWindowName`): empty.
- Native window bounds: x `330`, y `75`, width `1479`, height `826`.
- CDP page viewport: width `1479`, height `826`, DPR `1`.
- Packaged Codex source (`app.asar`) configures the primary Electron window
  with `titleBarStyle: "hiddenInset"`.
- The same packaged source computes `trafficLightPosition` from constants
  `x = 16`, titlebar height `46`, and button height `14`; at the observed
  zoom factor `1`, the resulting position is exactly `{ x: 16, y: 16 }`.
- The exact native-window/CDP-viewport size equality proves that Codex uses a
  full-size content view under a transparent titlebar. It does not reserve a
  separate macOS title row above the app UI.
- The normal macOS traffic-light controls remain AppKit-owned in that
  transparent titlebar, at the source-defined `{ x: 16, y: 16 }` position.

## Failing implementation evidence

- Window Server owner: `Zork GUI`.
- Native window title (`kCGWindowName`): `zork`.
- Before-fix native bounds: x `0`, y `33`, width `1470`, height `833`.
- `main.rs` uses GPUI's default opaque/native titlebar and then calls
  `window.set_window_title("zork")`.
- The full native capture therefore contains an extra titlebar with traffic
  lights, the text `zork`, and a divider before the zork content begins.

## Acceptance

- Configure GPUI with `TitlebarOptions.appears_transparent = true` so the app
  content uses the full window bounds.
- Set no native window title and do not call `set_window_title` after creation.
- Retain the system traffic lights and place them at the exact Codex
  `{ x: 16, y: 16 }` inset; do not draw, hide, or counterfeit them.
- Reserve the measured 46 px sidebar toolbar clearance inside the app content,
  then place the zork brand row below it. This prevents the native traffic
  lights from overlapping the brand while preserving the live Codex y-axis.
- The selected-task header begins at window y `0` in the main surface, matching
  Codex. The home state keeps its measured 46 px clear header region.
- Complete-window source and implementation screenshots must be compared at
  equal visible widths with proportional scaling. Content-only-vs-window
  comparisons are not acceptable evidence for this gate.
- Existing sidebar, home, transcript, composer, agent API, and task behaviors
  remain unchanged apart from the corrected full-window coordinate system.
- Do not modify local Codex, mini2 Codex, or Surge.

## Verification

1. Add a regression that requires a transparent titlebar, an absent native
   title, and platform-owned traffic lights at `{ x: 16, y: 16 }`; capture its
   failure first.
2. Implement the titlebar options through one shared function used by the app.
3. Rebuild and cold-launch the `.app`, then read `kCGWindowName` and native
   window bounds from Window Server.
4. Capture native home and selected-task states, create a complete-window
   comparison, and keep `design-qa.md` blocked until no P0/P1/P2 findings
   remain.
5. Run formatting, Rust tests, clippy, and fake-agent E2E checks.
