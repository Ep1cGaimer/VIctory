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

A prompt for a second researcher is in the session scratchpad (`prompt-memory-footprint.md`); merge its answers here.
