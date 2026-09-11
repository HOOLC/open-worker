# GPUI Web component examples

This WASM harness depends directly on `zork-ui`. Basic story renderers belong to that package; composed model/Agent settings examples reuse native source files with an in-memory API adapter. There is no backend frame streaming and no HTML/CSS recreation of the controls.

Run `python3 scripts/storybook/build_web.py`. Serve the generated directory over HTTP on localhost (or HTTPS). GPUI tries WebGPU, with WebGL fallback; `?backend=webgl` forces the latter. The shell loads the gzip-compressed WASM with `DecompressionStream`, so the local server does not need special content-encoding headers. Initial load includes the embedded CJK font; interactions work offline afterward.

`test_web.py` uses real browser mouse, keyboard and IME composition events, then captures every supported story offline. It checks table selection, dropdown selection/width and modal footer visibility at compact and wide sizes. The six conversation page stories remain native snapshots.

The pinned GPUI version is `1.17.0-pre`; wasm-bindgen must match `Cargo.lock`. The build script can use a preinstalled wasm32 target or build std from rust-src. On mini1, its isolated linker wrapper uses LLVM 23 only for wasm-ld while the Homebrew Rust compiler retains LLVM 22. Do not change global LLVM/z3 links to build this harness.

The gallery has one tab per primitive family and one tab per application page. Primitive tabs render every state in a single GPUI canvas using `FamilyStories`, with separate entities and namespaced control IDs. Only page examples retain the live HTML comparison; scene and viewport selection stay inside their page tab. Web embeds explicit 400/500/600/700 font instances because fontdb otherwise indexes the CJK variable font at its Thin default.
