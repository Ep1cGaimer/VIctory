# VIctory world at 30 fps on `victory-world-30fps-v2`

Last updated 2026-11-25. The v2 findings below apply to the RTX 4060 Laptop. The ss9–ss12 runs document the earlier branch.

## Goal and ground rules

- **Goal:** the title (`C:\Games\GTA6.Xbox.v1.0.2.1-CYBERLEEK`) running in the world with clean, smooth graphics at a locked 30 fps on this machine.
- **Never write to the game folder.** Reading it is fine. The owner wants to reuse it for other ports. After runs, check that nothing changed. Fingerprint (file count, total bytes, newest write time):
  `109 224001114630 2026-11-24T09:04:57.8026955Z`.
- **Trusted code only.** Take code only from `main` or the project's own branches of upstream VIctory (`Marvisak/VIctory`), or from popular working repos. Code merged into the main branch of a working fork is fine too (e.g. `Zettrox/ztrx`, GPL-3.0-or-later). **Never** take code from unmerged PRs; write it yourself instead (owner's clarification, 2026-11-25). XCL (`ivall/xcl`) is GPL-2.0 and VIctory is GPL-3.0: take ideas from XCL, not code.
- **Remotes.** The v2 branch tracks `Ep1cGaimer/VIctory:victory-world-30fps-v2`. The upstream project is `Marvisak/VIctory`.
- **Commits** end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Machine

- RTX 4060 Laptop plus an AMD iGPU (hybrid). The host device is made on the high-performance adapter (commit 4aa2f9f). Check the "host adapter" log line.
- The RTX reports 32 lanes only. The AMD iGPU reports 32–64 and ran a native wave64 dispatch in isolation.
- The NVIDIA view heap is capped at 1,000,000 slots.
- 16 GB RAM, so use `-j 4` for cargo. During game runs, free commit memory drops to about 400 MB.

## Build, test, translate

```sh
cargo build --release -p victory -j 4                                   # target/release/victory.exe
cargo build --profile diag -p victory --features diagnostics -j 4       # target/diag/victory.exe (DRED breadcrumbs, op notes)
cargo test -p victory-shader --release -j 4                             # 135 pass, 1 ignored
cargo clippy -p victory-shader -p victory -p victory-gpu --all-targets -j 4 -- -D warnings
```

Offline shader translation of the whole game. It reads `shaders.rpf` and writes nothing to the game folder.

```sh
victory.exe translate C:/Games/GTA6.Xbox.v1.0.2.1-CYBERLEEK/shaders.rpf             # ~80 s, refusal reasons grouped by count
victory.exe translate ... --validate                                                # ~3-4 min, adds dxil.dll validation
victory.exe translate ... --list --validate                                         # every program: name, [runtime hash], stage, outcome
victory.exe translate ... --pipelines                                               # also creates host pipelines
VICTORY_ONLY=<name substring> victory.exe translate ...                             # only matching programs
VICTORY_DISASM=1 / VICTORY_EMIT_DUMP=<dir>                                          # GFX10 disassembly / write the DXIL files
```

- The `[hash]` printed by `--list` is the runtime shader hash: the first 16 hex digits of the BLAKE3 of the container. It is the hash the game logs use (`[mesh 0d9d1c59733d6340, pixel ...]`). To map hashes from a log to shader names, grep a saved `--list --validate` output.
- To disassemble DXIL, use `"C:/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64/dxc.exe" -dumpbin file.dxil`.
- Diff tests against the console compiler need `VICTORY_CONSOLE_DXC`, which this machine doesn't have, so they are skipped here.
- `emit/host.rs` (the GPU unit tests) uses the default adapter.

### Translator counts (validated programs)

| | Session start | Now (8ec9285) |
|---|---|---|
| Vertex + pixel (of 14027) | 12155 | 12782 (12865 emitted) |
| Compute (of 2582) | 1417 | 1678 (2212 emitted) |
| Mesh (of 8713) | 8152 | 8154 (8216 emitted) |

Most of the gap between emitted and validated compute programs is compute shaders that sample with implicit derivatives ("operation incompatible with the shader stage"). The "derivs" agent below is on it.

## Running the game

**Known-good setup (2026-11-26, a61e139):**
- Use the diagnostics build (`cargo build --profile diag -p victory --features diagnostics -j 4`) with `VICTORY_SHADER_LOOP_LIMIT=256`.
- With these it ran 30 minutes in the world with no crash (run AL). The memory defaults since 6edc329 and the pool fix since 96e86dd need no switches.
- Only diagnostics builds take the harness's scripted keys (`VICTORY_KEYS`). A scripted release run stays at the start screen.

Use the harness in `tools/lab/`: `run-probe.ps1` is the launcher and `gpumem.ps1` is an optional GPU and commit memory monitor.

- Copy `run-probe.ps1` into a lab folder; it was `%TEMP%\victory-30fps-lab`, with output under `output\<run>`. It sets these variables:
  - game and data: `VICTORY_GAME_DIR`, and per-run `VICTORY_DATA_DIR` and `VICTORY_CACHE_DIR` (so nothing is written to the game folder)
  - frame captures: `VICTORY_CAPTURE` and `VICTORY_CAPTURE_FRAMES`
  - logging: `VICTORY_LOG`
  - crash data: `VICTORY_D3D12_DRED=1`
- It seeds the save profile from `%TEMP%\victory-readonly-run-20261124\hardware\data\7952987A` (storage and saves). If that folder is gone, the title starts with a fresh profile.
- Typical world run:

  ```powershell
  powershell -File run-probe.ps1 -Exe <victory.exe> -RunDir <lab>\output\ssN -Seconds 590 -Console series-s -Keys <keys>
  ```

  - Use Series S mode (`--console series-s`).
  - The game takes about 3 minutes to reach the menu, and boot time varies, so send Enter about every 10 s from 15 s to 585 s. That gets through the menus and into the world.
  - Use the diagnostics build for these runs: it is the one that records breadcrumbs and names commands in crash reports.
  - Optional switches: `-Validation` turns on the D3D12 debug layer and GPU-based validation (very slow); `-Warp` uses the software renderer.
- The shader cache is per run (`<run>\cache`). Copy an earlier run's cache in to warm a run. Since 4962cdb the cache key is a hash of the translator's sources (`crates/victory-gpu/build.rs`), so a rebuild that doesn't touch the translator keeps the cache.
- Read `<run>\stderr.log`:
  - Lines with `fps, ` are per-second frame stats: preparation and copies, GPU time, draws, and pipelines dropped or still being made.
  - `copystat` lines break down the copy cost.
  - `slow host submission ms=...` lines name the pipelines in a slow submission.
  - `device removed after` is the DRED crash report: which list stopped at which command, and the commands in flight between the surrounding barriers, with their shader hashes.

## What this branch did (newest first)

- **caefb47** The pipeline library (1.18 GB) is serialised only when pipelines were added: once 64 are waiting, after 5 minutes, at exit, or when the prebuilt list changed. Before, it was written again every 20–30 s. The early list is written by itself.
  - In a 13-minute run, the library was written once, the lowest RAM available was 114 MB instead of 1 MB, and there was no crash.
- **171de15 / 96e86dd / 0c590d3 (reliable past 6.5 minutes)**
  - Every run that reached about 6.5 minutes in the world used to die there.
  - **The title's job slot pool ran dry** (an `int3` at `+0x5149aec`). Its slots in use climb from about 8,500 to 20,000–24,000, then fall back.
    - 96e86dd gives the pool 65,536 slots at load, in memory only. `VICTORY_TITLE_POOL_SLOTS` sets the count; 0 keeps the title's own size.
    - The diagnostics probe `VICTORY_PROBE_POOL=a9ec938` logs the pool every 10 s.
  - **The driver's internal error removed the device** (`0x887A0020`). It happened only with the pipeline library mapped from its file (b6fd495), in 4 of 5 runs. 171de15 reverts that.
  - With both fixes, a 13-minute run reached frame 3,000 with no crash.
  - See `docs/memory-footprint-2026-11-26.md`, step 4.
- **6edc329** The memory settings are now the defaults: 64 KiB loose windows given up after 20 s idle, read-only copies in video memory, and a 512 MiB snapshot pool. A 12-minute run and a 10-minute run with them reached the world without a crash. The world runs at 600–700 ms a frame, about 270–370 ms preparing and about 275 ms on the GPU. The stalls are paging episodes.
- **3be1ffe / bab0189 (host copies of title memory)** Loose windows are 64 KiB, given up when idle, and let go when a larger window covers them. Buffer lookups are bounded to their range. With `VICTORY_COPIES_IN_VIDEO=1` (opt-in), read-only copies go in the GPU upload heap. A copy that shaders write is made again in system memory, and stale title slots are made again. Card system memory is down from 3.3–4.7 GB to 2.1–2.7 GB, and copying back is about 20 ms a frame. RAM still runs out at the peak of the load. See `docs/memory-footprint-2026-11-26.md`, step 3. These are the defaults since 6edc329. Runs here use `VICTORY_SHADER_LOOP_LIMIT=256`.
- **a14fee5 + census** Host textures can be let go when idle (`VICTORY_TEXTURE_IDLE_S`, opt-in). A census by holder and the card's memory budget are logged once a second. Result: 4.2 of 4.9 GB are targets with distinct title memory, so there is nothing to let go while loading. The watchdog crash after the first 2.7 s submission is the blocker before the world. See `docs/memory-footprint-2026-11-26.md`, step 2.
- **e5ee3ce** Compute and mesh shaders can emit implicit texture derivatives with validator 1.8. Compute groups use a flat host layout so the original console quads stay intact. The full archive validates **2,212 compute programs** (+534), with vertex/pixel and mesh counts unchanged. This measures DXIL validation. The host can still refuse a compute pipeline that requires a 64-lane wave.
- **8f7f5c3** Decode all three packed mesh vertex indices as nine bits. The adjacent edge flag is independent of each index.
- **8ec9285** Mesh shaders keep exports under the declared maxima and cull primitives with indices past `max_vertices`. Xbox clips out-of-range exports. The host leaves them undefined.
- **b0a4848 / 29dcbd5** Descriptor dwords read as data (for example `getDimensions`) are lowered by finding the view kind through other loads of the same descriptor. +238 validated, mostly `CS_RaytracingResolve` and water.
- **2d8e83e** SGPR spills forwarded through VGPR lanes (`v_writelane`/`v_readlane`). +111 validated. Also adds `VICTORY_ONLY` and `VICTORY_DISASM` to `translate`.
- **60b061b** 64-bit image atomics lowered to SM 6.6 typed 64-bit atomics. +518 pixel programs.
- **4962cdb** Shader cache keyed by the translator's sources, not the executable. `translate --list` prints runtime hashes.
- **3a17359** No indirect draws in a frame where a compute pass was dropped (or in the next frame). The dropped compute leaves garbage arguments, and the result was 25 s GPU hangs. `VICTORY_INDIRECT_UNGATED=1` turns this off. A diagnostics build also has `VICTORY_SKIP_INDIRECT=0,1` to drop indirect kinds.
- **f8c134d** One loop budget per lane for the whole program: 2^16 iterations in total over all loops, held in a private global `loop.budget`. Nested data-driven loops used to multiply their per-loop limits; hair strand, pre-skin and ambient-map compute shaders hung the GPU.
- **22cb245** and earlier: Series S plane compositing; full dynamic descriptor slots on the capped view heap; host device on the high-performance adapter.

## v2 branch on this laptop (`victory-world-30fps-v2`, 2026-11-25)

The v2 branch was rebased on upstream main `940d32f`. At the time of inspection the checkout was at `0bac672` with the host heap limit fix and diagnostics in the working tree. The previous test run recorded 154 passing `victory-gpu` tests, or 157 with `--features diagnostics`.

- **Boot crash, fixed (80a6d5c).** v2 crashed 3 s in with a null write in title code at `+0x2215c35`. That is the title's own watchdog: it writes to address 0 when a thread stops checking in. The cause was our "title slot past the host heap" warning, logged once per slot as the title walks its 1,245,184-slot heap. NVIDIA caps the host heap at 1,000,000, and the title side makes 737,856 slots usable. That was 280,000 lines in 32 s. The warning now logs once per 65,536 slots, and the boot reaches loading. Menus hold 30.0 fps.
- **Depth-plane copies are skipped (80a6d5c).** The title copies the depth of an `R32G8X24` texture into an `R32` one, which is an invalid host copy. The device was removed 11 ms after it. Without a trace, NVIDIA's driver faulted inside `CopyTextureRegion` at the same point. This is unconfirmed as the cause, because those runs also ran out of memory there; the "skipping GPU copy" warning has not fired yet. `VICTORY_LOG=...,victory_gpu::copytrace=trace` logs every texture copy.
- **Loading memory, fixed (664a0f4).** Host textures of title textures that are only read are now made on first use. Textures the GPU writes are still made at creation, so placed views of their memory can share them. `VICTORY_EAGER_TEXTURES=1` brings back the old behaviour. On Series X, host textures stay at 4.9 GB (about 1,700) through loading, down from 9.6 GB, and loading no longer fails with `E_OUTOFMEMORY`.
- **World performance.** Menus hold 30 fps. The reported world runs draw 1,000–6,000 draws per frame at 0.1–0.5 fps. CPU preparation takes 0.5–4.4 s. One frame spent 552 ms copying 862 MB, another 2,706 ms copying 2.27 GB. The reported GPU interval rose from about 145 ms at world entry to 1.35–2.30 s. In `host.rs` the submission metric spans execute through fence wait. Isolate individual commands before attributing that time.
- **Shadow-trace hang, fixed (2026-11-25, later).** There were two bounds bugs, both fixed and tested (`descriptor::host` test `dynamic_slots_have_view_addresses`).
  - **The heap size.** Translations now clamp heap indices below the host's real heap size (`victory_shader::emit::heap_views`; `descriptor::host::probe_view_heap` sets 1,000,000 on NVIDIA). The null buffer views sit in the top 3 slots, and the size is in the shader-cache key and the pipeline-library tag. This alone did not stop the hang.
  - **The view-address table.** `t1` in `MEMORY_SPACE` gives a BVH's base. It had entries only for title slots (737,856), while the shadow-trace table had been copied to dynamic slots (876,824 and up), so the shader read past a root buffer, which has no bounds check. Acceleration-structure SRVs (dimension 11, no resource) also recorded address 0. Now the table covers every slot a shader can index, dynamic slots record their address when made, dimension-11 views give their location (dwords 4–5), and the shader clamps the index.
  - **Result.** Run `v2-viewaddr-x2` ran `f607780a` unskipped and reached the world (frame 1862, 13,515 draws), then ran 3.5 minutes with no device removal. World frames were 3–25 s each (copies up to 13.7 s, copying back up to 3.5 s), and frames 3 and 20 into the world are near black.
  - **An earlier run crashed in the game itself.** Run `v2-viewaddr-x` crashed just before the world with a null write at `+0x23cf683` (the watchdog's site is `+0x2215c35`, and this stack has it as frame #1), right after a 2.7 s submission. It did not happen again on the rerun. ztrx's notes warn that once trace programs read real BVHs, a partly working BVH builder can corrupt title memory; the builder is now gated (below).
- **ztrx compared on this laptop; BVH builder gated (2026-11-25, evening).** ztrx `main` (`90b529f`) was built unchanged in a separate worktree (`labRoot/ztrx-src`, its own target dir) and run through `run-probe.ps1` (never its `play.ps1`).
  - **ztrx as-is.** Run 1 (cold cache) ended after about 2 min with `memory allocation of 1572864 bytes failed` (process commit 16.4 GB). Run 2 (warm cache, 10 min) did not crash, but frames 2100–2700 still show loading art and the credits. Its heavy loading frames (about 2,100 draws) ran at 0.5–1.9 fps with 190–540 ms on the GPU; ours spend 1,650–2,300 ms with the same kind of frame.
  - **The GPU gap is runaway loops.** `VICTORY_GPU_TIMING=1` on our build put `00167d58cc5a78d7` (`WindNew:CS_CopyLatestWindFieldForRenderingWithSmooth`, dispatch 16x16x16) at 1,126 ms average, up to 2.7 s, then `8bbcbda6008eaa37` (388 ms average), `c7feccae4e01e454` and `776d835da5ffa646`. `VICTORY_SKIP_PIPELINES=00167d58cc5a78d7` cut the median heavy-frame GPU time from about 1,740 to 874 ms. With `VICTORY_SHADER_LOOP_LIMIT=256` (cache `labRoot/cache-l256`, warmed by `translate --cache`), wind fell to 51 ms average, `8bbcbda6` to 43 ms, and the median to 432 ms, about ztrx's. Wind's six loops are the same in both translations (`VICTORY_SHADER_SAVE`, `dxc -dumpbin`); the one that runs away exits on `counter >= N` with N read at byte 284 of a 1,360-byte buffer through a heap index loaded from the table (`>> 5`). Which buffer that index reaches, and its last writer, is not traced yet. The working guess is a refused wave64 producer (ztrx runs wave64 programs as two host waves of 32 in `emit/lower.rs`).
  - **Not buffer placement.** ztrx keeps big buffer copies in the card's memory (`VICTORY_CARD_BUFFERS`, on by default); ours are `CUSTOM` write-back L0. With `VICTORY_CARD_BUFFERS=0` ztrx still spent 160–440 ms per frame on the GPU, and wind did not show among its slow commands.
  - **BVH builder gate (ported from ztrx main `pipeline.rs`).** Our build ran 595–1,190 builder dispatches a frame while some builder stages were refused (wave64), so later stages computed store offsets from data no one wrote, in plain title memory. ztrx's notes: with 14 of 35 stages running, the title died within a second. Builder pipelines (compute programs whose root parameters are in a spilled user-data table, from index 16) now run only with `VICTORY_BVH_BUILDER=1` and only if every one has a host pipeline. The 52 pipelines flagged in `v2-l256-timing` are exactly the 52 containers of `XboxBvhBuilder.dll` (BLAKE3 of each DXBC container in the DLL). With the gate, "copying back" is 0.0 MB a frame (it was 1.6–16 MB: the builder's stores). GPU time did not change.
  - **Shader dump.** `VICTORY_SHADER_SAVE=<dir>` writes each shader as given (`.cdxil`) and translated (`.dxil`) when first translated, cache hit or not; `VICTORY_SHADER_SAVE_ONLY=<hash,...>` limits it.
  - **AMD 780M run, inconclusive.** `v2-amd-timing` (`VICTORY_ADAPTER=AMD`, fresh cache `labRoot/cache-amd2`, 306 refused against about 166 more on NVIDIA's cache) sat on the account-picker screen (12 draws a frame) for all 8 minutes, as the earlier `v2-amd-x` did, with shared GPU memory pinned at 7.6–7.7 GB (the iGPU's limit, half of the 15.3 GB RAM). Wind never ran, so whether native wave64 cures the runaway loops is still untested.
  - **The slow shaders' effects have refused passes.** All four slowest (wind `00167d58` and scattering `CS_Gather_Classified_0/2/18`) are in effects with passes refused for wave64: `WindNew` passes 3–7 (`CS_PreProjection`, `CS_PreProjectionChild`, `CS_ProjectionIteration`, `CS_PostProjection`, `CS_FeedbackToParent4x4x4`) and eight `scattering_transmittance_system` passes (`CS_SortRayTiles`, `CS_RainMap`, `CS_DensityShadowMapMips`, `CS_CloudMultiScatteringCache`, `CS_GatherDiffusion`, `CS_GeneratePrecisionVolumeDistanceField`, `CS_GatherDensityAreaForReadBack`, `CS_SkyRadianceCache_Camera_Gather`). We refuse 138 compute pipelines in `v2-l256-timing` (`host pipeline refused`; 6 of them builder stages); ztrx refuses 5 (`CS_AlphTransmissionPrefilterMip`, `GpuParticleUpdate:CS_Visibility`, two `vfx_triangleClip`, `sr_tracing:CS_Tiling_Generate_Opaque`), none of them wind or scattering. Decision: port ztrx's wave64-in-two-host-waves lowering by hand, in tiers (no cross-lane code; LDS in groups over 32; cross-lane exchange).
  - **Wave64 as two host waves of 32, first part (ported, off by default: `VICTORY_WAVE_HALVES=1`).** From ztrx main: a wave64 compute program whose cross-lane code stays inside 32-lane halves runs with `WaveSize(32)` (thread ids from an LDS counter, `Lower::wave_base`), and LDS a console wave shares without barriers gets a group barrier before each access (`split_lds`, `lds_meetings`). Not ported: the LDS exchange for programs that cross halves. A census of the 132 refused (non-builder) programs: 8 need no crossing and no shared LDS, 38 need only the LDS barriers, 86 cross halves (62 with `v_mbcnt`, 24 with only `v_readlane`/`v_writelane`). With the switch, 28 more pipelines are made on the RTX, among them four wind producers (`CS_PreProjection`, `CS_PreProjectionChild`, `CS_ProjectionIteration`, `CS_PostProjection`) and `CS_RainMap` and `CS_GeneratePrecisionVolumeDistanceField`. The host wave width is in the shader cache key and the pipeline library tag.
  - **But the title stalls at its account picker with it** (`v2-w32-timing`: 13 draws a frame for 10 minutes; the earlier AMD run stalled the same way). With all 28 new pipelines skipped (`VICTORY_SKIP_PIPELINES`) it leaves the screen at 212 s as before. Which one holds it is not found yet: a bisect over the 28 was cut short (the owner pressed Enter during one step, which spoils that step's result). Resume it: halve the list, `VICTORY_SKIP_PIPELINES`, 270 s runs, check for a frame with over 100 draws.
  - **Belobug/VIctory-Belobug-AMD (read 2026-11-25).** A fork on an RX 9070 XT (16 GB, 64 GB RAM): 3–5 fps in the world. AMD runs wave64 natively, so nothing is refused and no loop runs away. Its own speed-up: the clamps of a run of indirect draws are recorded in a list before the run (`VICTORY_HOIST_CLAMPS`, 225 to 72–80 ms of GPU time a frame at 9,000 draws), big buffers in VRAM (`card.rs`), all views of a guarded physical page guarded together (vehicles lost parts), a constant ambient light in place of the missing ray-traced light, twice the dynamic descriptor slots. It commits 22–33 GB of RAM, more than this laptop has. Its `docs/wave64-on-wave32.md` is a design (an exchange in groupshared memory and a "console flow" lowering) that is not in upstream `main`; it may come from an upstream pull request, so read it for ideas only.
  - **fxceless/victory-fork on this laptop (2026-11-26, `d510ee4`, release build, `translate --warm all`, switches `VICTORY_JOIN_PARTS=1 VICTORY_BACK_OWN_VIEW=1 VICTORY_INDIRECT_GATHER=1 VICTORY_CARD_REHASH_RUNS=1`).** That fork reports 7–12 fps in the prologue's first rooms on an RTX 4070 12 GB with 32 GB RAM. Here: menus at 30 fps, world from about 260 s (1,000–2,400 draws), **0.2–1.9 fps**, then the title hit a breakpoint (`0x80000003` in the title image) at about 8.5 min. Per frame: copies of title memory 0.05–2.5 s (80–270 MB), "waiting for title memory to hold still" 756 ms in texture refreshes, the GPU about 10% busy, 750–1,000 pipelines still "being made" all run (its warm-up translates but does not create pipelines). Memory: commit 23 GB on 15.3 GB RAM with the working set down to 1.5 GB, so the process pages. **On this laptop RAM, not the GPU, is the wall for every fork tried (ours, Belobug, fxceless).** Output: `labRoot/output/fxceless-1`.
  - **Merging ztrx is not cheap.** A trial `git merge --no-commit ztrx/main` in a throwaway worktree conflicts in 44 files: 68 hunks in `emit/lower.rs`, 38 in `draw.rs`, 37 in `host.rs`, 17 in `shadow.rs`, 10 in `structure.rs`. Since `ffe310e` our shader crate has 96 commits (most of them upstream merges of others' PRs, which a swap to ztrx's crate would drop); ztrx's has 10, and its wave64 lowering is inside the squashed `578d459` (+4,350 lines in `lower.rs`) and `6a2d9f9` (`wave.rs`, `wave64_tests.rs`, `own_bits.rs`), with `e9765a7` fixing a device loss it caused. Porting it means taking the two-host-wave lowering by hand, not a cherry-pick.
  - **Note for runs:** "the world's first frame" is logged only with `VICTORY_CAPTURE_WORLD` set, and it is only the first frame with 500 draws; the loading screen draws the world behind it.
- **Memory growth.** With the shadow-trace pipeline skipped, shared GPU memory rose from 0.8 to 5.9 GB and commit from 7.5 to 23.7 GB over roughly three minutes. Dedicated usage stayed around 5.2–5.5 GB. `shadow.rs` keeps a strong reference to every loose 4 MiB-aligned window. An earlier run reached 4.5 GB of loose copies in about 1,000 windows.
- **Indirect work and PSOs.** The reported runs dropped 450–8,655 indirect commands per frame with 500–1,300 pipelines pending, even with a 27,160-pipeline prebuild. The indirect draw gate can close for a whole frame and the next after one missed compute pass. Count gate closures, missing PSOs and invalid input versions separately. Comparing normalized PSO state on cache misses will show how many are genuinely new variants.
- **Old texture blocker, for reference.** On Series X, before 664a0f4, v2 filled 7.6 GB dedicated and 7.2 GB shared; then a host command list failed to close with `E_OUTOFMEMORY`. At failure (d626e68 counters), textures were 9.0 GB made, of which lists had used 2.0 GB. The old branch binary used 4.5/0.8 GB dedicated/shared at the menus, against v2's 5.2/2.1 GB.

### Shadow-trace device removal

In `v2-heapfix-x`, DRED stopped at command 53 of 64 after a 24.6 s submission. The emulator prints a half-open range. In `52..54`, 52 was the marker and 53 was the indirect dispatch for `f607780aa8e502d0`. Earlier work can overlap that marker. An earlier run also stopped at 53 with a loop budget of 256. The `[0,1,1]` and `[1,1,1]` records in `v2-probe3-x` were sampled after lists that skipped this pipeline. The skip returns before the argument clamp. The exact post-clamp arguments for the failing invocation still need capturing. [DRED breadcrumb caveats](https://learn.microsoft.com/en-us/windows/win32/direct3d12/use-dred)

**The first address lookup has a bounds defect.** The cached shader for `CS_IrradianceCache_ShadowTrace_WithFar` was verified against the console container hash and the `cache-heapfix` key (1,000,000 views, loop limit 65,536). Its DXIL clamps ordinary heap handles at 999,999. It also reads the two acceleration-structure bases from a separate raw root SRV, `t1,space2`, before using `t0,space2` for BVH node memory. The reads use table 1 entries 3 and 4, eight bytes per entry. D3D12 root descriptors carry no length. [Root descriptor bounds](https://learn.microsoft.com/en-us/windows/win32/direct3d12/root-signatures-overview)

- The breadcrumb's `u5=0x1ac2300` is the original guest table byte offset, or slot **876,824**. `draw::State::tables` copies this eight-slot table into the host dynamic range, which starts at **737,856**. `State::bind` passes the rebased offset to the shader. `op_text` and `slot unknown` describe the original guest location.
- `descriptor/host.rs::HostHeap::view_addresses` allocates **737,856 entries** in this heap. Every dynamic index is outside that root buffer. `HostHeap::make_at` updates strides and write marks for dynamic slots, but omits the address entry. Even for a static slot, `write_address` derives its value from `buffer_range`, which returned `None` for both dimension-11 views in the trace. Their descriptor words encode guest VAs `0xd6016a200` and `0xd5f1d0600` in this run.
- A lane reaching either load therefore reads outside `t1`. This is a specific candidate for `DEVICE_HUNG`. The capture lacks a faulting instruction PC or a rerun with corrected metadata. The DXIL has two budget updates, 512 dwords of LDS, and zero DXIL barrier calls. The shader-visible heap fix and the separate root-buffer bounds issue can both be verified in the same cached translation.
- To inspect the same bytes, the entry under `%TEMP%\victory-30fps-lab\cache-heapfix\shaders` is `f0181ffb73463e27/2c/d0940523efe63ada53b95bab077f76bd2070d37c9560a6c569ad29c2a15f09`. The 42,052-byte DXBC container starts at offset 12 of the `VSH1` file. Its SHA-256 is `1df1f1c4f68100e470004771c6a1e091bf7d296f6440eff87100dcc37196526a`.

### Wind, scattering and missing producers

- In both `v2-probe-x` samples of `WindNew:CS_CopyLatestWindFieldForRenderingWithSmooth` (`00167d58cc5a78d7`), the counts at byte offsets 0 and 284 in the 1,360-byte input were **2 and 2**, in both the guest and its host copy. The other loop is fixed at three passes. The earlier 2.5 s measurement used adjacent bottom-of-pipe timestamps. Wind's exclusive duration and iteration count on the GPU were not measured. The same attribution issue applies to `CS_Gather_Classified_{2,18,0,16}`. Two of those indirect dispatches had 1,824 and 2,072 groups. [D3D12 timestamp semantics](https://learn.microsoft.com/en-us/windows/win32/direct3d12/timing)
- **Wave64 producer loss is independently observed.** Across two world frames, 166 compute pipelines were refused with `0x80070057` and 118 distinct shaders dropped at dispatch. The checked refusals requested wave64 on the 32-lane RTX. Examples include wind projection, shader binning, depth pyramid, instance culling, GPU particles, alpha resolve, `ptx_prerender` and cloud multiscattering.

  Two frequent drops, `caa78b8ca62bc3d6` (×438) and `28918b7150a7aad5` (×219), have no match in the archive. Identify the last writer of each consumed buffer range, including aliases and frame version, before attributing wind, indirect arguments or the BVH to a specific dropped producer.
- **Archive census.** 394 of 2,622 emitted compute shaders declare `WaveSize=64` under the console-lane or one-wave LDS rules in `Lower::new`. The earlier scan counted 54 with only LDS and 340 with wave operations. Among the latter, 97 use `waveReadLaneAt`, 88 active reductions, 37 prefix operations and 18 bitwise reductions. All 340 had `waveReadLaneFirst`, which the translator can also introduce when reconstructing thread IDs.
- **The AMD iGPU is a local wave64 reference.** An explicit-adapter D3D12 test dispatched one group and checked eight dwords of readback. At width 64, the high-half ballot was `0xffffffff00000000`, broadcasting lane 63 returned 63, summing 1 through 64 returned 2,080, the last upper-half prefix count was 31 and the first active upper lane was 32. Both adapters also passed the width-32 control. Using VIctory's translator and root signatures, the AMD device created all eight sampled game PSOs. The RTX created the width-agnostic `Prop_BatchCull_ImposterOnly` control and refused the seven explicit wave64 PSOs (four prop-culling variants, both shader-binning passes and `ProjectionIteration`). This experiment executed the synthetic kernel and tested pipeline creation for the game shaders. Comparing producer output requires captured inputs.
- **First port candidate.** `CS_GenerateShaderBins_Tiled` has `numthreads(8,4,1)`, only 32 guest invocations, 128 LDS dwords and four `waveReadLaneFirst` calls. Check lane numbering, EXEC and LDS ordering, then compare output byte for byte with the AMD on identical inputs. `CS_GenerateShaderBinOffsets` is a deliberate second test with 64 invocations and real ballots, prefix, reduction and indexed reads. `WindNew:CS_ProjectionIteration` has 64 invocations and LDS with no explicit DXIL wave or barrier calls. Its cross-half LDS dependencies need an ordering proof.
- **Sampler:** `powershell -File gpumem.ps1 -Name victory -Seconds N -Out file` reads the per-process "GPU Process Memory" counters every 2 s. Its source is in the session scratchpad; it is a small `Get-Counter` loop over `InstanceName -like pid_<pid>_*`.
- **Heap ordering.** An earlier debug-layer run reported a directly indexed sampler root signature before its heap. The main bind path already calls `bind_heaps` first and invalidates both cached roots on heap changes. Trace helper dispatches, root restoration and the actual sampler heap on the offending list. [SM 6.6 binding order](https://microsoft.github.io/DirectX-Specs/d3d/HLSL_SM_6_6_DynamicResources.html)
- **Pacing (3cc82a8):** a frame's pipeline waits share one 500 ms deadline, where each thread used to get its own. A frame wait that comes after a tick it missed ends at once.

## Current state in game (old branch)

- The world loads and draws: 700–3400 draws per frame, at about **0.5–1.4 fps**.
- **Cost per frame (ss9):**
  - preparing 160–590 ms, of which copies (shadow refresh of title memory) are 100–660 ms and 22–155 MB
  - GPU 140–240 ms
  - 700–880 pipelines still being made (cold cache)
  - 350–4000 draws per frame dropped without a host pipeline
- **Hang (ss9, before 8ec9285):** `DXGI_ERROR_DEVICE_HUNG` (0x887A0006) about 1.5 minutes into the world.
  - The submission took 24.5 s.
  - DRED stopped in the G-buffer list, at command 645 of 745, among direct mesh draws: `MS_VS_Transform*` / `MS_VS_TransformD*` with `PS_DeferredTextured*`.
  - None of these shaders has a loop, and DRED recorded no page fault.

### Continuation runs

- `git fetch` verified official `Marvisak/VIctory` `main` at `ffe310e`. It was already an ancestor of this branch.
- **ss10, 8f7f5c3:** The RTX 4060 still returned `DXGI_ERROR_DEVICE_HUNG` after a 25.2 s submission. DRED stopped on the first incomplete list at command 349 of 576, a direct mesh dispatch with mesh `857b2e13c9cc107f` (`MS_VS_TransformD_c5fd36bb_Wrapped`) and pixel `7eddef6876e35361` (`PS_DeferredTextured_c5fd36bb_Wrapped`). The same mesh shader appeared near the stop in ss9. The last ss10 world windows ran at 0.3–0.9 fps, with 232–1,411 ms/frame reported for copies.
- **ss11-skip-transformd, 8f7f5c3:** `VICTORY_SKIP_PIPELINES=857b2e13c9cc107f`. Cold cache, Series S, restricted account, 340 s total. The world reached 0.8–1.4 fps for roughly 35 s before the test was stopped. The captured log has no device removal. A longer run is needed to establish whether skipping this shader affects the hang.
- **ss12-mesh-cull, e5ee3ce plus the first culling patch:** All pipelines enabled. The world drew for about 3 min 40 s, usually 0.2–1.4 fps, before `DXGI_ERROR_DEVICE_HUNG` following a 25.0 s submission. DRED stopped at command 102 of 161 on `e135eaf39576263a` (`MS_VS_TransformAlphaPrepass_eb34a03f_Wrapped`) with `8ba01719b71d1d35` (`PS_AlphaPrepass_eb34a03f_Wrapped`). The original package fingerprint matched after exit. The first culling patch added an output scalar and pushed 13 previously valid mesh shaders past the 32 KiB output budget.
- The restricted runs used the local lab launcher `output/continuation-probe.py`. It starts VIctory with a per-run profile and an output-only writable sandbox, and compares the package fingerprint before and after. Windows PowerShell's script-file launch failed in that restricted profile with a language-mode error. The original package matched the saved fingerprint after ss11.
- In ss10 the translated shader could index beyond the physical heap or into the dynamic range using a guest texture index. The ss10 crash log did not record that index. The v2 working tree bounds physical heap handles. Guest-to-host remapping and the shadow-trace address buffer remain separate issues.

## Work in flight (local branches and worktrees, not pushed)

| Branch / worktree | What | State |
|---|---|---|
| `scalar-ported` (`.claude/worktrees/agent-a572ecb7c40e3e864`) | Scalar buffer record counts and descriptor bits at joins. | Review found two correctness issues. The join replaces reachable descriptor data with zero without a proof that the value goes unread. The count query uses a structured view while the scalar load uses raw byte addressing against the same heap slot. Keep the worktree as reference. Prove an exact static structured SRV slot and use element and byte-within-element addressing before enabling it for live scalar loads. |
| `worktree-agent-af09ffdd7f771d608` | Implicit compute derivatives. | Integrated as `e5ee3ce`. The old worktree contains the unfinished session's copy. |
| `worktree-agent-a12b955eeb456e3c0` | Wave64 on Wave32 hardware. | Review found the draft can race across split waves in barrierless LDS code and its uniformity test overlooks per-lane values produced by waterfall lowering. The conservative LDS-only fallback to a 32-lane wave covers three shaders in a local scan. Refer to `C:/Vivek_projects/Leonida/docs/design.md` for the Vulkan implementation's ordering rules. |

Merge one branch at a time. After each merge, rerun the shader tests, clippy and `translate --validate`, compare the counts with the table above, then push.

- Old worktrees from merged agents can be removed: `agent-a9ce295e7b6be8f67`, `agent-af1be18f616254684` and `agent-af756a61df2a95748` (`git worktree remove`).
- `%TEMP%\victory-30fps-lab\shader-integration` is a stale detached worktree with old conflicts; ignore or remove it.

## Next steps on v2

0. **(2026-11-26, after the reliability work) The GPU is now the fps limit.**
   - Run AL at 1.4 fps: 699–709 ms a frame. 243–248 ms of it is on the GPU, and about 361 ms is preparing:
     - copies 106 ms
     - heaps 82 ms
     - the guard handler 65–69 ms
     - copying back about 45 ms
   - Sampled GPU time (`VICTORY_GPU_TIMING=1`, run `fps-P-gputiming`):

     | Command | Per call |
     |---|---|
     | indirect `c7feccae4e01e454` | about 29 ms |
     | indirect `8bbcbda6008eaa37` | about 24 ms |
     | mesh `f5257487f2945fdb` | about 46 ms |

     These are the runaway-loop shaders above. Their loop counts come from buffers whose producers are refused.
   - Dropped a frame (AL):
     - about 4,200 by the BVH builder gate;
     - 526, 348 and 293 indirect commands of kinds 0, 1 and 2 after a dropped compute pass;
     - 170 distinct refused compute programs, all `E_INVALIDARG`, led by `0709e8356abd9082` and `e4576c6eca0f8ce3` (143 a frame each), then `c9da71da40af2e28`, `2561af9577c47289`, `b22a16a178976e1b`, `660814153af43185` and `f8f50ab85e7b10bb` (54–62).
   - These are wave64 programs whose cross-lane code crosses 32-lane halves (`v_mbcnt`, `v_readlane`/`v_writelane`, `ds_(b)permute`, masks read as data). `lower.rs` ported only ztrx's first part.
   - ztrx main (`labRoot/ztrx-src`, `remotes/ztrx/main`, `emit/lower.rs` `exchange`, around lines 242–1515) has the second part: two host waves exchange through a pair of LDS dwords per console wave, with lane-uniform and own-bit analyses.
   - Porting it is the next step. It should restore the producers, end the runaway loops and the dropped indirect commands, and let the BVH builder run.
   - **Check before porting (2026-11-26, from the earlier ztrx runs on this laptop, `labRoot/output/ztrx-*`).** ztrx refuses only 3–5 programs, and none of our top refused hashes appear in its drop lists. But its world frames are no faster on the GPU:

     | Run | Frames past 500 draws | Median ms a frame | Preparing | GPU | Dropped |
     |---|---|---|---|---|---|
     | ztrx-run2 | 70 | 956 | 443 | 221 | 1,052 |
     | ztrx-cardsoff | 54 | 1,389 | 705 | 389 | 561 |
     | ours, AL | (30 min) | about 700 | 361 | 245 | about 6,200 |

     So the exchange would mostly cure dropped draws (a rendering fix). It is worth tens of milliseconds of GPU time, not hundreds. Its risk to reliability (ztrx's `e9765a7` fixed a device loss it caused) argues for an offline-first port behind an off-by-default switch, gated by a 30-minute run.
   - **Preparing and the GPU run one after the other.** `host.rs` around line 1311: every submission is `ExecuteCommandLists`, `Signal` and `WaitForSingleObject(INFINITE)`, before the copy back. At about 700 ms a frame:
     - with no GPU time at all, about 2.2 fps;
     - with no preparing at all, about 3 fps;
     - overlapping the two, at most about 1.7–2 fps.

     30 fps (33 ms) needs both cut by more than 10x. No fork tried on this laptop has passed about 2 fps in the world. fxceless reports 7–12 fps on an RTX 4070 with 32 GB of RAM.

1. **Correct and isolate the shadow-trace binding.** Size `t1,space2` for every physical slot the shader may read. On this heap the extra storage is 2 MiB. Populate dynamic entries even if they predate the lazy creation of the address buffer. For dimension 11, publish the descriptor's 64-bit guest location. Clear stale entries on reuse and bounds-check the index against the metadata allocation before issuing a root load. Capture the actual rebased table, both resulting VAs and the post-clamp argument record immediately before the enabled dispatch.

   - Drain prior work, run the target alone and wait for its fence before submitting successors. Save DXIL, root signature/constants, descriptor types and generations, resource contents and states, page table and BVH pages at that boundary. Host GPU VAs must be rebound to replay resources. A direct `Dispatch(1,1,1)` and `ExecuteIndirect` on identical input distinguish the body from the indirect path. A zero-work control checks attribution. A known miss node isolates the remaining traversal.
   - Use a diagnostics build with `VICTORY_D3D12_DEBUG=1`, `VICTORY_D3D12_GBV=1` and `VICTORY_D3D12_DRED=1` before device creation. This branch disables GBV resource-state tracking. Full state tracking is practical in a small replay. If the corrected binding still hangs, compare WARP on that replay and obtain the NVIDIA active-warp PC with the matching DXIL and driver shader debug information. Bisect the node load, traversal and LDS accesses while preserving convergent wave operations. [GBV](https://learn.microsoft.com/en-us/windows/win32/direct3d12/using-d3d12-debug-layer-gpu-based-validation) · [Nsight crash dump inspection](https://docs.nvidia.com/nsight-graphics/UserGuide/gpu-crash-dump-setting-up.html)
2. **Bound memory so world measurements can run longer.** Track loose windows by guest allocation identity and range. Reuse covering ranges and give the cache a byte budget. Retire entries after the last recorded list, descriptor and in-flight submission releases them. Write back authoritative GPU data before dropping its last copy. Rebuild page-table ownership on eviction. `memory::Layout` also holds strong references, so changing `LOOSE` to weak entries alone can leave windows pinned. Record live unique bytes, overlapping bytes, evictions, reuploads and DXGI local/nonlocal budgets alongside process commit.
3. **Restore producers in dependency order.** Trace a failing indirect argument, visibility list or BVH page to its last writer. Rank the refused shaders by downstream draws or dispatches unblocked. Use native wave64 where supported. On wave32, compile half-local operations natively when lane identity, EXEC, scalar decisions and LDS dependencies permit it. For a converged 64-thread group, use group-indexed LDS scratch for cross-half values. Publish, synchronize, consume, then synchronize before reusing scratch. Preserve all 64 mask bits, inactive lane state where the ISA requires it, first active lane, DPP/permute routing, reductions and prefix order. Implicit LDS communication also needs ordering.

   - A group barrier requires every invocation in the **entire host group** to reach the same dynamic instance, including explicitly padded invocations and lanes whose guest EXEC bit is clear. A wave-uniform branch in a multi-wave group is insufficient. Define logical lane IDs from the guest group layout. A native half-wave fast path also needs a proven mapping to physical lanes. A fallback for divergent wave-level code is for each of 32 physical lanes to carry two guest VGPR values with a single logical scalar PC/EXEC per guest wave. Key the shader cache and pipeline library on the selected host width and lowering mode. Validate masks crossing lanes 31/32, high-half-only EXEC, partial waves and divergent paths against a CPU reference or a tiny native wave64 run. [Wave size specification](https://microsoft.github.io/DirectX-Specs/d3d/HLSL_SM_6_6_WaveSize.html) · [Group synchronization semantics](https://learn.microsoft.com/en-us/windows/win32/direct3dhlsl/groupmemorybarrierwithgroupsync)
4. **Virtualize guest descriptor indices.** Keep all 1,245,184 logical descriptors on the CPU. A dense 32-bit guest-to-host map costs 4.75 MiB on the GPU. Populate it for reachable live views in one shader-visible resource heap, reserving physical slots for internal views, transient tables and typed nulls. Translate a guest index once at the handle boundary. Dynamic tables already rebased by the host and raw SRV/UAV pairs in the page table carry physical indices. Preserve that distinction for address, stride and write-mark metadata and mark lane-varying heap accesses nonuniform. Version mappings, descriptors and metadata together through consuming fences. Measure the maximum simultaneously reachable set. If it exceeds the slot budget, heap switches require demonstrably disjoint command batches and rebinding. [D3D12 heap binding limits](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12graphicscommandlist-setdescriptorheaps)
5. **Synchronize unique dirty ranges.** `memory::prepare` currently enumerates every copy named by the page table. Bindless preparation also visits the ranges a bound heap can reach. At 30 fps the observed frame copy volumes would require 26–68 GB/s just for those copies. Keep one canonical backing per guest allocation or physical page, with aliases sharing ownership and version. Track CPU writes once per epoch at OS-page granularity, combine adjacent dirty ranges, and transfer them once before a GPU reader needs them. Record DMA, GPU and aliased writes too. Preserve partial-page writes from different owners. Keep hot buffers in DEFAULT memory and use a persistently mapped, fence-retired upload ring. Read back GPU-owned ranges only for a guest CPU read, an explicit synchronization that makes the data CPU-visible, or eviction. For unpredictable shader addresses, keep the potentially reachable working set current and use access telemetry to refine it across runs.

   - `Host::shared_buffer` already uses write-back L0 system memory. D3D12 exposes `OpenExistingHeapFromAddress` for special-purpose diagnostic heaps wrapping a whole VirtualAlloc region. A controlled benchmark can compare a qualifying CPU-shared buffer with a persistent GPU-local copy. On the RTX, the GPU reaches system memory over PCIe. Check `OPTIONS16.GPUUploadHeapSupported` before benchmarking direct, sequential CPU writes into local memory. Measure dirty bytes, bytes actually moved, effective bandwidth, CPU preparation and waits. [Existing system-memory heaps](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12device3-openexistingheapfromaddress) · [GPU upload heaps](https://microsoft.github.io/DirectX-Specs/d3d/D3D12GPUUploadHeaps.html)
6. **Reassess 30 fps on the same world sequence.** Use a warm cache, correct indirect output and a run beyond the previous three-minute failure. Record dropped commands by reason, new graphics PSO keys by state field, stable commit, and p50/p95 frame, CPU and GPU times. For diagnosis, serialize selected dispatches with completion boundaries and bracket their GPU work with timestamps. Restore normal scheduling for frame throughput. A 33.3 ms frame requires about a 40–69× reduction in the reported steady GPU interval and 15–133× in CPU preparation. Pipeline warm-up and a lower loop limit alone cannot establish feasibility.
