# World at 30 fps: proposed solutions

*For review. Nothing here is implemented, committed or pushed.*

- Date: 2026-11-25
- Branch: `victory-world-30fps`
- Evidence:
  - run ss13 (`%TEMP%\victory-30fps-lab\output\ss13-cull-handles-copies`)
  - the final shader census
  - disassembly of the refused programs
  - the code

## How to read this

Each problem has the same parts:
- what the data shows
- the cause: proven, or a hypothesis plus the experiment that decides it
- the proposal
- why I chose it over the alternatives
- why it should work
- how to check it
- the risks

The world runs at about 1 fps today: 766–1,100 ms per frame in ss13. 30 fps means 33 ms per frame, and no single fix gets there. The plan removes the biggest costs in the order that keeps each next measurement trustworthy. Re-take the numbers after each step before starting the next.

## Summary

| | Problem | Cause | Proposal | Confidence |
|---|---|---|---|---|
| d | About 900 pipelines always "being made"; 800–4,200 draws dropped per frame | Proven: every run starts with empty caches (harness setting) | One shared cache folder; report waits and drop reasons; if the count shows it helps, pipelines in current use jump the queue | High |
| – | The frame line doesn't add up | Proven: copy time is counted twice, and pipeline waits are not shown | Fix the two counters first | High |
| a | GPU hang whose crash site moves | Hypothesis: buffers read through heap indices keep their first copy (stale data) | A deciding experiment, then keep those buffers up to date; clamp indices as a safety net | Medium |
| b | Copies take 150–550 ms per frame | Proven in part: the cost is the work around copying (queries, failed reads, page faults), not the bytes (25 ms) | Measure by elimination, remove the overhead, then overlap CPU and GPU | Medium–high |
| c | GPU takes 150–370 ms per frame | Hypothesis: every buffer lives in system memory and is read over PCIe 4.0 x8 | Profile a warm run; test VRAM copies of read-mostly buffers behind a switch | Medium |
| e | 122 wave64 compute programs refused | Proven: NVIDIA runs 32 lanes, and 122 of 123 variants need cross-lane or shared-memory ordering | Run each 64-lane wave as two 32-lane halves; single-wave groups (94 programs) first | Medium–high |
| f | 160 programs the translator refused during ss13 | Several gaps. The largest (46): buffers whose element size only the view knows | Two small lowerings (`image_gather4_l`, `image_sample_d_cl`), then a prototype of raw buffer views plus a per-slot descriptor table | Medium |

## Recommended order

1. **d plus the accounting fix.** These take hours, not days. Until the caches are warm, most of the world's draws and compute passes never run, so copy and GPU numbers describe a different workload.
2. **The hang experiment (a).** Correctness comes before speed, and the outcome changes b: fixing stale data means more refreshing.
3. **b**: attribution, then fixes, then overlap.
4. **c**: profile a warm run, then the VRAM experiment.
5. **e stage 1**, then **f** (culling and material passes), then **e stage 2**.

Every translator change starts the cache cold again. That covers the heap clamp, wave64, new lowerings and a loop-limit override: the shader-cache key is the translator source hash plus the loop limit (`shader_cache.rs:54-55`). So:
- batch translator changes and warm the cache once;
- never compare a warm run with a cold one.

---

## 1. Pipelines and cold caches (d)

### What the data shows

- **The backlog never drains.** In ss13 the "being made" count stayed between 750 and 970 for the whole 5½ minutes in the world (17:18–17:23). In most windows, 800–4,200 draws per frame were dropped without a host pipeline. The run ended with 753 pipelines still queued, so those aren't in its library either.
- **Every run starts cold.** `tools/lab/run-probe.ps1:16-17` puts `VICTORY_DATA_DIR` and `VICTORY_CACHE_DIR` inside each run's own folder, so a run begins with no translated shaders and no pipeline library.
- **"Dropped without a host pipeline" also counts refused pipelines.** `host_pipeline` (`pipeline.rs:197`) returns nothing in three cases (`make.rs:110`):
  - one of its shaders doesn't translate;
  - the host refused the pipeline;
  - it is still being made.

  The diagnostics build already records which case each drop was (`make::missing`, `make.rs:74`), but the fps line shows only the total. So the total never falls below the draws and dispatches of refused and untranslated programs.
- **The caches themselves work.** ss13 left:
  - `cache/shaders`: 977 translated shaders, 72 MB
  - `cache/pipelines/host-pipelines.bin` (53 MB), with `early-pipelines.txt`

  The "library saved" lines are missing from the log only because the harness filter (`run-probe.ps1:21`) leaves out `victory_gpu::pipeline`.
- **A new pipeline is expensive on a cold cache.** It costs GFX10-to-DXIL translation, dxil.dll validation and the driver compile. This runs on 8 workers at below-normal priority (`pipeline/make.rs:167`); translation happens on first use (`pipeline.rs:107`).
- **Waiting is capped, and the wait is invisible.** A thread that needs a pipeline waits for it, up to 500 ms per frame shared by all threads (`make.rs:98`). After that it drops the draw or dispatch. The wait does not appear in the fps line. The queue is first-in, first-out (one channel shared by the workers, `make.rs`), so a pipeline a thread is waiting for can sit behind hundreds of others.
- **Making pipelines on the title's own threads was tried, and removed.** Decision 0007, quoted at `make.rs:1-8`, records why:
  - the first world frame wants over 1,200 new pipelines;
  - a translation can take ~0.4 s and some driver compiles 3.4 s;
  - made on the recording threads, that frame took more than 30 s and the title's GPU watchdog ended the run.
- **A dropped dispatch switches off indirect draws.** It calls `compute_dropped()` (`draw.rs:136`), which switches indirect draws off for that frame and the next (`draw.rs:151`, used at `draw.rs:1062`). The ss13 log gives that reason 104 times. Some dispatch is dropped in nearly every world frame (pipelines still being made, plus the 122 wave64 programs), so indirect draws are very likely off almost all the time.

### Cause

Proven. The harness throws the caches away, and the drop rule treats a missing compute pipeline the same as a missing draw pipeline.

### Proposal

1. Use one cache folder for all runs: `VICTORY_CACHE_DIR = %TEMP%\victory-30fps-lab\cache`, and keep `VICTORY_DATA_DIR` per run. Copying in ss13's `cache` folder would only help the build that made it: `target/diag/victory.exe` was built at 22:46 and `lower.rs` changed at 22:57. So in practice:
   - build once from the sources to be tested;
   - warm the cache with long runs that stay in the world until "being made" reaches 0, which may take more than one run.
2. Add `victory_gpu::pipeline=info` to the harness log filter.
3. Add these to the fps line:
   - milliseconds waited for pipelines per frame
   - pipelines made in the window
   - their average time
   - drop totals by the reason `make::missing` already gives: doesn't translate, refused, being made
   - how many distinct pipelines the frame asked for that aren't made yet (this sizes item 4)
4. Only if that last count is well below the backlog: let pipelines the current frame asks for jump the queue.
   - Each queued pipeline remembers the last frame that asked for it. The first time a frame asks, `ready()` also pushes it onto a front queue that the workers take from first. Compute pipelines go ahead of draw pipelines there.
   - A worker claims a pipeline (an atomic flag) before making it, and skips one that is already claimed or made. Without the claim, two workers could take the same pipeline. The second would block on the `OnceLock` (`make.rs:62-64`) for as long as the first takes, up to 3.4 s, and the "being made" count would drop twice.
   - The per-frame budget stays as it is.

### Why this one

- **Items 1–3** are configuration and logging. They change no behavior, and they make every later number meaningful.
- **Item 4**: a dropped draw only leaves a hole in one frame. A dropped dispatch does more damage:
  - its outputs stay stale;
  - indirect draws switch off for two frames;
  - the passes that read its outputs (culling results, indirect arguments, lists) carry on with old data. That is the same kind of data that can turn a cheap draw into a multi-second one (section 3).

  Today the queue runs in order of first use, and every first use waits (`ready()` queues, then waits), so "being waited for" doesn't single anything out. Being asked for again does. On a cold cache, a pipeline that every frame needs can sit behind ones the title asked for once (a loading screen, a one-off pass) and has since stopped using. Asking again moves it ahead, and compute goes first because its drops do the most damage. This reorders the work and doesn't shorten it: if most of the backlog is in use every frame, it helps little. That is why the count in item 3 comes first. The warm cache is the real fix.
- **Not chosen:**
  - *More workers, or higher priority.* On a warm cache, "making" a pipeline is mostly loading it from the library, which takes milliseconds.
  - *No budget, or making the pipeline on the thread that needs it.* That is what decision 0007 removed: on a cold cache the first world frame took more than 30 s, and the title's GPU watchdog ended the run.
  - *Precompiling everything offline.* `translate --pipelines` builds one pipeline per shader rather than the title's real pipelines, and it doesn't fill the runtime caches. A later `translate --cache` option could warm the shader cache on all 16 threads after a translator change, but it isn't needed first.

### Why it should work

The shader cache and the pipeline library are keyed by content. A second run with the same translator reads its translations from disk and its pipelines from the library. The NVIDIA driver also keeps its own compiled-shader cache across runs.

### How to check

After the warming runs, do one more run on the same route. Within a minute of reaching the world:
- "being made" falls close to zero;
- drops counted as "being made" fall close to zero;
- the new wait figure is close to zero.

If item 4 is built, compare two cold runs on the same route, with and without it: drops counted as "being made" should fall sooner with it.

Two things will *not* change yet, and that is expected:
- Drops counted as "refused" (the 122 wave64 programs, which the host refuses) and as "doesn't translate" (the 160 programs the translator refused) stay at their floor until sections 6 and 7 land.
- Indirect draws keep being dropped, because refused dispatches still close the gate in nearly every frame.

### Risks

- Any translator change makes the next run cold again (see the note under the recommended order).
- Item 4 keeps the 500 ms budget, so a cold start is no slower than today. Its cost is bookkeeping on uses whose pipeline isn't made yet: one frame number compared, and at most one push per frame.
- The cache key has no build-feature part. Today the diagnostics feature changes emitted code in one place only, the loop-limit override (`emit/mod.rs:127`), and that value is already in the key. But anything diagnostics-only that does change it needs one of two things, either the feature added to the key, or separate cache folders for diagnostics and release builds. The optional trap buffer in section 3 is such a change.
- More draws and dispatches will run, so copy and GPU time per frame will go **up**, not down. That is the real workload, and the later sections must be measured against it.

## 2. Make the frame line add up (do this with section 1)

### What the data shows

- In ss13 the copy figure is often larger than the preparing figure, although copies are part of preparing. Example, 17:23:37: copies 546 ms, preparing 426 ms.
- The ms/frame figure is larger than preparing + GPU + copy-back by 216–1,592 ms, usually 270–400 ms.

### Cause (found in the code)

- **Copy time is counted twice.** In `run_deferred` the `Step::Copy` timer starts (`host.rs:1302`) before `batch.title_written()` (`host.rs:1308`), which flushes the pending refreshes. Each refresh adds its own time to the copy counter (`Batch::refresh`, `host.rs:1435`). Then the copy step adds its whole elapsed time again, refreshes included (`host.rs:1353`).
- **Some time is not shown at all:**
  - pipeline waits (up to 500 ms per frame)
  - recording host lists (binding, descriptor mirroring)
  - the title's own CPU work

### Proposal

- Start the `Step::Copy` timer after `title_written()`.
- Add pipeline waits and list-recording time to the fps line, and print what is left over as "other".

### Why

The next steps choose between copy work, GPU work and pipeline work using these numbers, and two of the three are currently wrong.

## 3. The world GPU hang (a)

### What the data shows

| Run | How it ended | Where DRED stopped | Shaders |
|---|---|---|---|
| ss13 | 2.4 s timeout (TDR) | Five identical DrawInstanced calls (breadcrumbs 29–33) | `VS_DistantLights3d` reads heap indices per lane from a buffer (`buffer[v19].dword2 << 5`); paired with a trivial `PS_DistantLights` |
| ss12 | About 25 s | `DispatchMesh(1)` | `MS_VS_TransformAlphaPrepass` and `PS_AlphaPrepass` (a loop bounded by a constant-buffer count; image atomics). The mesh shader reads a typed buffer through a descriptor it picks with a packed value from a user register: `(s21 & 0x3ff) << 5`, a 32-byte stride. |
| ss10 | About 25 s | Small mesh dispatches | `MS_VS_TransformD` (heap index from a constant buffer; no loops) |

No run reported a GPU page fault.

### Hypotheses

**H1 (leading): stale buffer contents.**

`shadow.rs:13-16` states:

> only the ranges draws are known to read are brought up to date (vertex and index buffer views, constant buffers in tables, root descriptors, buffer views in tables); buffers read through directly indexed heaps keep what their first use copied.

All three shaders read buffers through descriptors picked at run time from data ("bindless" access). For ss13 and ss10 the pick is a heap index. For ss12 it isn't confirmed yet whether the descriptor comes from the heap, which is not refreshed, or from a table, which is. When the title rewrites such a buffer (per-frame instance data, light data, ring buffers), the GPU keeps reading the first copy. Old positions, counts or indices produce huge or wrong primitives, and a cheap pixel shader then covers the screen thousands of times. This fits every observation:
- Identical draws ran just before the stall, so the stall depends on data, not on the code.
- There were no page faults, because stale data still lies inside valid buffers.
- The crash site moves, because which buffer goes stale depends on timing and streaming.
- It explains ss10, which has no loop that could run away.

**H2: a loop bounded by data runs to the cap** (65,536 iterations per lane). This fits ss12's loop, but not ss10 or ss13.

**H3: heap indices out of range.** Indices from 737,856 up alias the dynamic slots, and indices from 1,000,000 up fall outside the heap. This is a possible consequence of H1 or H4, not a separate source.

**H4: producers that didn't run.** A dropped compute pass leaves its old outputs behind. The indirect-draw gate protects only indirect draws; direct draws and mesh dispatches that read those outputs are not covered.

### Proposal

**Step 1: decide before fixing.**
- **A counter** (diagnostics build). Per frame, it reports the bytes the title wrote, but the submission didn't refresh, in buffers that a mirrored heap view points to. It uses write-watch dirty pages, read without resetting them. The counter changes nothing; it shows whether stale data exists at all, and how much. It also times the write-watch check over those buffers, because the cost of step 2 depends on it.
- **A switch,** `VICTORY_REFRESH_HEAP=1`. At each submission it brings every buffer that a mirrored view points to up to date (dirty pages only). It is slow and meant only for diagnosis.
- **Runs:**
  - warm cache, DRED on
  - at least 10 minutes in the world per run (ss12 hung after 3 min 40 s)
  - two runs with the switch and two without
- **A side run for H2:** one run with `VICTORY_SHADER_LOOP_LIMIT=4096`. It has its own cache keys, so it starts cold; give it time to warm up.
- **H1 is confirmed if both hold:**
  - the counter shows megabytes per frame;
  - the runs with the switch don't hang while the baseline runs do.

**Step 2: fix, if H1 holds.** `mirror()` keeps the set of buffers that views in the shader-visible heap point to. Each submission brings their written pages up to date, the same way it already refreshes the ranges it knows about.

**Open question: the cost.** A write-watch query scans the whole range it is given. So checking every buffer the heap can reach costs in proportion to their total size, even when nothing changed. ss13 measured about 150 ns per 4 KiB page, so 1 GB takes about 40 ms. The counter run measures the real total before step 2 is designed. If it is too slow, there are two options:
- **Read-only guards** on those buffers, in 64 KiB blocks. `watch.rs` already falls back to these when write-watch isn't available (`guard`, `watch.rs:125`).
  - The cost is then one fault and one protection change per written block, instead of a scan of every checked page.
  - ss13 changed ~87 MB per frame. That is at least 1,400 blocks, and more if the writes are scattered.
  - One catch: a write by the OS itself (a file read straight into title memory, say) fails instead of faulting, so those paths need checking.
- **Checking once per frame** instead of once per submission. This would miss title writes between a frame's submissions.

**Step 3: a safety net, whatever the cause turns out to be.**
- Clamp heap indices computed from data to the title's range; an out-of-range index gets a null descriptor.
- Apply the console's buffer bounds rules: out-of-range loads return 0 and out-of-range stores are dropped. This comes with 7.3.
- Optionally, add a diagnostics buffer that records the shader hash when an index is out of range or a loop hits its cap.

### Why this one

- The shortcut in `shadow.rs` is a documented correctness gap on exactly the path all three shaders use. Closing a known gap beats tuning limits.
- The experiment separates H1 from H2 and H4 in a few runs, and the counter alone shows whether stale data exists.
- Clamping and bounds rules on their own only make garbage cheaper: the picture would still be wrong, and the draws could still be slow. They contain the problem; they don't cure it.
- **Not chosen:**
  - *A longer TDR delay.* It is a registry change, and it hides the hang rather than fixing it.
  - *Skipping heap-indexed draws.* Most world draws are bindless.

### Why it should work

If H1 holds, the GPU reads the bytes the title wrote, as it does on the console, and the draws shrink back to their real size. The dirty-page machinery (write watch and guards) already exists. The fix widens what it covers, not how it works.

### How to check

Compare the baseline with the fix on a warm cache, on the same route, for 10+ minutes, twice each. Expect:
- no hang;
- fewer "slow host submission" lines;
- a shorter longest submission.

### Risks

- More refreshing means more copy time. Section 4 must plan for more refreshes, not fewer.
- If the switch doesn't stop the hangs, H4 is next, and the answer is a **per-range gate**:
  - When a dispatch is dropped, mark the ranges it could write as stale until a later dispatch writes them. Those are its root and table UAVs, plus, for a program refused at pipeline creation, whatever its translation says it writes.
  - Drop any kind of draw whose arguments or inputs fall in those ranges.
  - This would also let through the indirect draws that today's all-or-nothing gate blocks.
  - Its limit: we can't know the heap-indexed writes of a program that didn't translate.

## 4. Copy cost (b)

### What the data shows

ss13, last copystat window, per frame:

| Measure | Value |
|---|---|
| Submissions | 16.6 |
| Refresh requests | 4,546, covering 3.7 GB of ranges |
| Write-watch queries | 598, taking 89 ms; 2.25 GB checked, of which 2.17 GB hadn't changed |
| The copying itself ("moves") | 25 ms |
| Unmapped title memory met | 64 MB |
| Copies that fell back to the slow path | 264 |
| Page faults | 116,822 |
| Texture data hashed | 84 MB |

Across all the windows:
- watches took 17–89 ms
- moves took 7–49 ms
- page faults ranged from 28K to 154K
- fallbacks ranged from 122 to 390

**So the bytes are not the problem.** About 87 MB changed per frame, and copying it took 25 ms. The time goes into the work around the copy.

### Where the rest goes (each part can be measured)

1. **Write-watch queries:** 89 ms for 2.25 GB checked, about 150 ns per 4 KiB page.
2. **Failed reads:** a copy that meets an unmapped page fails and is retried, first in 64 KiB blocks and then in 4 KiB pages, with `VirtualQuery` calls in between (`copy_sparse`, `host.rs:1488`). This happens 264 times per frame.
3. **Page faults:** 117K per frame across the process.
   - A soft fault costs 1–2 µs, so that is 100–250 ms if most are soft, and far more if they come from the page file.
   - The machine has 16 GB of RAM and the page file peaked at 6.9 GB, so there is memory pressure.
   - Host copies duplicate title buffers in system memory, which adds to that pressure.
4. **Bookkeeping per refresh** (locks, lookups), 4,546 times per frame.

### Proposal

1. **Measure by elimination**, with what already exists:
   - one warm run with `VICTORY_COPY_MODE=plain` (copies without the guard);
   - one warm run with `VICTORY_COPY_MODE=none` (no copies: timing only, so the picture will be wrong);
   - during a normal run, log `typeperf "\Memory\Page Reads/sec" "\Process(victory)\Working Set" "\Process(victory)\Private Bytes"` to tell hard faults from soft ones.
2. **Remove whatever the numbers point at:**
   - **Unmapped spans.** Remember them per host copy, either from the first failure or from xmem's mapping calls. Forget them when `title_memory_remapped` reports a change. Copy around them without the retry ladder.
   - **Watch queries.** Make one query per allocation per submission, over the union of its ranges. That saves calls but not the scan: a query costs per page checked, and 2.17 of the 2.25 GB checked per frame hadn't changed. For large, mostly clean ranges, read-only guards at 64 KiB granularity cost per written block instead (see section 3, step 2).
   - **Page faults.** If they come from the page file, the duplication is the bug. Free host-copy pages that haven't been used for N frames, or cap the total.
   - **Textures.** Where possible, detect changes by write watch or guard instead of hashing 84 MB per frame.
3. **Overlap CPU and GPU.**
   - Today every submission waits for the GPU (`WaitForSingleObject(INFINITE)`, `host.rs:682`), so a frame costs CPU preparation plus GPU time plus copy-back.
   - Instead, a completion thread would wait, copy back and then signal the title's fence, while the next submission prepares.
   - The host copies already record which ranges a running submission may write (`pending`), so a refresh can avoid them.
   - But snapshots and pending ranges are kept per host copy, not per submission (see the comment at `host.rs:637`). That has to change first.

### Why this one

- The time is in the calls, not in the bandwidth, so the fix is fewer and cheaper calls.
- The elimination runs come first because each fix is only worth its share of the time, and the shares are unknown. Page faults could be most of it, or very little.
- Overlap comes last because it is the biggest change. At 33 ms per frame it is needed anyway: CPU and GPU must *each* fit in 33 ms, not their sum.
- **Not chosen for now: zero-copy**, meaning the GPU reads title memory directly (`OpenExistingHeapFromAddress` or `OpenExistingHeapFromFileMapping`).
  - *For it:* it would remove every buffer copy, and the stale-data problem with them.
  - *Against it:*
    - these calls are documented for diagnostic heaps;
    - they only give system-memory buffer heaps;
    - they wrap whole allocations and pin them in RAM, which is risky with 16 GB and a busy page file;
    - title memory is made of placeholder section views that the title remaps.
  - *Verdict:* worth a time-boxed spike later (does it accept our allocations, how fast is it, how much can be pinned?), but not a plan yet.

### Why it should work

The changed data copies in about 25 ms. If the overhead falls to the same order, copies cost roughly 30–50 ms per frame, and overlap then hides most of that behind GPU time.

### How to check

Compare copystat before and after, under the same warm workload:
- watches and fallbacks per frame
- page faults per frame
- the corrected copy time

### Risks

- A remembered hole can go stale if a remap is missed; the copy would then write wrong zeros.
- Overlap changes the order of the title's fence signals. The fence tests and a long world run must pass before it stays.

## 5. GPU time (c)

### What the data shows

In steady state in ss13, the GPU took 150–370 ms per frame, with spikes: 61 submissions took over 250 ms, the last one 2.4 s. This is with most draws dropped.

### Hypotheses

- **G1: buffer reads over PCIe.**
  - Every host copy of a title buffer lives in system memory (`Host::shared_buffer`, `host.rs:2015`: a custom heap, write-back, `MEMORY_POOL_L0`).
  - So vertex, index, constant and structured data all cross PCIe 4.0 x8. That is about 13 GB/s in practice, against roughly 250 GB/s for the card's own memory.
  - A renderer reads the same data several times per frame (depth, shadow cascades, G-buffer).
- **G2: garbage geometry** from stale data (section 3). This would explain the spikes.
- **G3: VRAM over budget.** Textures live in the card's 8 GB. Past the budget, Windows moves allocations to system memory.
- **G4: translated code is slower than it should be** (waterfall loops, bounds checks).

### Proposal

1. Do a warm run with:
   - `VICTORY_GPU_TIMING=1` (the top commands of each slow list);
   - `QueryVideoMemoryInfo` on the fps line (VRAM and system-memory use against budget).
2. Write a small benchmark: GPU read speed from the system-memory heap versus VRAM on this laptop.
3. Run an experiment behind `VICTORY_SHADOW_VRAM=1`, where each host copy gets a VRAM twin:
   - refreshes still write the system-memory copy;
   - at the start of the submission, the changed ranges are copied into the twin on the GPU;
   - shaders use the twin;
   - after the submission, ranges that shaders wrote are copied back into the system-memory copy, before the existing copy-back runs.

   Compare GPU time with and without the switch.
4. If it helps, make it the default for read-mostly buffers (vertex, index and meshlet data, big constant arrays), within a budget taken from `QueryVideoMemoryInfo`. Buffers the GPU writes stay in system memory.

Also check `OPTIONS16.GPUUploadHeapSupported`; Resizable BAR is on (BAR1 is 8 GiB).
- *For upload heaps:* the CPU can write straight into VRAM.
- *Against them:* CPU reads from those heaps are very slow, so snapshots and copy-back can't use them.

### Why this one

- Measure first: shader-level fixes without per-command timing would be guesses.
- The VRAM experiment tests the biggest structural suspect behind a switch, without a rewrite.
- Copying only the changed bytes to the GPU takes about 7 ms per frame over the bus (about 87 MB). That replaces re-reading every buffer over the bus each time a pass touches it.

### How to check

Compare GPU ms per frame and per-command timings with and without the switch, on the same warm route.

### Risks

- The 8 GB of VRAM has to hold the textures too.
- The extra GPU copies cost time.
- Written ranges must be copied back before the title can read them.

## 6. Wave64 compute (e)

### What the data shows

- **Every one of these refusals is a wave64 program.** All 122 compute programs refused during ss13 are wave64. The translator marks them `[WaveSize(64)]`, which NVIDIA's 32-lane hardware refuses (`E_INVALIDARG`). Reading the PSV0 parts of the 977 shaders in ss13's cache finds 122 wave64 compute programs and 165 wave32 ones.
- **Almost all of them depend on the 64 lanes acting as one.** From the disassembly of all 122 (123 variants):
  - Only 1 needs neither cross-lane operations nor shared memory (`CS_ClusterLightsWorldSpace`).
  - 61 use shared memory (LDS). 54 of those contain no `s_barrier`, so they rely on all 64 lanes moving in lockstep.
  - 86 use cross-lane operations:

    | Operation | Variants |
    |---|---|
    | `mbcnt` | 56 |
    | `readlane` | 54 |
    | 64-bit mask find-first | 52 |
    | 64-bit mask counts | 46 |
    | DPP | 34 |
    | `permlane` | 31 |
    | `swizzle` | 21 |
- **Most groups hold a single wave.** Group sizes, from PSV0:

  | Threads per group | Programs |
  |---|---|
  | 32 or fewer | 4 |
  | Exactly 64 (one wave) | 94 |
  | 128 | 7 |
  | 256 | 15 |
  | 1,024 | 2 |

### Proposal: run each 64-lane wave as two 32-lane halves, in stages

**Stage 0** (4 programs, 32 threads or fewer). The upper half never has lanes, so run them as 32-lane waves with the upper mask bits set to zero.

**Stage 1** (94 programs, one wave per group). Use 64 host threads; a thread's half is its index divided by 32.
- **Keep both halves on the same path.** GFX10 branches are scalar: every branch depends on values the whole wave shares. So every 64-bit lane mask that is used as scalar data or decides a branch is assembled from both halves through group-shared memory and a group barrier. That covers exec, vcc, compare results, `s_bcnt1` and `s_ff1` over them, and `mbcnt`. Both halves then see the same values. With one wave per group, all 64 host threads take the same path, so every barrier is in uniform control flow, as D3D12 requires.
- **Reads across halves go through group-shared memory.** This covers `readlane` of a lane at 32 or above (or a computed lane), and `readfirstlane` of values that may differ between lanes. Operations that stay inside a 32-lane half (DPP rows, `permlane16`/`permlanex16`, `swizzle`) map to host wave operations.
- **Order writes and reads between halves.** When memory written by one half is read by the other, a barrier goes between the write and the read. This applies to shared memory and to buffers, and a scalar load counts as a read. The console needed no barrier here, because one wave runs in order.

**Stage 2** (24 programs, several waves per group). Use the round scheduler described in Leonida's design; we take the idea only and write our own code:
- cut the program at cross-lane operations and at memory conflicts;
- an outer loop runs every wave's halves one segment per round, with one barrier per round in uniform control flow;
- values cross halves through an exchange buffer;
- `s_barrier` becomes a scheduler state.

### Why this one

- **There is no native option.** NVIDIA has no 64-lane mode. Sending these dispatches to the AMD iGPU would mean copying their inputs and outputs between adapters for every dispatch, which costs far more than the work itself.
- **Ignoring the halves would be wrong** for 122 of 123 variants. Lane counts and compaction (`mbcnt`, `bcnt`) would give wrong offsets, which write wrong indirect arguments: exactly the garbage that can hang the GPU.
- **It avoids what sank the earlier draft.** The draft (worktree `a12b955…`) was rejected for two problems:
  - races in barrier-less shared-memory code;
  - a uniformity test that missed per-lane values from waterfall lowering.

  Stage 1 avoids both by construction: it never assumes values are uniform (it exchanges masks), and it orders every cross-half memory access with a barrier.
- **Spin-waiting between the halves is unsafe.** D3D12 promises no forward progress between waves, so it can deadlock.
- **Stage 1 comes first** because it covers 94 of the 122 programs with a much simpler structure than the scheduler. Its parts (mask exchange, cross-half reads, memory ordering) are also what stage 2 is built from.
- **Stage 2 is known to be buildable:** Leonida's scheduler was checked on a 32-lane NVIDIA GPU.

### Why it should work

With one wave per group and the masks exchanged, both halves follow the console's scalar control flow exactly, and every point where the halves interact runs in the console's order. Work done within each lane isn't affected by the split.

### How to check

- **Translator tests**, in the style of `translated_compute_matches_pc` and run on the GPU against a CPU reference, covering:
  - ballot and `mbcnt` compaction
  - `readlane` across halves
  - a shared-memory producer and consumer without barriers
  - branches driven by masks
- `translate --pipelines` over the 122: their pipelines get created.
- **In the world:** compute drops and the gate reason become rarer, and indirect draws come back.

### Risks and cost

- Barriers and exchanges cost GPU time in hot culling shaders. The counts per program are known from the disassembly, so the cost can be estimated before building.
- The exchange area plus the program's own shared memory must fit in 32 KiB.
- It relies on the host forming 32-lane waves from consecutive thread indices, which is true on NVIDIA. Where that doesn't hold, in-half operations also go through shared memory.
- Stage 1 is most of the work. Stage 2 is a second project.

## 7. Translator refusals (f)

### What the data shows

The translator refused 160 programs during ss13. (Across all shaders, the census has 2,775 refusals.) Ranked by use at runtime:

| Refusal | Programs | Examples (role in the world) |
|---|---|---|
| a structured buffer of unknown stride | 46 | `InstanceCulling` `CS_*_BatchCull` (trees, grass, props, imposters): world culling |
| not named in the log | 34 | still to be identified by hash |
| a scalar memory read as data | 18 | `MS_VS_TransformD`, skinned and depth mesh shaders: geometry |
| an address not from a root parameter | 17 | ray tracing, light probes, laser shadows: lighting |
| a descriptor's address or format joined with data | 16 | ped `CS_RaytracingResolve` |
| `image_gather4_l` | 7 | `meshlet_culling` `CS_BatchCull*Meshlets*`: world culling |
| a descriptor read from the first lane of computed VGPRs | 4 | `MS_VS_TransformD`, foliage mesh shaders |
| `image_sample_d_cl` | 3 | `PS_DeferredTextured`: world materials |
| a lane mask holding lanes elsewhere | 3 | ped `CS_RaytracingResolve` |
| others (1–2 each) | 12 | |

### Proposals, smallest first

**7.1 `image_gather4_l` (7 programs, meshlet culling).** Replace it with four texel loads at the chosen mip level, applying the sampler's addressing to the coordinates. (A gather returns one channel of the four texels that a bilinear sample would use.) Leonida does the same. Small and self-contained; high confidence.

**7.2 `image_sample_d_cl` (3 programs, deferred materials).** DXIL's `SampleGrad` takes a minimum-LOD clamp operand, so this maps directly. Small; high confidence once the operand order is checked against the console instruction.

**7.3 Unknown stride (46 programs, instance culling), together with scalar buffer loads and element counts.**
- **The proposal:**
  - Mirror structured and raw buffer views as raw host views.
  - Keep a per-slot table: a buffer bound at a fixed root parameter and filled by `mirror()`, holding each view's stride, element count and byte offset.
  - A structured access becomes *byte = index × stride + offset*. The stride comes from the shader when it is known, and from the table otherwise.
  - Bounds follow the console rules: an index past the count loads 0, and a store there is dropped.
- **Evidence:** `CS_Tree_BatchCull` uses one buffer descriptor (`s[12:15]`, loaded from a table) for four kinds of access:
  - indexed loads (`idxen`), whose element size comes from the descriptor
  - byte-offset loads (`offen`)
  - scalar loads
  - an atomic

  A structured host view with one fixed stride can't serve the byte-offset loads. A raw view can, but it needs the stride from somewhere, and the table supplies it. But the refused access itself isn't identified yet. The refusal's offset doesn't point at it. When an instruction without side effects can't be lowered, the translator marks its results as missing. It raises the error only where one of them is first needed (`lower.rs:2455-2459`). At `0xb4c` that is a branch. Its condition (`v_cmpx_ge_f32` at `0xb48`) is computed from values loaded earlier, so the refused access is one of the loads that feed it. So the first steps are:
  1. make the error also give the offset of the instruction that failed, not only where its value was needed;
  2. confirm that the raw-view model covers it, with a prototype and the translated-versus-PC tests;
  3. only then change the main buffer path.
- **Why not one pipeline per stride:** that multiplies pipelines (problem d), and it fails when one table mixes strides. Raw views plus a table give one shader for every case.
- **A bonus:** it removes the second objection from the scalar-ported review (a count from a structured view against raw byte loads on the same slot). Both now come from the table and the raw view.
- **Risks:**
  - This changes the most common buffer path. It must pass the existing translated-versus-PC tests and be measured for speed, as one change with its tests.
  - Views with hidden UAV counters need their counter mapped separately.

**7.4 Descriptor dwords read as data (18) or joined with data (16).** The same per-slot table also holds console-format descriptor dwords, which `mirror()` builds from the view's description: title GPU address, stride, count, format and dimensions. Reads of descriptor dwords then return real values, and nothing is zeroed at joins, which answers the scalar-ported review's first objection. Joins that build a new descriptor from an address need the address window in 7.6, so they wait until then.

**7.5 A descriptor from the first lane of computed VGPRs (4 programs, mesh geometry).** Treat `readfirstlane` of a computed value as a dynamic heap index that is the same across the wave, as the index-from-constant-buffer case already is. Medium.

**7.6 Addresses not from root parameters (17 programs, lighting) and SGPR addresses (the geometry-shader refusals).** These programs follow title GPU addresses stored in memory.
- The long-term answer is one host buffer window covering the title's GPU address range: a reserved resource whose 64 KiB tiles map onto the host copies' memory. Any title GPU address then becomes an offset into it.
- Leonida maps the console window into one allocation in the same spirit.
- It is a large change, so it comes after the rest.

### Why this order

- 7.1 and 7.2 are small, and they unblock world culling and materials.
- 7.3 and 7.4 are one descriptor-model change. It unblocks the largest group and fixes the rejected scalar-ported approach.
- 7.5 is a moderate change.
- 7.6 is an architectural change.

## What I need from you

1. Agree to the order above, or change it.
2. Approve the section 1 changes: one shared cache folder warmed by long runs of a single build, and, if the new count shows it would help, letting pipelines the current frame asks for jump the queue.
3. Approve the counter and the switch for the hang experiment (diagnostics build only).
4. For 7.3 and 7.4, approve *investigating* a change to how buffer views are mirrored (raw views plus a per-slot table): find the refused access, build a prototype, run the tests, and come back with results before the main path changes. If it goes ahead, it is the biggest design change here.

Still waiting from before: committing the verified work, updating the handoff, and pushing.

## Sources

- **ss13 log and copystat:** `%TEMP%\victory-30fps-lab\output\ss13-cull-handles-copies`
- **Census, disassembly and analysis:** the session scratchpad, which is temporary: `C:\Users\vivek\AppData\Local\Temp\claude\1efc4d22f19fa6cd\5cc6c1e4-c4c7-4582-8421-e4639eb6cb24\scratchpad`
  - census: `C:\Users\vivek\AppData\Local\Temp\claude\1efc4d22f19fa6cd\5cc6c1e4-c4c7-4582-8421-e4639eb6cb24\scratchpad\census-final.txt`
  - refused wave64 programs: `C:\Users\vivek\AppData\Local\Temp\claude\1efc4d22f19fa6cd\5cc6c1e4-c4c7-4582-8421-e4639eb6cb24\scratchpad\dumps\w64`
  - instance culling shader: `C:\Users\vivek\AppData\Local\Temp\claude\1efc4d22f19fa6cd\5cc6c1e4-c4c7-4582-8421-e4639eb6cb24\scratchpad\dumps\instcull`
  - wave sizes and group sizes, read from PSV0: `C:\Users\vivek\AppData\Local\Temp\claude\1efc4d22f19fa6cd\5cc6c1e4-c4c7-4582-8421-e4639eb6cb24\scratchpad\psv.txt`
- **Leonida** (ideas only, no code):
  - `design.md` rows 257, 321, 358 and 366
  - `buffer_bounds.md`
  - `descriptor_lookup_ada.md`
