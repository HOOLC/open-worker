# Fixed-shader bounds and initialization audit

This applies only to the pinned model and the derived `validated.wgsl`, not to
arbitrary WGSL. The only shader data supplied by source text are two masked
feature words per token. The host generates all dispatch, stream and chunk metadata.

Host invariants: token count > 0, two words/token, count <= adapter's maximum
storage binding / 128. One stream, chunk counts <=32, power-of-two leaf capacity,
32 embedding channels, 41321 weights. Dispatch dimensions and metadata use these
counts; all dimensions and loop counters fit u32. Buffers are allocated for the
full tensor shapes and output is rounded to a multiple of four bytes.

- a/b: invocation guards protect chunk/stream reads. Chunk source ranges and
  neighbor summaries come only from host metadata. Forward/backward loops stay
  within the stream's chunk range.
- c: 32 lanes. Neighbor searches stay in each source chunk. `gb` checks stream
  token bounds (including the 0xffffffff neighbor sentinel) before loading M.
  Weight indexes are masked features and fixed 32-channel offsets. The maximum
  embedding row is 748, ending at weight 23967. j indexes are token<32 x channel<32.
  ib is tokens x32; recurrent summaries l are chunks x32 x4.
- d: 256 lanes, eight 32-lane scans per group, four groups per stream. Only Ab<chunk
  count permits l access; an underflowed reverse index on an inactive lane is never
  dereferenced. Lane-relative j reads are guarded by wb>=ga or wb>0. j indexes <=255.
- e: all j indexes remain in a 32x32 tile. XOR partner channels use shifts 0..4.
  ua indexes stay within each chunk's source range. r leaf index is tree base +
  leaf capacity -1 + chunk index, below 2\*leaf capacity -1 nodes.
- f: padding initializes unused r leaves. Every parent read is guarded by the
  current active count. Channels use xor shifts capped at 4. Tree down-pass writes
  only active children. l capacity is max(scan floats, tree nodes x32).
- g: m is a full 63-node x32 local tree (2016 floats). Each level is bounded by
  fixed capacity 32 and the actual chunk count. Head workspace: 8 lanes of tokens
  x16 gates, x72 hidden, x9 classes. qa only dereferences source/ua and writes Fd
  when Sb (or bb, which implies Sb). **The upstream select did not guard its M
  load**; validated.wgsl replaces it with an if before disabling injected clamps.
- Weight ranges follow the upstream tensor offsets and fixed channel/hidden/class
  loops; the maximum used index is 41320. No feature controls an unmasked index.

Loop bounding: loops are finite static feature/channel loops, bounded host counts,
or halving/doubling a positive bounded power of two. No code-provided feature can
extend a loop. GPU memory limits bound the stream count far below u32 overflow.

Shared-memory initialization: c initializes x and neighbor y/z before use, then
all active y/z gates. d initializes each active scan lane before every scan.
e initializes x, y/z, then w and updated z before mixing and reducing the tree.
g initializes active leaves, then the complete lower tree, before top-down use;
head shared values are written by all participating lanes before each barrier.
Unused vector components do not feed an observable calculation. Buffer and
workgroup barriers remain intact. Label output is cleared before atomic packing.

Verification must include partial/full tiles, transitions around powers of two,
Unicode, and changing input sizes, comparing exact labels against upstream.
Disabling runtime checks is not a substitute for these invariants. Other derived
shaders must receive their own audit before using this compilation mode.
