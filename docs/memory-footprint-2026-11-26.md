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
