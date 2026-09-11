# zork-ui

Shared Rust/GPUI visual components used by the native client and the design handbook's interactive WebAssembly examples. The package owns tokens, fonts/SVG assets, buttons, fields, dropdowns, navigation rows, avatars, provider marks, Markdown rendering and text selection, activity and brand motion. It has no Station, database, mesh or HTTP service dependency.

The `stories` feature adds the primitive catalog and isolated examples. Normal client builds omit it. `zork-gui` composes these controls into application views; the Web example harness compiles those same settings view files against an in-memory adapter. Full conversation pages currently have native snapshots only.

Native and Web render the same Rust implementation. Text rasterization and CJK font fallback differ by platform: native uses system fonts; Web embeds Noto Sans SC under OFL. Page examples retain the live original HTML and optional snapshots for comparison. Primitive tabs use GPUI exclusively.

From the repository root:

```sh
python3 scripts/storybook/test_package.py
cargo run --locked -p zork-gui --features headless-bench --bin zork-gui-storybook
python3 scripts/storybook/build.py
uv run scripts/storybook/test_web.py
uv run scripts/storybook/test_web.py --backend webgl --output artifacts/storybook/webgl-checks
```

The build command exports native component/window PNGs and geometry, captures the existing local design reference, builds GPUI Web and generates the comparison gallery under the monorepo `apps/zork-design/components` directory. It uses the design reference server started by `pnpm design:dev`. It does not publish the site or connect to a real node. See `scripts/storybook/build.py --help` for options.

Sidebar navigation uses `navigation::TabGroup` for both device/task rows and settings tabs.
Each sidebar owns a group and wraps its complete content in `surface`; `tab` provides
hover/pressed feedback and `column` provides the 2 px row gap. The active tab has an
independently sliding 2 × 14 px leading marker in `BRAND_ACCENT` orange, with no selected fill or change in
font weight. Both surfaces are painted across the whole group, so section gaps do
not clip their motion; settled surfaces respect their target's scroll viewport.

`components::tooltip::sliding_popup` provides a keyed floating surface for custom
read-only content. Timeline nodes reuse one key, move the anchor to the hovered
node and retain the last painted position and size through reversals. It shares
the navigation details overlay's distance-adaptive, critically damped motion, clamps to the viewport,
snaps with reduced motion and stops scheduling frames once settled. Content
height is measured once per content key and width.

All sliding surfaces retain their painted position and velocity when retargeted.
From rest, 34 / 160 / 600 px moves settle in approximately 70 / 140 / 225 ms;
the response approaches a 240 ms limit for longer travel. They accelerate and
brake continuously, with extra braking when a nearer target is reached mid-flight.

The gallery has one tab per primitive family and one tab per application page. Primitive tabs render every state in a single GPUI canvas using `FamilyStories`, with separate entities and namespaced control IDs. Only page examples retain the live HTML comparison; scene and viewport selection stay inside their page tab. Web embeds explicit 400/500/600/700 font instances because fontdb otherwise indexes the CJK variable font at its Thin default.

Modal backdrops use a 35% black scrim, leaving the foreground card clear. On macOS,
the compositor layer covers the window frame above the native titlebar, and converts
card coordinates from the GPUI view into that frame. `scripts/storybook/test_native_scrim.m`
checks regular/full-size windows, resizing, rapid reopening and layer cleanup.

`components::liquid_composer` paints a cached GPUI silhouette for the composer,
member bubbles and connecting necks. A compact smooth distance union produces the liquid field;
boundary tracing and marching squares extract smooth cubic contours without
full-area blur passes. Headless builds retain a Gaussian approximation reference
for comparisons (`ZORK_LIQUID_COMPARE_BLUR=1`). The solid surface matches the sidebar (`#F6F5F1`), with a 0.5 px `#DEDFDF` geometric
stroke traced from the same complete contour as the fill;
no native input view or platform material is involved. Transparent avatar variants
live under `assets/avatars/portraits/`.

`components::attachment_fan` owns the shared file poses and closed aperture
geometry. A single file uses a straight slot; multiple files change its width
and curvature. On expansion it becomes a rounded capsule with a common final
height. `SurfaceCache::element_with_attachments` cuts that closed contour out of
the composer with even-odd fill, retaining the member silhouette and rounded
outer corners. Native draft membership transitions live in `zork-gui`; the
shared geometry has no file-storage or service dependency.

The default path uses optimized CPU tessellation without a GPU readback.
On macOS `ZORK_LIQUID_GPU=1` enables an experimental precompiled Metal compute kernel. Its shared
output buffer becomes a cached GPUI image; text, input and hit testing remain
ordinary GPUI elements. The shader library is built with Xcode's Metal toolchain.
Other platforms retain the CPU contour renderer, also selectable on macOS with
`ZORK_LIQUID_CPU=1`. The measured CPU frame cost includes GPU completion and the
image handoff; kernel timing alone is not reported as end-to-end performance.

ARM64 also has an optional four-point NEON sampler (`ZORK_LIQUID_SIMD=1`).
Scalar/SIMD contact and separation tests agree within 0.0001 px; paired native
headless runs did not show a material end-to-end gain, so scalar remains the
default. `ZORK_LIQUID_SCALAR=1` overrides the experimental sampler switch.

## Component contract

The approved rules start at [`docs/gui-approved-design.md`](../../docs/gui-approved-design.md), with the [state contract](../../docs/ui-interaction-states.md) and [migration ledger](../../docs/ui-system-unification.md). `design::INTERACTION`, `design::FORM` and `TextRole` own common feedback and typography. Use the semantic controls rather than overriding ordinary state colors in application views. Platform-specific navigation and touch geometry remain explicit.

`interaction-overview` exercises action feedback; `interaction-form` combines production fields, a select, a switch, text actions and local save/error/busy states. Existing model/connection stories render the actual settings view against isolated fixtures. Fixed state swatches are reference samples; input-driven checks provide behavioral evidence.

`components::collapse` measures uncompressed content at the available width and animates
its clipped layout height with a shared speed limit and smooth acceleration/braking.
The content retracts slightly and fades as the aperture closes. Retain it by stable ID while closed;
use `mounted` to omit settled hidden rows and `interactive` for descendant tab stops.
Its frame callback must invalidate the owning region’s intrinsic height. The
`navigation-fold-open` / `navigation-fold-closed` stories run the same component
on native and Web. `scripts/storybook/test_collapse.py --url <web-index-url>`
checks Web reversal, removal, keyboard activation and idle behavior.
