# Cutting the memory footprint on a 15.3 GB RAM / 8 GB VRAM laptop (research, 2026-11-26)

## Why it matters
Every fork tried on this laptop (ours, Belobug, fxceless) commits 21–23 GB on 15.3 GB of RAM. The working set falls to ~1.5 GB, the process pages, one frame spends up to 2.5 s copying title memory, and the GPU sits ~90% idle. fxceless's fork reaches 7–12 fps on an RTX 4070 / 32 GB RAM and 0.2–1.9 fps here with the same switches (`docs/handoff-victory-world-30fps.md`). RAM is the wall, not the GPU.

## Where it goes (our branch, diagnostics counter `host::mem`, ~5 min into the world, `labRoot/output/v2-l256-timing`)
| What | Size | Notes |
|---|---|---|
| Title page pool (sections) | 6,236 + 710 + 185 MB | `xmem.rs` `XMemAllocatePhysicalPages`: `CreateFileMappingW` with `SEC_RESERVE`, pages committed when first mapped. 61,293 title views of it (61,287 of one 64 KiB page), 10.25 GB of views with aliasing |
| Title private memory | rest of the title's ~10–13 GB | `XMemVirtualAlloc` → `VirtualAlloc2` with `MEM_WRITE_WATCH` |
| **Shared copies (CUSTOM, WRITE_BACK, L0 = system RAM)** | **5.0 → 5.8 GB, growing** | a second copy of title memory: loose windows 2.9 → 3.2 GB (4 MiB windows kept forever, `shadow.rs` `LOOSE`), view copies 0.9 → 1.3 GB (14,000 → 20,000), whole-buffer copies 1.2 GB |
| Host textures (DEFAULT) | 4.6 GB | detiled copies; VRAM is full (7.2 / 8 GB), so ~6.3 GB of "shared GPU memory" spills into RAM |
| Depth twins, upload | 0.3 + 0.25 GB | |

GPU-visible title buffers the title declares are only ~1 GB (3,398 buffers; 286 MB private, 417 MB in one run of a section, 320 MB scattered pages; census at menu time). The 5–6 GB of shared copies is mostly windows of title memory reached by raw addresses, copied in 4 MiB windows that are never freed.

## Probe on this RTX 4060 (fxceless's `VICTORY_INPLACE_PROBE=1`, its `inplace.rs`, run 2026-11-26, `labRoot/output/fxceless-probe`)
- `ID3D12Device3` and `ID3D12Device13`: yes; heap tier 2; cross-adapter row-major textures: **no**.
- Private memory as a heap: `OpenExistingHeapFromAddress` (whole allocation) ok; `OpenExistingHeapFromAddress1` (a part) ok. Heap: CUSTOM, write-back, pool L0, flags shared | cross-adapter | write watch | shader atomics. A buffer placed in it (cross-adapter, UAV) ok. **The GPU reads the bytes, and CPU writes made afterwards with no call in between; the CPU reads what the GPU wrote, by copy and by UAV.** Write watch did not see the GPU's writes.
- While the heap stands, the title's own `VirtualProtect`/decommit **inside** the opened part fail (outside: fine). Release works after the heap goes.
- Page pool: a `SEC_RESERVE` section is **refused** (0x80070057); a `SEC_COMMIT` section is accepted, a buffer can be placed at a view's section offset, and it works through views made read-only, unmapped and remapped.
- Cost: opening 64 MB grew the working set and "shared with the card" by 64 MB (**the NVIDIA driver makes the whole opened range resident**; on WARP fxceless measured no working set for sections). The GPU copies 64 MB out of such a heap in ~20 ms, **the same as out of today's shared copies** (both are system RAM).

## What follows
1. **Zero-copy title buffers (biggest lever: ~5–6 GB RAM, and most of the per-frame copy and copy-back time).** Today's shadows live in system RAM anyway, so having the GPU use title memory in place costs no GPU bandwidth over today and drops the second copy. Open only the title ranges the GPU uses, with `OpenExistingHeapFromAddress1` on committed parts (private memory and committed section views), since opening pins what it opens. Windows outside declared buffers (the loose windows) are the bulk; open them per 64 KiB run, not per 4 MiB window.
   - The heap must be closed before the title protects, decommits or frees those pages (hook `VirtualProtect`/`VirtualFree`/unmap in `xmem.rs`; fxceless has `title_memory_freeing` for this).
   - Page guards (`VirtualProtect`) can't be used on in-place ranges, but they aren't needed there: the GPU reads live bytes.
   - Semantics match the console: CPU writes are visible at submission, GPU writes after the fence. Our late-wait / relay paths must not assume copies there.
   - Big, hot, read-mostly buffers can still get VRAM copies (Belobug's `card.rs`, fxceless's card buffers), within a budget.
2. **The page pool as `SEC_COMMIT`.** Needed only to open whole sections with `OpenExistingHeapFromFileMapping`; with `Address1` on committed views it is not needed. `SEC_COMMIT` charges 7.1 GB of commit up front (`xmem.rs` moved off it because a busy host refused 6.1 GiB, error 1450).
3. **Stop the growth now (quick win):** loose windows retire after their last consuming fence and are capped by bytes; view copies are merged and freed. These are 4.5 GB and growing in a 5-minute run.
4. **Textures under a VRAM budget:** `QueryVideoMemoryInfo` budget; evict cold host textures (destroy and re-detile from title memory on next use, or `Evict`/`MakeResident`), so VRAM stops spilling ~6 GB into RAM.

## Still to measure
- A census in the world (the probe's census ran at menu time).
- How much of the page pool the title has actually touched (resident) in the world, so the cost of pinning it is known.
- fxceless's `VICTORY_INPLACE=2` (title buffers in place, private memory only) on this laptop: commit and frame time against `fxceless-1`.


## Second researcher's answer, checked against the code (2026-11-26)
A second agent answered the prompt. Its claims, checked against `victory-world-30fps-v2`:

**Confirmed**
- `shadow.rs:89` `POOL_BYTES = 2 << 30`: up to 2 GiB of CPU snapshot `Vec`s are kept for reuse. They are not counted by `host::mem`, so they are commit on top of the 5–6 GB shared copies. The pool exists because fresh buffers cost a page fault per 4 KiB, so shrink it and measure rather than remove it.
- `shadow.rs:885` `WINDOW = 4 << 20`, and `LOOSE` holds every window forever (a `shortcut:` note says so). Smaller windows plus fence-retired eviction is the quick win.
- `texture.rs:512`: every texture keeps its own upload buffer (`Staging`) and a CPU `Last::read` for as long as it lives. Upload heaps total about 0.25 GB, so a pooled upload ring saves little RAM but is the right shape.
- `resource.rs:1897` `PLACED_TEXTURES`: one host texture per address and description, never released (a `shortcut:` note says so). This must be fixed for a texture budget to work.
- `EXACT` (view copies): one copy per (address, length) of a structured view that doesn't start on an element of its buffer's copy (`for_view`). Ring positions change every frame, so copies are made by the hundred. The suggested fix, keying copies by (stride, address mod stride) over the whole parent range, is sound. A buffer placed at m × 64 KiB in one heap covers phases that are multiples of gcd(64 KiB, stride); for stride 12 that is m = 0, 1, 2.
- There are no title `VirtualProtect`/`VirtualFree` hooks (`kernel.rs`). In-place memory needs them, because those calls fail with error 87 under an opened heap.

**Corrected**
- *"`HostHeap::kept` leaks view copies"*: no. `descriptor/host.rs:587` `keep()` replaces the slot and retires the old resource on overwrite. A copy lives while a slot still shows it, which is required. Slots the title never rewrites do hold copies, so ageing out slots not seen for N frames is a possible refinement, not a leak fix.
- *"20 ms per 64 MB is sysmem vs VRAM"*: the probe compared copying out of an opened heap with copying out of today's shared copies. Both are system RAM, so in place is no slower than today. VRAM mirrors for hot buffers are an extra gain, not a requirement.
- Its source for `inplace.rs` is given as `ztrx/main`; I read it in fxceless's fork. Check where it is merged before porting any code from it (no unmerged PRs).

**Agreed with, still unmeasured**
- `Evict` on an opened heap may not unpin pages on NVIDIA. Destroying the heap does.
- Lazy 64–128 MiB `SEC_COMMIT` slabs for the page pool, so one heap per slab replaces 61k single-page views and aliasing stays free. Commit is charged only for touched slabs.
- Destroying and re-detiling cold sampled textures, rather than `Evict`: `Evict` moves the bytes into system RAM, which is the scarce thing here.
- A stable 30 fps is not reachable on this laptop from memory work alone. Once paging stops, the GPU time (432 ms a frame in `v2-l256-timing`, with 138 wave64 pipelines refused) is the wall. A realistic target is 5–10 fps after the memory work, and more only with the wave64 and GPU work.

## Combined plan
1. **Quick wins, about 3–5 GB.**
   - `LOOSE`: 1 MiB windows, each with a last-use frame and fence; retire after the fence with a byte cap (clear its page-table and descriptor users first; check `memory.rs`).
   - `POOL_BYTES`: 2 GiB → 256–512 MiB.
   - `EXACT`: copies keyed by (stride, phase) over the parent range instead of per ring position.
   - Measure with one short world run, using the `host::mem` lines and commit/working set.
2. **Texture budget, about 1.5–2 GB of VRAM and spill.** A `QueryVideoMemoryInfo` budget; an LRU that destroys cold sampled textures and re-detiles them from title memory; `PLACED_TEXTURES` and per-texture staging made releasable.
3. **Census, then in place.**
   - Log the distinct pool pages committed and the backing of the loose windows in the world (private or view).
   - Probe `Evict` and `SEC_COMMIT` slab pinning on the RTX.
   - Then: in-place private memory (`OpenExistingHeapFromAddress1` on 2–4 MiB chunks, with `VirtualProtect`/`VirtualFree` hooks), and in-place page pool (lazy `SEC_COMMIT` slabs plus `OpenExistingHeapFromFileMapping`).
4. **VRAM mirrors** for hot, read-mostly buffers within a budget.

## Measured (2026-11-26, `mem-A-default` / `mem-B-evict`, build 79527fc, 6 min each, both still on loading screens)
The old `host::mem` counters were cumulative ("made"). The new "host memory live" line gives:

| | A: defaults | B: 1 MiB windows, idle 20 s, cap 1 GB, pool 512 MB |
|---|---|---|
| Loose windows live | 4,268 MB (976); lookups asked for 1,622 MB of their pages | 1,964 MB (1,481); asked for 837 MB; 2,847 given up (4.3 GB) |
| Whole-buffer copies | 1,180 MB | 1,189 MB |
| View copies live | ~300 MB (made: much more; most die with their slots) | ~300 MB |
| Snapshot pool | 0 MB at the sample | 0 MB |
| Process commit | 21.8 GB | 19.5 GB |
| Working set | 2.6 GB | 3.6 GB |
| ms/frame (last samples) | 3,700–5,100 (GPU 2,812) | 1,800–3,400 (GPU 1,452) |

- Captured frames match between A and B (loading-screen art, a fade in B at frame 2100), and no device removal. B reached frame 2100 in the time A reached 1800.
- Not yet checked: world frames with eviction on. Shaders reading through the page table at computed addresses see an evicted window as missing until a lookup asks for it again.
- So: view copies (0.3 GB) and the snapshot pool (0) are not where the memory is. Loose windows are, and the switches already cut 2.3 GB of them.
- What remains in the 19.5 GB is mostly the title's own memory plus about 3.5 GB of copies; textures sit in VRAM and spill. Zero-copy can remove the copies, but the title's memory stays.

## Step 2: host textures (2026-11-26, build a14fee5 and the census after it, `tex-C` / `tex-D`)
- **What changed.** A title texture now holds its host texture in a mutex, not a `OnceLock`, so it can be taken away. `host_resource` returns an owned reference.
- **Census.** Once a second it counts host textures by what holds them, together with `QueryVideoMemoryInfo`. It also adds up the title memory that target textures lay over.
- **Opt-in release.** `VICTORY_TEXTURE_IDLE_S` lets go of textures that the title fills, the host never wrote, and no list or slot has asked for in that long, through `host::retire`. `VICTORY_TEXTURE_BUDGET_PCT` limits that to when local use is above that share of the budget.
- **Measured** about 90 s in, still loading:

| Holder | Textures | MB |
|---|---|---|
| Targets (render, depth, unordered access) | 1,187 | 4,230 |
| Named by lists now | 7 | 7 |
| Descriptor slots only | 61 | 83 |
| Host-written | 9 | 6 |
| Free (what the idle release can take) | 1 | 0 |

- **What it means:**
  - The idle release has nothing to take. Its census bucket is empty, and `tex-C` with it on let go of 0.
  - Targets lay over 4,007 MB of title memory, and 3,845 MB of it is distinct. So the title's targets hardly alias one another, and putting them on heaps that mirror title memory would save only about 4 %.
  - The card has 5.3 GB local in use against a 7.2 GB budget, plus 4.5 GB of system memory. VRAM is not oversubscribed while loading.
  - The working set is 3–6 GB of 15.3 GB. RAM is not short while loading either. The 19 GB commit is a limit question, not a paging one, until the world.
- **Step 2 stops here.** Textures are not where loading time goes. The release stays opt-in for the world, where streaming may change the picture.
- **Both runs ended on the game's watchdog.** Each crashed at `+0x23cf683` (frame #1 `+0x2215c35`) right after the first big submission, which took 2.7 s:
  - It existed before this build: `v2-viewaddr-x` (older build) crashed the same way after 2,707 ms, and `mem-B-evict` survived 2,702 ms.
  - It is a race against the watchdog. The fix is making that first submission shorter (32 lists of compute: copies, 564 ms of preparing in one frame), not memory.

## Step 3: host copies of title memory (2026-11-26, builds 3be1ffe and bab0189, runs `mem-G` to `mem-X`)

- **Where RAM goes at the peak (run L, 10 minutes, typeperf beside the log):**

  | Item | Amount |
  |---|---|
  | Victory working set | 4.5 GB |
  | Modified page list | 1.5 GB |
  | GPU-locked system memory (the card's "system" use) | about 3.9 GB |
  | Pools (nonpaged + paged) | about 2 GB |
  | Paging | 8k pages/s in, 26k pages/s out |

  The machine has 11.3 GB free before the game starts. Run L ran out of RAM during the world load and crashed on the title's watchdog (`+0x23cf683`) after long stalls. The largest share we control is our host copies of title memory, at 3.3–4.7 GB of locked system memory.
- **Probes (ignored tests in `host.rs`: `evict_probe`, `in_place_probe`, `gpu_upload_probe`):**
  - `Evict` and `MakeResident` work, and writes through a mapping still land while the buffer is evicted. Eviction does not shrink locked memory enough to matter here.
  - `OpenExistingHeapFromAddress` works on private `VirtualAlloc2` memory, with or without `MEM_WRITE_WATCH`. It fails if any block is read-only (our guard pages), protection cannot change after opening, and it pins the whole allocation. Title pools are 2.2 GB and 1.2 GB, fully committed, so wrapping them in place would pin more than it saves.
  - The GPU upload heap (ReBAR) is supported on this RTX 4060. CPU writes run at 2.7 GB/s and reads at 22 MB/s.
- **Census (`shadow::backing`, `shadow::held`):** loose windows lie mostly over the title's mapped page-pool views. Most title buffers are made with unordered access (flag 0x4). About 700 MB of buffer copies are written by shaders.
- **What changed:**
  - **Loose windows (3be1ffe):** 64 KiB windows (`VICTORY_LOOSE_WINDOW_KB=64`), windows given up when idle (`VICTORY_LOOSE_IDLE_S=20`), and free windows a larger one covers are let go ("superseded"). Loose windows dropped from about 2.7 GB to 1.0–1.5 GB.
  - **Bounded lookups (bab0189):** constant, vertex, index and view-copy lookups ask only for their range, with no 64 KiB tail. Live loose bytes are now about 1.08× the bytes asked for.
  - **Copies in video memory (bab0189, `VICTORY_COPIES_IN_VIDEO=1`, opt-in):** copies of buffers made without unordered access, and loose windows, go in the GPU upload heap. When the budget leaves less than 512 MiB, they go in system memory as before.
    - A copy that shaders turn out to write is made again in system memory, because copy-back would read it at 22 MB/s. Its pages are remembered, so later windows over them start in system memory.
    - Title slots that view the old copy are made again before the next submission (`HostHeap::take_demoted`, matched by buffer).
    - Unordered views and root UAVs take system memory from the start.
  - **Buffer idle release (`VICTORY_BUFFER_IDLE_S`, opt-in):** it released nothing. Every idle buffer copy is still held by a slot or a page table.
- **Results, 5 minutes each, still loading at the end (64 KiB windows, idle 20 s, snapshot pool 512 MiB):**

  | Run | Card system memory | Copying back per frame | Stalls | Crash |
  |---|---|---|---|---|
  | K, M, N, P (copies in system memory) | 3.3–4.7 GB | 17–85 ms | several | none |
  | Q (video memory, no demotion) | 2.1–2.7 GB | about 172 ms | | none |
  | R (demotion, 5 copies) | 1.6–2.5 GB | 157–183 ms | 11 s, 4 s | none |
  | X (demotion and stale slots made again) | 2.1–2.7 GB | 18–47 ms | 6 s, 4 s | none |

  - Runs R to W were spent tracing why demotion did not help. The remaining reads were all on the marked-view path, from title slots made before the demotion. Those slots keep only the copy's buffer, so a check on the copy itself missed them.
  - In run X, copy-backs read video memory almost never: 0–5 pages per 15 s, against about 50 a frame before.
- **Still not good enough:** at the peak, RAM available still falls to 400–800 MB, and commit grows to 44 of 46 GB as the pagefile grows. The loading screen runs at 29–30 fps, then 0.5–1.5 fps during the load itself. No 5-minute run reached the world.
- **Long runs (2026-11-26, after 6edc329):**

  | Run | Settings | Length | Crash | Stalls | RAM available (min / median) | Peak commit | Card system memory (median) |
  |---|---|---|---|---|---|---|---|
  | L (earlier build) | 4 MiB windows, system memory | 10 min | watchdog | many | near 0 | | 3.9 GB |
  | Y | the settings above, set by hand | 12 min | none | 17, 110 s | 15 / 897 MB | 46.2 GB | 2.25 GB |
  | Z | defaults only (`VICTORY_SHADER_LOOP_LIMIT=256`) | 10 min | none | 25, 215 s | 17 / 958 MB | 49.6 GB | 2.14 GB |

  - Both reached the world, at 1,000–6,000 draws a frame. A steady frame takes 600–700 ms: 270–370 ms preparing (copies 70–160 ms, heaps 40 ms) and about 275 ms on the GPU. Copying back is 15–25 ms.
  - Hard faults are 27–222 a frame in steady play, so paging costs in episodes (the stalls, up to 8 s frames), not every frame.
  - Making a copy again in system memory reads the video-memory copy at 22 MB/s: 53 of them in run Z, with frames of up to 500 ms copying back when it happens. A GPU copy would avoid that.
- **Why RAM did not get freer:** the median available RAM is about the same with copies in system memory (K–P: 600–1,050 MB) as in video memory (R–Z: 750–1,040 MB). Locked memory let go goes to the title's paged-out pages, which are resident again. Process commit is 11.5 GB at boot (the title's own) and 19–23 GB in the world; mapped title memory is 10–20 GB committed but under 2.5 GB resident.
- **Why more copies cannot move to video memory now:** video memory is full at 6.65 of 7.19 GB, within the 512 MiB margin. Of it, 4,270 MB is 1,191 render targets lying over 4,047 MB of title memory (3,885 MB distinct). That is console-sized data, so widened formats and aliasing would save under 5 %. Only a lower render resolution would free a lot of it.
- **Commit headroom:** the pagefile is 20–40 GB (custom, not system-managed), so the commit limit can reach about 55.6 GB against a 49.6 GB peak. Without the game, 10.9 GB of RAM is free.
- **Next for RAM:**
  - About 1.18 GB of whole buffer copies (3,460), most of them made with unordered access. Moving them to video memory needs room there first (see above).
  - When a copy is made again in system memory, use a GPU copy instead of reading video memory from the CPU.
  - View copies.
  - Use tracking from the GPU side, so that windows that are idle but still held by a page table can be let go.

## More references (from the user's research agents, 2026-11-26; checked only where noted)
- **fxceless's fork (its pushed main, GPL-3.0; reviewed in part):** `resource.rs` `room()` and `let_go_idle()`, a texture budget from `QueryVideoMemoryInfo` with a 2 GiB floor, at 90 % of budget. It sweeps idle textures the title fills and the host never wrote, if no list or slot holds them. Also `texture.rs` `let_go_idle` for staging, `HostHeap::texture_slot_gone`, a snapshot pool capped at 512 MiB, `VICTORY_WINDOW_KB`/`VICTORY_WINDOW_TAILS`, and card buffers. Reported to carry an RTX 3050 8 GB for 90 minutes at 1–3 fps with a large pagefile.
- **XCL (GPL-2.0-only; ideas only, see below):** a self-sizing coherent heap with its size remembered between runs (`cache/uma_coherent.txt`), sparse VRAM windows through `UpdateTileMappings`, and batched write-watch.
- **Xenia (BSD-3):** `d3d12_tiled_shared_memory`, a reserved buffer mapped in 64 KiB tiles on touch, and a soft/hard texture cache limit.
- **Microsoft D3DX12Residency (MIT):** an LRU residency manager on `QueryVideoMemoryInfo`.
- **Yuzu (GPL-3.0+):** a texture/buffer garbage collector with `expected_memory` / `critical_memory` watermarks.
- **shadPS4:** image GC on a memory budget, and falling back to host-visible memory instead of failing with out-of-memory.
- **VKD3D-Proton 2.13:** no ReBAR upload heaps on GPUs with 8 GB or less.
- **Licence note:** the user allows code from GPL-2.0 repositories, but GPL-2.0-*only* code cannot legally be combined into this GPL-3.0-or-later project. GPL-2.0-*or-later* code can. Check each file's licence header.
