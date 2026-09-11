# Rust / Metal gpu-lexer experiment

This directory contains the reproducible harness and fixed source inputs.
Machine-specific results, process snapshots and generated reports stay local.
Run the harness to produce measurements for the checked-out fixture hashes.

Standalone benchmark, outside the production Cargo workspace. It ports the
`gpu-lexer@0.0.2` tokenizer and WebGPU dispatch to Rust + wgpu 29.0.4 (Metal).
The upstream WGSL and model are unchanged. No production highlighter is replaced.

## Provenance

- Author: Shu Ding; npm maintainer `quietshu`.
- Package: https://registry.npmjs.org/gpu-lexer/0.0.2
- Distribution: https://registry.npmjs.org/gpu-lexer/-/gpu-lexer-0.0.2.tgz
- Demo: https://gpu-lexer.vercel.app/
- The distributed package declares MIT in `vendor/package/package.json`;
  the three-file npm archive does not include a separate license text.
- `assets/checksums.json` pins the archive, original JS, extracted shader,
  and decoded weights. `extract.py` verifies the pinned archive, restores its ignored `dist` inputs,
  and regenerates assets without evaluating JS.
- Model weights contain 41,321 f32 values, decoded from the upstream quantized
  encoding. The native driver uses shader-f16 when supported, matching upstream.

## Reproduce

Run from the repository root on macOS with a Metal GPU:

```sh
eval "$(python3 scripts/lib/build_env.py --shell)"
export CARGO_TARGET_DIR="${ZORK_BUILD_ROOT:-$PWD/target}/isolated/gpu-lexer"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4
python3 benchmarks/gpu-lexer/extract.py
cargo build --locked --release --manifest-path benchmarks/gpu-lexer/Cargo.toml
cargo clippy --locked --release --manifest-path benchmarks/gpu-lexer/Cargo.toml -- -D warnings
uv run --script benchmarks/gpu-lexer/verify.py
python3 benchmarks/gpu-lexer/run.py
```

`verify.py` uses Playwright 1.58.0 and a locally installed Chrome for Testing.
It discovers Chrome in the Playwright cache, or accepts `CHROME_BIN` as an
explicit executable path. It closes its browser and HTTP server on completion.
`make-fixtures.py` refreshes committed fixture snapshots from current repository
sources. Published fixtures use synthetic infrastructure identifiers. Regenerating
or editing them changes the measurement baseline; compare their SHA-256 values
and rerun measurements rather than reusing reports from older inputs.

## Measurement contract

- 25 fixtures: five languages, approximately 512 B / 4 KiB / 16 KiB / 60 KiB /
  256 KiB. Whole-line prefixes and repetitions of repository sources; fragments
  may end mid-syntax. Source paths and SHA-256 values are in the fixture manifest.
- Release builds; main measurement 30 iterations per fixture per engine,
  confirmation 10 iterations, in independent processes. `LEXER_REPS` controls the
  count; the runner defaults to 30 for both rounds. Execution order alternates each iteration. Report main-run median and p95; use the shorter run to check the trend.
- GPU wall time includes UTF-16-compatible feature extraction, UTF-8 range mapping,
  input upload, all seven compute stages, synchronous readback, and span merging.
  Same-size buffers and bind groups are reused, as in upstream. A change in token
  count reallocates buffers in this prototype; first-use fixture time includes it.
- Syntect 5.3.0 uses exactly the production features (`default-syntaxes`,
  `default-themes`, `regex-fancy`) and `InspiredGitHub`. It runs the same line
  highlighting and adjacent-color merge, returning compact RGB spans instead of
  GPUI HighlightStyle. SyntaxSet/ThemeSet are reused. This is a highlighter-core
  benchmark, not an exact call to the production UI wrapper.
- Both engines recompute highlights on every timed iteration. Production's 32-item
  result cache is excluded; these results represent uncached work. They do not
  imply a benefit on cache hits. No streaming input is tested.
- GPU output uses nine category IDs; syntect output uses RGB. Theme mapping,
  GPUI layout, rendering, UI scheduling, power and memory peaks are not measured.
- The bundled syntect syntax set does not recognize `ts`; those rows report
  `syntect_supported: false` and no comparative syntect timing.
- Production bypasses unsupported languages and highlighting above 64 KiB or 4096 bytes per line. Rows with
  `zork_would_skip: true` compare full parsing outside that production policy;
  they are not current-app speedups.
- Initialization is measured separately. OS shader caches are not reset, so a
  fresh process is not necessarily a first-ever shader compilation.
- `run.py` waits for observed compiler/render-benchmark processes to finish and
  records overlaps every 0.5 seconds. Reject overlapping runs. This is a shared
  desktop, not a controlled power/thermal lab.

## Correctness

`verify.py` compares the Rust feature words and UTF-16 token boundaries against
upstream JavaScript exactly, then compares native GPU labels with the original
JS/WebGPU implementation in Chrome. The additional UTF-8 fixture covers Chinese,
emoji, CRLF, tabs and operator boundaries. Every returned native span is checked
for UTF-8 boundaries and complete coverage. Agreement with the original model
checks port fidelity, not syntax-highlighting accuracy.

Results and conclusions: `REPORT.md` (generated locally).

## Unmodified browser API comparison

`BROWSER-REPORT.md` (generated locally) compares the original npm public `parse` API
in a dedicated Chrome WebGPU worker against the recorded native results. Run
`uv run --script benchmarks/gpu-lexer/benchmark-browser.py`, then
`python3 benchmarks/gpu-lexer/report-browser.py`. No model or package source is
modified. The worker times the awaited API directly, excluding IPC and rendering.
Browser rounds use 30 and 10 samples; hardware adapter and competition monitoring
are recorded. This is WebGPU, not a WebGL implementation.

## Exact 8 B to 8 KiB sweep

`SWEEP-REPORT.md` (generated locally) contains the 11 byte-doubling levels and
five languages, including original WebGPU median/p95 and a same-input syntect
baseline. `make-sweep.py` derives exact-size UTF-8 prefixes from the saved source
snapshots. The browser runner accepts `LEXER_MANIFEST`, `LEXER_RESULT_PREFIX`,
`LEXER_REPS` and `LEXER_CONFIRM_REPS`; the native binary accepts `LEXER_MANIFEST`
and `LEXER_REPS`, and its runner accepts `LEXER_RESULTS_DIR`. These settings keep
sweep data separate from previous measurements. See the report for exact commands.

## Three-engine decision report

`THREE-WAY-REPORT.md` (generated locally) compares syntect, the Rust GPU port,
and unchanged browser WebGPU for every exact byte size, with per-language
median/p95 and a four-language equal-weight summary. Generate it with
`python3 benchmarks/gpu-lexer/report-three-way.py`.

## Optimized native backend

Select `LEXER_BACKEND=optimized` for the Rust/Metal driver in `src/metal.rs`.
The existing default `wgpu` backend is retained unchanged as the comparison
baseline. The optimized runtime uses the original-model MSL in `assets/dawn/`,
GPU-private weights/scratch, shared input/output, geometric buffer capacity reuse,
and bounded shared-event waiting. It neither launches Chrome nor runs JavaScript.

After the documented release build:

```sh
LEXER_BACKEND=optimized uv run --script benchmarks/gpu-lexer/verify-optimized.py
python3 benchmarks/gpu-lexer/final-optimized-benchmark.py
python3 benchmarks/gpu-lexer/report-optimization.py
```

The final runner measures native/browser/native in that order, discarding and
retrying runs with detected compiler or renderer-benchmark competition.
`LEXER_GPU_ONLY=1` omits the already measured syntect timing; it never skips GPU
inference. All final native repetitions are 100; browser confirmation uses 30.

`LEXER_BACKEND=tint` retains the earlier Metal comparison defaults (shared
storage and callback completion). `LEXER_METAL_PRIVATE=weights|all` and
`LEXER_METAL_WAIT=callback|direct|event` support the recorded A/B experiments.
The optimized default is private weights/scratch plus event waiting.

Only macOS/Metal on Apple M4 is validated. Metal source generation requires the
pinned Chrome version and a reviewed shader ABI; ordinary builds and execution
use the embedded sources without needing a browser. Performance excludes GUI
layout/rendering and output-cache hits. There are no production UI changes.

Failed/unused shader-layout, kernel-fusion and compiler experiments are preserved
in `experiments/`, along with their source transformations and diagnostic output.
They are not compiled into the final optimized path. The temporary wgpu-hal
vendor patch was removed from Cargo resolution; its diff remains for reference.

## Shiki comparison

`SHIKI-REPORT.md` (generated locally) adds Shiki 4.4.3 Oniguruma/WASM and JavaScript
RegExp engines to the exact 8 B–8 KiB sweep. It uses a reused core highlighter,
five languages, github-light, unlimited tokenization, and codeToTokens plus flat
span conversion. Both engines run in dedicated workers, twice (100/30 samples).
Dependencies and a frozen pnpm 10.33.0 lockfile are isolated in `shiki/`.
The report includes reproduction commands; generated browser bundles are retained.
