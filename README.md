# VIctory fork for NVIDIA GeForce (work in progress)

This is a fork of [VIctory](https://github.com/Marvisak/VIctory) by Marvisak
and its contributors. **The work is on branch
[`victory-world-30fps-v2`](https://github.com/Ep1cGaimer/VIctory/tree/victory-world-30fps-v2)**
(this `main` branch follows upstream): upstream `main` (`940d32f`) plus fixes found while running the game's world on an
**NVIDIA** laptop GPU. It contains no game files, code or assets; you need
your own copy of the game.

## State of this fork (2026-11-25)

**Not playable yet: well under 1 to about 2 frames per second in the world.**
Tested on one machine only: Windows 11, **NVIDIA GeForce RTX 4060 Laptop
GPU 8 GB**, 15.3 GB RAM (an AMD Radeon 780M integrated GPU is present too;
VIctory runs on the NVIDIA card, check the "host adapter" log line), Series X
profile.

| What | State on that machine |
|------|-----------------------|
| Boot, menus | work, 30 fps |
| Loading into the world | works (lazy host textures keep it inside 8 GB) |
| The world | reached and drawn; frames take 0.4 to several seconds |
| GPU time per heavy frame | about 1.7 s by default; about 0.43 s with `VICTORY_SHADER_LOOP_LIMIT=256` |
| CPU time per frame | 0.3 to 6 s, mostly copies of title memory (200 to 400 MB a frame) |
| Memory | grows to 20+ GB committed over minutes |

### Why NVIDIA is slower than AMD here

The console runs most compute programs in 64-lane waves. NVIDIA cards only
run 32-lane waves, so about 130 of the game's compute pipelines are refused
on this card (AMD cards run them natively). Their outputs are never
written, and the programs that read them (wind, sky and fog scattering) loop
on garbage counts until the translator's loop limit stops them: about
1.3 s of GPU time a frame. Work in progress ports a way to run these
programs as two 32-lane halves (`VICTORY_WAVE_HALVES=1`, off by default: with
it the game currently stalls at the account picker). See
[docs/handoff-victory-world-30fps.md](https://github.com/Ep1cGaimer/VIctory/blob/victory-world-30fps-v2/docs/handoff-victory-world-30fps.md)
for measurements and next steps.

### What this fork changes over upstream (highlights)

- Translated shaders clamp heap indices to the host's real view heap
  (1,000,000 on NVIDIA) and view addresses cover dynamic slots and BVHs
  (a device hang entering the world).
- The ray-tracing structure builder runs all or none, off by default
  (`VICTORY_BVH_BUILDER=1`); with some stages refused it wrote garbage into
  title memory.
- Host textures of textures only read are made on first use (loading no
  longer runs out of video memory on 8 GB).
- Depth-plane copies the host cannot do are skipped; watchdog-triggering log
  floods removed.
- Diagnostics: `VICTORY_GPU_TIMING=1` (slow commands by pipeline),
  `VICTORY_SHADER_SAVE=<dir>` (shaders as given and as translated),
  `VICTORY_SKIP_PIPELINES=<hash,...>`.

### Credits

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors is the base of everything here (GPL-3.0-or-later; so is this
  fork).
- The BVH builder gate and the wave64-in-two-halves lowering are ported from
  [Zettrox/ztrx](https://github.com/Zettrox/ztrx) `main` (GPL-3.0-or-later).
- Developed with [Claude Code](https://claude.com/claude-code).

Everything below this line is upstream VIctory's own README; its status
section describes upstream on upstream's AMD test machine, not this fork.

---

# VIctory

Runs Grand Theft Auto VI (Xbox Series X|S) natively on Windows, from files
the user supplies.

The title's x86-64 code runs unchanged inside our process; VIctory supplies
the platform underneath it and converts the title's data, ahead of time where
it can and on first use where it must (shaders). See
[docs/architecture.md](docs/architecture.md).

## Status (2026-11-23)

**Not playable yet.** The title boots, runs its menus at its own 30 fps and
loads the world, where rendering fails.

| Stage | State |
|-------|-------|
| Boot, first-boot screens (accessibility, language, settings, legal) | works, 30 fps |
| Intro video with sound | works |
| Start screen, profile and saves | works |
| Loading into the world (about 50 s) | works |
| The world (first mission's intro) | reached and runs with indirect draws; its title card renders, the scene after it is still dark; 0.5-1.8 s per frame |

Tested on one machine: Windows 11, AMD RX 6700 XT.

> **Warning.** Entering the world on a real GPU can currently hang it, and
> Windows then resets the display driver (every monitor goes black for a
> moment). To look further, use a diagnostics build with
> `VICTORY_D3D12_WARP=1`, which renders on the software adapter.

What each part does today and what it still needs:

| Subsystem | Today | Needs |
|-----------|-------|-------|
| Loader | Runs the title's executable and its DLLs in our process; crash reports name the faulting code and are kept in `crashes.log` | – |
| Runtime services | One signed-in offline user, licences, packages, saves that survive a kill at any step, text entry dialog | Speech is silent; the dialog and relaunch are untested live |
| Network | Offline by design: the title cannot reach the network | – |
| Files and streaming | Archives and streamed data read, zlib and BCPack decoded in parallel | Loading is limited by writes into title memory |
| Input | XInput pads, keyboard, keyboard stand-in pad | Trigger rumble |
| Audio | Menu, video and loading sound; XMA voices and Opus streams decode | Reverb is silent; Opus results unverified against the title |
| GPU: menus | Draws, compute and presentation on host D3D12; only changed title memory is copied | Frame pacing drifts on one screen in release builds |
| GPU: world | Mesh pipelines, shader-written buffers, descriptor lifetimes, indirect execution (arguments clamped), pipelines made by worker threads and kept on disk | Most of the scene is still missing (untranslated shaders drop draws); frames take seconds; real queries, depth bounds, HDR and scaling are missing |
| Shaders | Console GPU code recompiled to DXIL at first use, cached on disk. Menus: all. World: mesh 8,191 of 8,506, vertex/pixel 10,617 of 12,346, compute 1,643 of 2,539 | 64-bit image atomics, merged geometry stages, the ray-tracing builder's shaders; some compute programs need a GPU with 64-lane waves |
| Textures | Console tiled layouts and detiling for the modes the title uses | Render-target metadata planes |
| Series S mode | Runs through the menus to the loading screen | The world, as above |

## Getting started

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with
shader model 6.6 (and mesh shaders for the world), Rust (pinned in
`rust-toolchain.toml`).

The title also needs several GiB of available Windows commit at startup.
Check **Committed** under **Task Manager → Performance → Memory**. If
`crashes.log` reports Windows error 1455, close other applications or increase
the Windows paging file size, then retry.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo test --workspace                   # needs no game data
cargo run -p victory -- config           # check your paths
```

Run the title:

```powershell
cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log`
are kept under the data folder (default `%LOCALAPPDATA%\victory\data`), derived
data under the cache folder.

Input: XInput pads. With none connected the keyboard is a pad while the
title's window has focus:

| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

## Commands

```
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
victory config
```

Logging: `VICTORY_LOG=victory_loader=trace,info` (a release build logs
info and above).

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

| Variable | Effect |
|----------|--------|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_WATCH=<rva>,...` | logs a title u32 when it changes |
| `VICTORY_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `VICTORY_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `VICTORY_CAPTURE=<dir>` | saves presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash |
| `VICTORY_GPU_TIMING=1` | with the above, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `VICTORY_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | translated shader loops stop after `n` iterations per lane instead of 2^20 (own shader cache entries); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `VICTORY_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
