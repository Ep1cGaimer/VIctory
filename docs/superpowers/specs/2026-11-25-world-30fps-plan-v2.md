# World at 30 fps: plan v2 (after surveying upstream, the forks, XCL and Leonida)

Date: 2026-11-25. Branch at the time of writing: victory-world-30fps (base ffe310e plus our commits plus uncommitted work).

This replaces the order in [2026-11-25-world-30fps-solutions-design.md](2026-11-25-world-30fps-solutions-design.md).
That document still holds the evidence and reasoning for each problem; this one says what changed around us and what to do now.
Nothing here has been implemented, committed or pushed.

## How to read this

- **Problem letters** are the same as in the first proposal:
  - a: the world GPU hang
  - b: copy cost
  - c: GPU time
  - d: pipelines and cold caches
  - e: wave64 compute
  - f: translator refusals
- **One new problem:** **g**, black shadows (lighting).
- **Who wrote what:** "Upstream" is Marvisak/VIctory main. "The fork" is fxceless/victory-fork; ztrx is a repackage of it (see §1). "XCL" is xcl-dev/xcl. "Ours" is this branch.
- **Trust rule:** code may be taken only from upstream main or its own branches. Everything else is ideas only, re-written by us.

## Summary

1. **Rebase onto upstream/main first.**
   - Upstream fixed a large share of our problem list in the last day:
     - the pipeline prebuild and library loads (d)
     - copy-backs only for pages shaders actually mark (b)
     - every heap range current at submission (a)
     - raw-address memory, the per-slot stride table, the BVH node test and many lowerings (f, g)
   - All three lines (ours, upstream, the fork) split from the same commit, ffe310e. Upstream is the only one we may take code from.
2. **Then warm the caches once with upstream's `translate --cache --pipelines`**, kept in one shared folder. This is our old d, mostly solved by upstream.
3. **Re-measure before building anything else.** Several old proposals (a, b, the front queue in d, raw views in f) may already be covered.
4. **New work in priority order:**
   1. lighting (g), starting with the parked lane-mask fix
   2. wave64 as two 32-lane halves (e), designed after XCL
   3. CPU and GPU overlap, the fork's "late wait / relay" idea
   4. GPU-side items (c)
5. **Honest frame rate.** The best public NVIDIA number is 240–450 ms per frame (2–4 fps) on a desktop RTX 4070, from the fork. Upstream reports 1–2 s per frame on AMD. A locked 30 fps (33 ms) is not close for anyone. The plan is milestones with numbers, not a date.

## 1. What changed out there (2026-11-24 to 2026-11-25)

### Upstream VIctory (trusted)

- **Scope:** main moved from ffe310e to 940d32f: 182 commits, 132 files, +38,396 / −2,151.
- **Branches:** the only other branch is `fix/tests-without-gpu` (one commit: tests that need a GPU skip without one).
- **Status, upstream README of 2026-11-24:**
  - The world draws at 1–2 s per frame on an RX 6700 XT; surfaces are unlit.
  - Shaders: mesh 8,765 / 8,765, VS/PS 12,550 / 12,592, compute 2,579 / 2,637.
  - Still listed as missing: merged geometry stages, and "compute programs that need 64-lane waves". So upstream does **not** solve e.

The maintainer commits that matter to us:

| Commit | What it does | Problem |
|---|---|---|
| 9c58a1c | `translate --cache --pipelines` builds the pipeline library ahead of time from the shader pack (~27,000 pipelines, ~20 min on 16 threads); mesh pipelines can now load from the library (before, every one without an explicit topology was refused) | d |
| c47b529 | shader warm-up; refusals cached; cache keyed by translator source; draws go ahead of early starts | d |
| f821cb9 | pipelines of different names load from the library at the same time | d |
| cd94d35, 940d32f | shaders mark the 64 KiB pages they write through UAVs; copy-backs compare only marked pages (before: 2.8–4.5 GB a frame, 98 % unchanged, 0.45–0.7 s per frame) | b |
| 6d021fe | one page table per view heap; refresh once per submission; change checks don't wait on the memory lock; copy-back snapshots only declared ranges | b |
| c75fdba | lists that index the heap have every range current at submission; 64 KiB page change journal with guards; kernel writes and aliased mappings reported | a (our H1 hypothesis: stale heap shadows) |
| 86a5ed6 | views off a buffer's elements get their own copy (culling constants used to read zeros) | a, f |
| 4051149, d71c388 | null views for empty or out-of-range buffer views (one removed the device); bounded root views; guard against texture descriptors read as buffers | a (H3) |
| 0f63757, 27c3dc9 | title memory at raw addresses through a two-level page table; root views through it, bounds-checked | f, g |
| 0b35b7f | 64-bit image atomics, the BVH ray test, lane masks, a per-slot stride table for structured buffers, `image_gather4_l`, LDS byte/half accesses, hardware SGPR inputs | f, g |
| 683ad7a, 75677cb, 8af0472, 13bbd6f, e0034c2 | rect lists through a GS; buffer sizes from descriptors; lane masks spilled and combined; one host texture per texture memory; relative moves bounded | f |
| 42961e8, 13185f5 | log TDR settings (warn when a hang would freeze the PC); GetDeviceRemovedReason | a (diagnosis) |

### fxceless/victory-fork and Zettrox/ztrx (untrusted: ideas only)

**ztrx is a repackage.**
- ztrx is victory-fork at commit 2119b0e plus one commit (90b529f), which adds a README and `scripts/play.ps1`.
- It has no code of its own. Its 1–3 fps "prologue playable" status on an RTX 3050 is victory-fork's work.
- It was created at 01:12 UTC today and had 444 stars and 76 forks about two hours later. That looks inflated or hype-driven. It is not a reason to trust it.

**victory-fork** (2,121 stars, GPL-3.0):
- Based on ffe310e, like us. It has **not** taken upstream's 182 commits.
- Main is now 310c6eb, five commits past what ztrx copied.
- It merges other people's PRs and fork branches whole, for example 3bff182e and 31714deb, which "take the … branch of …'s fork".
- Its README reports, on an RTX 4070 desktop:
  - 2.5–4 fps in the world
  - 420–450 ms per frame by default
  - 240–350 ms per frame with its new switches
  - everything in shadow black
  - about 24 GB of memory in use

Fork ideas that upstream does **not** have yet, and that matter here:

| Idea | Fork's numbers | Where it goes in this plan |
|---|---|---|
| **Parked lane-mask restore.** The console compiler parks a wave64 mask in two lanes of a spare VGPR and restores the halves with instructions in between. A relative VGPR read (`v_movrels`) should not turn unspilling off. | Lets `CS_IrradianceCache_Shade` translate; it was "dropped every frame, leaving the irradiance cache unlit and everything in shadow black". Costs ~40 ms of GPU time per frame on their machine. | g, step 4 |
| **Late wait and relay.** Submissions go ahead of the wait for the GPU; a relay thread lands results as the card finishes (`VICTORY_LATE_WAIT`, `VICTORY_RELAY`). | GPU idle behind the submitting thread fell from ~136 to ~30 ms per frame | step 6 |
| **Shader clean-up passes** over the finished DXIL: dead code, folding, reuse, branch facts, loads within a block | ~43 % fewer instructions; ~20 ms of GPU time less per frame | step 7 |
| **Large title buffers copied to card memory,** with shader writes found by hashing on the card | part of their 240–350 ms | step 7 (our old c hypothesis, independently pursued) |
| **In-place title memory:** `OpenExistingHeapFromAddress(1)` / `FromFileMapping` so shaders use title bytes with no copy | first stage only; probe on the GPU works. Risks they wrote down: tile mappings into such a heap remove the device; while open, the title cannot free or protect those pages; opening locks the pages. | step 8 (research) |
| **A barrier form for wave64 removed the device** (barriers inside branches and loops); now off unless `VICTORY_WAVE_WIDE=1` | — | e design rule |
| **All or none for the BVH builder:** its stages run only when every builder pipeline exists. A half-working builder wrote garbage bounds into title memory and crashed the title. | — | g |
| **`VICTORY_HOST_MAX_WAVE=32`** on translate, so the prebuild matches NVIDIA | — | not needed: see step 2 |


#### What victory-fork has measured, and whether upstream has it

ztrx itself adds nothing, but the fork it carries is the result of more than 120 numbered game runs on NVIDIA. Its code comments record what each change was worth.

- **About these switches:** they are the fork's own. Upstream has none of them; in the third column, "yes" means upstream fixed the same problem its own way.
- **How they map to our problems:** many name exactly the costs our ss13 measurements attributed to "the work around copying, not the bytes" (old b) and to the pipeline waits (d).
- **Checked against upstream/main:** the third column is from reading upstream/main where noted. "?" means not checked yet; check it during the rebase.

| Fork finding (switch) | Measured on their machine | Upstream/main | Plan step |
|---|---|---|---|
| **The pipeline wait budget is per thread, not per frame** (`VICTORY_PIPELINE_WAIT_SHARED`): it counts only after a wait ends, so each of five recording threads starts with the whole 500 ms | 2.5 s of waiting in every frame while pipelines were being made (runs 34–49) | **same bug**: `make.rs:227-238` reads `WAITED_US` before the wait and adds to it after | 2 (the first proposal assumed a shared budget; that was wrong) |
| **Memory questions walk page tables.** `VirtualQuery` was asked before copies (`VICTORY_COPY_ASK`), per texture slot (`VICTORY_COMMITTED_QUERY`), for guarded views (`VICTORY_GUARD_ASK`) and per emulated `clzero` (`VICTORY_CLZERO_ASK`) | 2–9 ms per question in 0.6–2.2 GB heaps, 89 ms in the 6.2 GB pool; six title threads spent 25–78 % of their time there (~27 s CPU per 9 s frame); 71 of 500 samples on the submitting thread; a texture pass of 208–264 ms | upstream already writes without asking first (`host.rs:2053-2073`); `clzero` has no query; 8 other `VirtualQuery` call sites are not checked yet | 3, 6 |
| **Snapshot only what can be stored to** (`VICTORY_WRITE_SKIP`): a UAV range is declared written only if the shader's code can store to buffers | 3.97 GB per frame of snapshot and compare, 0.49 s + 0.28 s (run 28) | upstream's view marks (cd94d35, 940d32f) cover the compare side; snapshots: ? | 3 |
| **Large title buffers in VRAM** (`VICTORY_CARD_BUFFERS`, on by default) | BVH skinning list 50.5 → 4.5 ms on the card (run 75) | no | 7 |
| **Ask once** (`VICTORY_ASK_ONCE`, on) | 375–450 ms per frame against 573–647 | ? | 6 |
| **Hold submissions list by list** (`VICTORY_SPLIT_WAITS`): holding a whole submission for a memory-value wait made the queues wait for each other until the present forced them | 2.4–2.9 s per frame before the present, against 13–32 ms after | ? (not found in queue.rs) | 3: a candidate cause of slow and stuck frames |
| **A late frame wait ends on the tick that passed** (`VICTORY_LATE_WAITS`) | 17 ms of every 730 ms frame | **missing**: `frame.rs:100-117` always takes the next tick | 6 (small) |
| **Indirect clamp in windows** (`VICTORY_INDIRECT_WINDOW`): one pass clamps many mesh-draw records instead of one compute dispatch and two waits per record | 3,460–4,126 records per frame; 14–16 µs each even for empty draws | ? | 7 |
| **Group barriers on NVIDIA** (`VICTORY_GROUP_SYNC`): a barrier some thread never reaches holds the group until Windows resets the GPU | device resets | ? | 3: **a hang candidate** for our problem a |
| **Root SRVs as sized one-view tables** (`VICTORY_ROOT_SRV_DIRECT`): reads past a buffer copy hit freed host memory | device removed (their reference: upstream PR #493) | upstream has bounded root views (d71c388); check it covers mesh vertex fetch | 3 (hang) |
| **Root views cut at 1 MiB** (`VICTORY_ROOT_VIEW_MB`) | 1,452 of 2,021 mesh streams read further: meshes without bodies, exploded triangles | ? | 3 (picture) |
| **Driver background recompiles** (`VICTORY_D3D12_BACKGROUND=0`) | 1,000–1,200 ms against 830–950 ms per frame while those threads ran | — (a D3D12 switch) | 3: one measurement |
| **Mapped pipeline library:** the file was replaced while still mapped | intermittent device loss | ? | 2 (we use the library heavily after the prebuild) |
| **Texture lock wait while the runtime remaps** (`VICTORY_TEXTURE_NO_WAIT`), texture guards instead of hashes (`VICTORY_TEXTURE_GUARD`) | frames of 300–3,708 ms of "textures"; 51 ms per frame hashing 229 MB | ? | 6 |
| **Threads for refresh, merge, snapshot and compare** (`VICTORY_WIDE_REFRESH`, `VICTORY_WIDE_MERGE`, `VICTORY_WIDE_BACK`); pooled lists and scratch (`VICTORY_LIST_POOL`, `VICTORY_SCRATCH_POOL`, `VICTORY_LAZY_PRELUDE`) | 45–50 ms of refreshes; 51 + 35 ms of snapshots and compares; 57 upload buffers of 4 MiB made per frame | ? | 6 |
| **Its own wave64-as-two-host-waves** (`VICTORY_WAVE_WIDE`, `VICTORY_WAVE_DEEP_SKIPS`, `VICTORY_WAVE_LOCAL`), with a `VICTORY_WAVE_WHY` report | the barrier form lost the GPU (runs 121–124), so it is off; the barrier-free 32-lane form is on | no | 5: a second design to compare with XCL's |
| **Diagnostics:** `VICTORY_FAIL_DUMP` (first 3 programs per refusal reason, saved for offline work), `VICTORY_SHADER_SAVE`, `VICTORY_SHADER_CENSUS`, `VICTORY_MESH_TEST`, `VICTORY_SCISSOR_TEST`, `VICTORY_BARRIER_TEST` | — | no | 3, 9 |

**How this changes the plan:**
- **Step 2:** add the shared-budget fix to the waits work. This is a real upstream bug, still present.
- **Step 3:** test these as hang causes on the new base, before writing anything of our own: barriers some thread never reaches; reads past root SRVs; submissions held whole.
- **Step 6:** becomes "the fork's measured list". Take its findings in the order of their measured gains, re-measure each on our machine, and write each fix ourselves. Many are small: a query removed, a lock not waited on, a budget counted correctly.
- **Optional baseline:** run victory-fork (or ztrx, the same code) on this laptop once, to see what our 4060 gets and how its picture compares. This means building and running untrusted code (ask 2b).
  - **What I found in a read-only scan:** no `build.rs`, no binaries, no new URLs or network calls. The only new process spawn is `dxc`, in a test module (`pipeline/host.rs:847-887`). Its dependency changes are lint settings and the `zlib-rs` swap.
  - **What that scan is not:** a line-by-line review of 37,000 lines.

### XCL (GPL-2.0: ideas only)

XCL is another compatibility layer, used for other titles. Its "Retail game reaches 30 fps" (956a476) is a different title and does not measure this one.

What applies to us:

- **Wave64 on 32-lane GPUs** (3ecf4cf and follow-ups):
  - Each console wave runs as two hardware wave32s of the same thread group. They exchange through groupshared memory where a value must be the whole wave's: ballots, `readlane`/`readfirstlane`, `ds_bpermute`, GLC scalar loads.
  - Both halves keep the full 64-bit EXEC/VCC and identical SGPRs, so they take every branch the same way.
  - DPP, `ds_swizzle` and `permlane16` stay inside a half.
  - Several console waves per group meet pairwise through double-buffered slots and arrival counters.
  - The exchange defaults to a **bounded spin**, not group barriers (066ae5b): a desynchronised pair degrades instead of hanging the GPU.
  - NVIDIA does not form waves from consecutive thread indices in 3-D groups, so hardware threads take console IDs in arrival order.
  - VS/PS classification by whether a 32-lane cut changes them (dfc5578): 7,208 of 13,210 programs are exact.
  - On a GPU that runs 64-lane waves the output is byte-identical; `XCL_SPLIT_WAVE64=1` forces the split form on AMD so it can be tested there.
- **Pipelines:**
  - A bind that waits marks its build as wanted, so the build skips the queue and the slot (25c6f7e).
  - At most one or two huge compiles at a time (724b9e2).
  - The title's `Sleep` does not count time a bind spends waiting for a build, so the title's 30-tick watchdog doesn't fire (9cdae10).
  - Pipelines an earlier run used are built ahead (7296a8b): skipped dispatches fell from ~2,170 to 94–150.
  - ExecuteIndirect with a pending pipeline is skipped (2d9a9fe): otherwise a root-signature mismatch led to device removal.
- **Memory:**
  - The write watch scans in a few calls (712bbbe). A call costs ~3.5 µs however small; one 2 GiB call ~1.3 ms. Ranges under 64 MiB apart are merged.
  - **GPU-visible memory is write-watched** (e93dc9c): the glyph atlas was uploaded once, empty, and never again, so lowercase letters were missing.
  - Shared-heap places are given a page at a time (1ad0d4a): on NVIDIA, the shared heap ran out and lists read other heaps' descriptors, which showed as flicker.
  - Textures unchanged since upload aren't uploaded again (69f0635).
- **Frame clock:** every event type due at the same deadline fires (956a476). A title that scheduled two at one offset waited forever.

### Leonida

- The remote is gone: `gh api` returns 404.
- Our local clone ends at 3faac557 (2026-11-23), with nothing new.
- Its wave64 "rounds" scheduler stays a design reference for e.

### Security assessment

- **What I did with them:** all foreign code was read as data. The bare mirrors are in the scratchpad, read with `git` and `gh api` only. Nothing was built, run or installed, and nothing was copied into this repo.
- **ztrx's diff against the fork:**
  - It is only the README and `play.ps1`. I read the script: it clears four debug variables, sets the cache and data folders and runs `victory.exe run … --execute`. It is benign.
  - There are no binaries, no `build.rs`, and no new URLs.
  - Its Cargo changes only relax lints.
- **victory-fork:**
  - Its newest commits swap `miniz_oxide` for `zlib-rs`, a widely used crate. That swap is benign.
  - It adds a `std::process::Command::new(dxc)` call. I could not read the call site: the mirror's blob fetch kept failing on TLS. It must be reviewed before any of that code is used.
  - It absorbs whole branches of other people's forks. That is exactly what our trust rule excludes, so it stays a source of ideas unless you decide otherwise (ask 2).

## 2. Three-way map: who has what

"Likely duplicate" means the same fix was written independently. git's patch matching can't confirm these, so each is confirmed during the rebase by reading both sides.

| Topic | Upstream | Fork | XCL | Ours | Take from |
|---|---|---|---|---|---|
| Pipeline prebuild and library | yes (9c58a1c, f821cb9) | prewarm (#1) | yes | early-start list | upstream |
| Shader cache keyed by sources | yes (c47b529) | yes | yes | 4962cdb (likely duplicate) | upstream |
| Copy-back cost | view marks (cd94d35, 940d32f) | hashing on the card | — | measured only | upstream, then re-measure |
| Heap ranges current (hang H1) | c75fdba | similar ("table exhaustion") | — | hypothesis | upstream |
| Raw-address memory | two-level page table | 64 KiB directory | — | — | upstream |
| BVH ray test | 0b35b7f | intersect.rs | — | — | upstream |
| Structured stride per slot (f's biggest gap) | 0b35b7f | — | — | proposed | upstream (old ask 4 superseded) |
| 64-bit image atomics, SGPR spills, descriptor dwords, mesh 9-bit indices | yes | partly | — | our commits (likely duplicates) | upstream; drop ours |
| Compute texture derivatives | ? | "730 compute programs, invalid DXIL" fixed | — | e5ee3ce | ours, unless upstream has it |
| Mesh exports under maxima; mesh `SV_CullPrimitive` | ? | ? | 798ff0d (refusal) | 8ec9285, uncommitted | ours |
| Indirect gate; loop budget per lane; present planes; packed 16-bit targets; adapter choice; `VICTORY_ONLY`/`VICTORY_DISASM` | partly (4ab20fc, 4e60ea3 indirect) | ? | — | 3a17359, f8c134d, 22cb245, e4515c9, 4aa2f9f, 0d91b4b | ours, merged with upstream where both exist |
| `HandleKey`, batch `gpu_write` per shadow, `copy_sparse` | ? | ? | — | uncommitted | ours |
| Parked lane-mask restore | — | yes | — | refusal seen | write ourselves (g) |
| Wave64 as two 32-lane halves | — | approximate forms, barrier form disabled | full design | proposal | write ourselves after XCL's design (e) |
| Late wait / relay | — | yes | ECL-X / frame events | proposal (old b "overlap") | write ourselves |
| DXIL clean-up passes | — | yes | DXC -O1 for large HLSL | — | write ourselves |
| Write watch in few calls; GPU-visible memory watched; shared heap a page at a time | partly? | partly | yes | — | check upstream, else write ourselves |

## 3. The plan

Each step has a check, and the next step starts only after it.

**Machine rules for every step:**
- Every run writes nothing to the game folder; compare the fingerprint after each run.
- Use `-j 4` for cargo.
- Use the run-probe harness.
- Check the "host adapter" line: it must name the RTX 4060.

### Step 0: Save our state (needs your approval)

- **What:** commit the verified uncommitted work (batch `gpu_write` per shadow, `copy_sparse`, `HandleKey`, mesh cull, test) and the handoff; push `victory-world-30fps`; tag it `pre-rebase-2026-11-25`.
- **Why:** the rebase rewrites history. The old branch must stay intact as the fallback, both on GitHub and locally.

### Step 1: A new branch from upstream/main with our unique work replayed

- **What:**
  1. Create `victory-world-30fps-v2` from `upstream/main`.
  2. Cherry-pick our commits in order. For each one, first check whether upstream already has the fix:
     - If it does, drop ours; if ours covers a case upstream misses, keep only that delta.
     - If it doesn't, keep ours.
- **Conflicts expected** (from `git merge-tree`): README, translate.rs, descriptor/host.rs, draw.rs, host.rs, shader_cache.rs, texture.rs, emit/{container,host,mod,shader}.rs, lower.rs, valu_tests.rs.
- **Why:**
  - Upstream is the only line we may take code from.
  - Its last day solves or attacks a, b, d and much of f.
  - Rebuilding those ourselves would be slower and would never merge back.
  - A new branch (rather than rebasing in place) keeps the old one runnable for comparisons.
- **Check:**
  - `cargo test --workspace -j 4` passes, including the hardware tests on the 4060.
  - A census of the shader pack with `translate` has no fewer validated programs than either parent. A regression points to a wrongly resolved conflict.

### Step 2: Warm caches once, in one shared folder (d)

- **What:**
  1. Point `run-probe.ps1` at one cache folder outside each run, for example `%LOCALAPPDATA%\victory-lab\cache`, never the game folder.
  2. Run upstream's command once:

     `victory.exe translate --cache --pipelines <game>\shaders.rpf <exe> <game>\XboxBvhBuilder.dll <game>\amd_fidelityfx_upscaler_xbox.dll`

     It reads the game folder and writes only the cache: about 1.3 GB and 20–40 min on this CPU.
- **Wave size check (done):** upstream sets `WaveSize` from the console program alone (`lower.rs:531`, `wave = if native.wave64 {64} else {32}`), not from the device. So a prebuild on the 4060 makes exactly the pipelines the runtime asks for. Wave64 programs that need cross-lane order are refused at creation, the refusal is cached, and they are e's problem. The fork's `VICTORY_HOST_MAX_WAVE` isn't needed for upstream's translator.
- **Also (from the first proposal, still needed):**
  - Show waits and drop reasons on the fps line: being made, refused, not translated.
  - Fix the double-counted copy time.
- **Not now:**
  - The old "pipelines asked for this frame jump the queue" (item 4) stays conditional, and is likely superseded.
  - With a prebuilt library, nearly nothing should be "being made". Revisit it only if, after a warm run, the drop line still shows many "being made".
- **Why:**
  - Proven: every run started cold, and ~900 pipelines were always "being made" while 800–4,200 draws per frame were dropped.
  - Until this is fixed, every copy and GPU number describes a different workload from the real one.
- **Check:**
  - The second run's drop line is down to refused or untranslated programs only.
  - The library reports loads, not builds.
  - No 30 s watchdog exit.

### Step 3: Re-measure the world on the new base (a, b, c)

- **What:** run ss13's route on step 1's build with step 2's warm cache. Record:
  - frame time, copy time and GPU time;
  - whether the hang happens, using upstream's TDR log line and `GetDeviceRemovedReason`.
- **Decision rules:**
  - **Hang gone:** drop the old §3 experiment, keep only the index clamp as a safety net if a reason still points at heap indices. **Hang still there:** run the old §3 deciding experiment on the new base.
  - **Copy time:**
    - well under 100 ms per frame: the old §4 work shrinks to the overlap part (step 6);
    - otherwise: §4's elimination measurements on the new base.
  - **GPU time:** profile the warm run, as in the old §5.
- **Why:** upstream's view marks (b), heap-range currency and null views (a) target exactly what we measured. Building our own fixes before seeing theirs run on our GPU risks duplicating them.

### Step 4: Lighting, the "black shadows" problem (g, new)

No public build lights anything in shadow. The title's indirect light and its lamp and sun shadows are its own compute programs: ray tracing in software over its own BVH. Without them, everything the sun doesn't hit directly is black.

- **4a. Parked lane-mask restore.**
  - Our census refuses `CS_IrradianceCache_Shade` at 0x10a78 with "a lane-mask join of an untracked mask". The fork describes and fixed the same refusal. `ShadeSecondary`, `ShadeCopy` and `ShadeVerify` already validate here.
  - Write the fix ourselves from the description:
    - pair two `v_readlane_b32` restores a few instructions apart, when nothing between them touches the second register, writes the first, or saves into the lanes;
    - let M0-relative reads (`v_movrels`) keep parked lanes.
  - The fork measured ~40 ms of GPU time per frame for this, so measure ours too.
- **4b. Recount after the rebase.** Our census shows many probe programs refusing with "an address not from a root parameter". Upstream's raw-address page table (27c3dc9, 0f63757) likely covers them. Recount before writing anything.
- **4c. The BVH builder and trace programs.**
  - Use upstream's ray test (0b35b7f).
  - Adopt the fork's all-or-none rule for the builder as an idea: a half-working builder corrupts title memory.
  - Off by default until it runs end to end.
- **Why now, before speed:** you asked for clean graphics, and this is the biggest visible defect. 4a is small and well understood. 4c is big and stays behind a switch.
- **Check:**
  - Capture frames of the same scene with and without the change (`VICTORY_CAPTURE`): shadowed areas are no longer flat black.
  - The GPU time cost is recorded.

### Step 5: Wave64 as two 32-lane halves (e)

The design comes from XCL's write-up and Leonida's scheduler; we write our own code.

- **Exchange:**
  - The two halves exchange through groupshared memory only where a value must be the whole wave's: masks, lane reads, `ds_bpermute`, GLC scalar loads.
  - DPP, swizzle and `permlane16` stay in a half.
  - Both halves hold the full EXEC/VCC and SGPRs, so control flow is the same in both.
- **Bounded spin, not barriers, by default.** Both XCL (066ae5b) and the fork (e9765a7) lost or risked the device with group barriers inside branches.
- **Thread IDs:** console thread IDs are taken in arrival order, because NVIDIA doesn't group consecutive IDs in 3-D groups.
- **Stage 1:** single-wave groups. 94 of our 122 refused programs are single-wave.
- **Stage 2:** several waves per group, with paired slots.
- **Stage 3:** classify VS/PS by whether a 32-lane cut changes them.
- **Why:** proven: NVIDIA refuses `[WaveSize(64)]`, and 122 of 123 variants need cross-lane order. XCL ran a whole retail title this way on 32-lane GPUs. And upstream has no plans for it ("needs a GPU with 64-lane waves").
- **Check:**
  - The instruction tests run in split form and compare against native wave64 output, with a test that fails when the two halves end with different SGPRs.
  - The census shows the 122 programs translating.
  - No device loss in a 10-minute world run.

### Step 6: CPU and GPU overlap (b, part 2)

- **What:** submit before waiting on the GPU, and land the results on a relay thread as each submission finishes. This is the fork's idea. It must only read title memory that a submission still running doesn't write; upstream's view marks name exactly those pages.
- **Also from XCL, if upstream lacks them:**
  - the write watch in a few merged calls;
  - GPU-visible memory registered for the write watch (a correctness fix: text and atlases that the CPU fills);
  - the shared heap given a page at a time (NVIDIA flicker);
  - unchanged textures not uploaded again.
- **Why:**
  - Today the GPU sits idle while the CPU copies and records, and the CPU sits idle while the GPU runs.
  - The fork measured GPU idle falling from ~136 to ~30 ms per frame. That is the largest single win any public build reports on NVIDIA.
- **Check:** GPU idle per frame and frame time, compared against step 3's numbers on the same route.

### Step 7: GPU time (c)

- **7a. Clean-up passes over finished DXIL.**
  - Dead code, folding, value reuse, loads within a block. They are part of the cache key and off with a switch.
  - The fork measured −43 % instructions and ~20 ms per frame.
  - Write our own; check each pass with the existing validator and the instruction tests.
- **7b. Read-mostly title buffers copied to VRAM**, behind a switch: our old §5 experiment.
  - PCIe 4.0 x8 here gives ~13 GB/s, against ~250 GB/s for VRAM.
  - The fork went the same way ("large title buffers get a copy in card memory").
- **Check:** `VICTORY_GPU_TIMING` per list on a warm run, before and after each change.

### Step 8: Research only, in-place title memory

- **The idea:** open title memory as a D3D12 heap (`OpenExistingHeapFromAddress1` or `FromFileMapping`), so that buffers need no copy. This is the only path towards a console-like single memory, and so the long-term route to large frame-time cuts.
- **Its known traps (documented by the fork):**
  - tile mappings into such a heap remove the device;
  - the title cannot free or protect the pages while they are open;
  - opening locks the pages;
  - VIctory's page pool is made `SEC_RESERVE`, which the API refuses.
- **The deliverable:** a probe and a written finding, not a change.

### Step 9: Remaining refusals and pacing (f, then 30 fps)

- **Refusals:** recount after steps 1, 4 and 5, then take the largest remaining groups.
  - Upstream already covers `image_gather4_l` and the stride table.
  - Our `image_sample_d_cl` and the deferred-error offset (old §7.3 step 1) remain.
- **Pacing, once the frame time is near 33 ms:**
  - Check VIctory's frame clock for the "two event types at one deadline" bug XCL fixed.
  - Check that pipeline waits can't trip the title's own watchdog (XCL's `Sleep` hold).

## 4. Status of the first proposal's items

| Item | Status now |
|---|---|
| d: one shared cache folder | **still needed** (harness change). Use upstream's `translate --cache --pipelines` to fill it. |
| d: waits and drop reasons on the fps line; copy double-count fix | **still needed** |
| d: front queue for pipelines the current frame asks for | **conditional, likely superseded** by the prebuild. Only if the warm-run "being made" count stays high. |
| a: deciding experiment on heap shadows | **re-check first**: upstream c75fdba / 86a5ed6 / 4051149 / d71c388 target it |
| b: measure by elimination; write-watch cost | **re-check first**: upstream view marks and 6d021fe; XCL's few-call scan if still slow |
| b: overlap CPU and GPU | **still needed**, now step 6 |
| c: profile; VRAM copies | **still needed**, step 7 |
| e: two 32-lane halves | **still needed**, step 5, design refined by XCL |
| f: `image_gather4_l` | **superseded** (upstream 0b35b7f) |
| f: raw views plus a per-slot table (old ask 4) | **superseded** by upstream's per-slot stride table; recount |
| f: deferred-error offset; `image_sample_d_cl` | **still needed** |

## 5. Milestones (on this laptop: RTX 4060 Laptop, PCIe 4.0 x8)

| Milestone | Condition | Expected after |
|---|---|---|
| M1 | world runs 10 min with no hang or device loss; warm cache; drops only from refusals | steps 1–3 |
| M2 | shadowed areas lit (not black); frame captures compared | step 4 |
| M3 | ≤ 500 ms per frame (the fork's default level, on a faster desktop GPU) | steps 3, 5, 6 |
| M4 | ≤ 200 ms per frame | steps 6, 7 |
| M5 | locked 33 ms per frame | needs step 8 or similar structural work; no public build is near it |

## 6. What I need from you

1. **Base:** OK to build `victory-world-30fps-v2` from upstream/main and replay our unique work, keeping the old branch as it is?
2. **Fork code:** default is ideas only. victory-fork is a fork that also absorbs other forks' branches, and ztrx is a repackage of it. Do you want to allow code from fxceless's own commits after review, or keep the rule as is? I recommend keeping it.
   2b. **Fork baseline run:** do you want me to build and run victory-fork once on this laptop as a measured baseline? It would use its own cache and data folders, with the game folder read only and checked by its fingerprint afterwards. My recommendation is yes, but only after you agree to running code that has had only the scan described in §1.
3. **Step 0:** approve committing the verified uncommitted work and the handoff, pushing `victory-world-30fps`, and tagging it.
4. **Prebuild:** OK to spend 20–40 min of CPU and ~1.3 GB on `translate --cache --pipelines`, into a shared cache folder outside the game folder?
5. **Priority:** lighting (step 4) before speed (steps 5–7), as proposed, or speed first?

## Sources

- Upstream: `git log ffe310e..upstream/main`; commit bodies; `upstream/main:README.md`; `lower.rs:531`.
- victory-fork: GitHub API (repo, branches, commits, compare, README, `docs/fork-ray-tracing.md`, `wave.rs`, `inplace.rs` docs); a ztrx mirror for the diff against ffe310e.
- XCL: commit bodies of 3ecf4cf, 066ae5b, ba905cf, dfc5578, 956a476, 25c6f7e, 724b9e2, 5fab112, 7296a8b, 9cdae10, 2d9a9fe, fe7d70b, b5d6322, e93dc9c, 712bbbe, 1ad0d4a, c1a9b52.
- Our census: `continuation-final-census.txt` and `wave-review-cs-8f7f5c3.txt` in the lab folder.
- Scratch copies (temporary): upstream-log.txt, ztrx-log.txt, ztrx-stat.txt, xcl-key-commits.txt, vfork-README.md in the session scratchpad.
