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

## What this branch did (newest first, all pushed)

- **8ec9285** Mesh shaders keep exports under the declared maxima, and a primitive with a vertex index past `max_vertices` is culled. These writes are undefined on the host. Suspected cause of the last world hang; **not yet confirmed in game** (that is run ss10).
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
  - Hypotheses, in order:
    1. Out-of-range mesh indices or exports (fixed in 8ec9285, to be checked).
    2. Huge garbage triangles from stale vertex data, giving seconds of pixel work.
    3. An out-of-range heap index in `createHandleFromHeap`.
  - If ss10 still hangs, bisect by skipping pipelines (`crate::pipeline::is_skipped`, diagnostics) or add per-draw GPU timestamps.

## Work in flight (local branches and worktrees, not pushed)

| Branch / worktree | What | State |
|---|---|---|
| `scalar-ported` (`.claude/worktrees/agent-a572ecb7c40e3e864`) | Port of the "scalar memory read as data" work (s_buffer_load record counts; reused descriptor bits at joins taken as undefined). Originally +173 validated. The original 3 commits are on `worktree-agent-a572ecb7c40e3e864`; they conflicted with 29dcbd5 in `lower.rs`. | An agent was re-applying it on 8ec9285. Check that the tests pass and that no stage's validated count drops, then merge. |
| `worktree-agent-af09ffdd7f771d608` | **derivs**: compute shaders that sample with implicit derivatives. Needs SM 6.6 derivatives in compute, with a group shape whose quads match the console's. About 95 programs. | Agent running. |
| `worktree-agent-a12b955eeb456e3c0` | **wave64**: about 319 wave64 compute programs that the 32-lane host refuses. Plan: emit without `WaveSize(64)` where nothing depends on wave width, emulate where feasible, refuse the rest precisely. These likely include the world's culling and indirect-argument passes. | Agent running. |
| ss10 game run | World run of 8ec9285 (diagnostics build), to check the mesh fix against the hang. | Was being started when this note was written. |

Merge one branch at a time. After each merge, rerun the shader tests, clippy and `translate --validate`, compare the counts with the table above, then push.

- Old worktrees from merged agents can be removed: `agent-a9ce295e7b6be8f67`, `agent-af1be18f616254684` and `agent-af756a61df2a95748` (`git worktree remove`).
- `%TEMP%\victory-30fps-lab\shader-integration` is a stale detached worktree with old conflicts; ignore or remove it.

## Next steps (by impact on the goal)

1. **Stop the world hang** (above). Nothing else can be measured while the session dies.
2. **Wave64 compute** (agent) and the **derivative compute shaders** (agent). Dropped compute passes break culling, lighting and indirect draws, and they force the indirect-draw gate closed.
3. **Copy cost:** per-frame shadow refresh of title memory is the largest CPU cost. See `crates/victory-gpu/src/shadow.rs` and `copystat`. Ideas:
   - track writes with page guards, and refresh only the pages that changed
   - keep GPU-written buffers resident instead of copying them back
4. **Pipeline creation:** warm caches between runs; compile pipelines in parallel, or ahead of time from `translate --pipelines`.
5. **Remaining refusals:** run `translate` and work down the grouped refusal list. The large groups are:
   - lane masks read as data
   - a descriptor's address or format read as data, for image descriptors (the scalar agent suggests extending its join rule)
   - interpolation-mode packing in signatures (a validator error)
   - mesh output over 32 KB
   - signature mismatches
6. Then profile GPU time (140–240 ms) to get to 33 ms per frame.
