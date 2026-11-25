# Handoff: VIctory world at 30 fps (branch `victory-world-30fps`)

Last updated 2026-11-24. This file is for the next agent or developer picking up this branch.

## Goal and ground rules

- **Goal:** the title (`C:\Games\GTA6.Xbox.v1.0.2.1-CYBERLEEK`) running in the world with clean, smooth graphics at a locked 30 fps on this machine.
- **Never write to the game folder.** Reading it is fine. The owner wants to reuse it for other ports. After runs, check that nothing changed. Fingerprint (file count, total bytes, newest write time):
  `109 224001114630 2026-11-24T09:04:57.8026955Z`.
- **Trusted code only.** Take code only from `main` or the project's own branches of upstream VIctory (`Marvisak/VIctory`), or from popular working repos. **Never** take code from PRs, forks or contributors' integration branches; write it yourself instead. XCL (`ivall/xcl`) is GPL-2.0 and VIctory is GPL-3.0: take ideas from XCL, not code.
- **Remotes:** `origin` = `Ep1cGaimer/VIctory` (push here, branch `victory-world-30fps`); upstream = `Marvisak/VIctory` main.
- **Commits** end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Machine

- RTX 4060 Laptop plus an AMD iGPU (hybrid). The host device is made on the high-performance adapter (commit 4aa2f9f). Check the "host adapter" log line.
- Waves are 32 lanes only: `WaveSize(64)` compute pipelines are refused with 0x80070057.
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

## Current state in game

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
- Direct heap addressing is still a separate hang candidate. In the ss10 shader, the host uses a metadata-derived texture index without bounding it to the static range (`emit/lower.rs`, `heap_index`). On the capped heap, indices from 737,856 through 999,999 alias dynamic table descriptors, and indices at or above 1,000,000 are outside the heap. The crash log does not contain the computed index. Check the GPU-visible metadata value before attributing the hang to this path.

## Work in flight (local branches and worktrees, not pushed)

| Branch / worktree | What | State |
|---|---|---|
| `scalar-ported` (`.claude/worktrees/agent-a572ecb7c40e3e864`) | Scalar buffer record counts and descriptor bits at joins. | Review found two correctness issues. The join replaces reachable descriptor data with zero without a proof that the value goes unread. The count query uses a structured view while the scalar load uses raw byte addressing against the same heap slot. Keep the worktree as reference. Prove an exact static structured SRV slot and use element and byte-within-element addressing before enabling it for live scalar loads. |
| `worktree-agent-af09ffdd7f771d608` | Implicit compute derivatives. | Integrated as `e5ee3ce`. The old worktree contains the unfinished session's copy. |
| `worktree-agent-a12b955eeb456e3c0` | Wave64 on Wave32 hardware. | Review found the draft can race across split waves in barrierless LDS code and its uniformity test overlooks per-lane values produced by waterfall lowering. The conservative LDS-only fallback to a 32-lane wave covers three shaders in a local scan. Refer to `C:/Vivek_projects/Leonida/docs/design.md` for the Vulkan implementation's ordering rules. |

Merge one branch at a time. After each merge, rerun the shader tests, clippy and `translate --validate`, compare the counts with the table above, then push.

- Old worktrees from merged agents can be removed: `agent-a9ce295e7b6be8f67`, `agent-af1be18f616254684` and `agent-af756a61df2a95748` (`git worktree remove`).
- `%TEMP%\victory-30fps-lab\shader-integration` is a stale detached worktree with old conflicts; ignore or remove it.

## Next steps (by impact on the goal)

1. **Stop the world hang.** Run with the mesh fix and all pipelines enabled. Check which shader and command the next DRED breadcrumb names. Keep timing active until the world has drawn for at least two minutes.
2. **Wave64 compute.** Dropped compute passes break culling, lighting and indirect draws, and can force the indirect-draw gate closed. A correct emulation needs logical lane IDs, ballot and lane exchange semantics, and ordering across the two host waves, including shared memory.
3. **Copy cost.** The last three ss10 windows reported 118–297 ms/frame in write-watch calls and 167–206 MB/frame of unmapped reads. `Batch::gpu_write` can flush just the shadow it snapshots and leave other refreshes coalesced. Retain full flushes before title-memory writes and submission. Separately, skip known reserved spans after a failed read in the sparse-copy fallback. Compare `copystat` under the same world workload.
4. **Pipeline creation:** warm caches between runs; compile pipelines in parallel, or ahead of time from `translate --pipelines`.
5. **Remaining refusals:** run `translate` and work down the grouped refusal list. The large groups are:
   - lane masks read as data
   - a descriptor's address or format read as data, for image descriptors (the scalar agent suggests extending its join rule)
   - interpolation-mode packing in signatures (a validator error)
   - mesh output over 32 KB
   - signature mismatches
6. Then profile GPU time (140–240 ms) to get to 33 ms per frame.
