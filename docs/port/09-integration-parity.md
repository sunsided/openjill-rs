# Epic 9 Subplan: Integration, Parity, and Release Readiness

Decision-complete subplan for issue #9. Goal: confirm the full port runs episode
1 from original data, document parity gaps and run instructions, and verify that
every prior epic's acceptance criteria still hold together.

This epic is mostly **integration + documentation**: the runtime pieces already
exist (CLI `run`, `data verify`, saves, audio). The work is proving they hold
together, replacing the stale README, and writing the user-facing run guide and
known-limitations list.

## Current state (already satisfies parts of acceptance)

- `openjill data verify --data-dir <path>` exists and checks the required
  episode-1 files (`required_files`: `JILL.DMA`, the episode VCL/CFG/SHA, the
  intro JN, the map JN), parsing each to confirm it is well-formed.
- `openjill run --data-dir <path> [--episode N]` boots `GameApp`, opens a
  `winit` window, and presents via `wgpu` (`openjill-render`).
- Saves / high scores persist to the per-user runtime dir (epic 7).
- Sound events route through `rodio`; the player cues play the original VCL
  sounds, unsupported audio (music, unmapped cues) is documented (epic 8, #209).
- `cargo test --workspace` passes without proprietary data; real-data
  integration tests self-skip unless `OPENJILL_DATA_DIR` (or the default
  `data/original/JILL1`) is present.

## Gaps this epic closes

1. **README is the old Java OpenJill readme** (Maven `mvn` build, Java module
   list) - wrong for the Rust port and the single biggest doc gap.
2. **No full-game smoke test** that boots the orchestrator from real data and
   drives it through menu -> map -> a level over many ticks.
3. **No consolidated parity-gaps / known-limitations** document.
4. No single **run guide** covering data placement, CLI, controls, troubleshooting.

## Key decisions (decision-complete)

### 1. Run / usage docs (rewrite README)

Replace `README.md` with a Rust-port readme. Sections: what it is + scope
(episode 1; bring-your-own original data), data placement
(`--data-dir`/`OPENJILL_DATA_DIR`/default `data/original/JILL1`), build
(`cargo build --release`), `data verify`, `run`, the control map, save/high-score
location, audio + the NOISE toggle, and troubleshooting (missing files, no audio
device, no GPU). Link to `docs/port/` for format/porting detail. The original
data is **never** shipped or committed.

Controls (from `KEY_MAP`): Left/Right arrows move; Up climbs; Down ducks;
Space/Shift jump; Ctrl/Alt throw; Tab/Backspace cycle inventory; Esc opens the
in-level "really quit?" / dismisses menus; `Q` quits; `N` toggles sound;
`T` toggles turtle (slow-mo); `S`/`R` save/restore.

### 2. Full-game smoke test (gated)

Add a real-data integration test that resolves the data dir the same way the
existing real-data tests do (`OPENJILL_DATA_DIR` override, else the default
`data/original/JILL1`) and self-skips when neither is present. It constructs the
orchestrator from that dir, then drives a few hundred ticks across a
representative path - start menu -> PLAY -> world map -> enter a level - feeding
scripted input, asserting it never panics and keeps producing non-empty frames. This is a **smoke test, not a scripted full playthrough**: a
deterministic clear of every level needs authored input sequences out of scope
here; the smoke test guards integration (asset load + screen transitions + tick
loop) end to end.

### 3. Parity gaps / known limitations

A `## Known limitations` section in the README (and/or this doc) enumerating:
episodes 2/3 not yet supported; background music (`*.DDT`, CMF/OPL2) not played;
the sound event->slot mapping is partial pending EXE RE (#209); save files are
byte-faithful to DOS Jill but validated by round-trip (no DOS save fixtures);
any residual gameplay quirks. Each gap names its tracking issue or doc.

### 4. OpenJill behavior comparison

Document where parity is verified **structurally** rather than by live
side-by-side run: the per-entity `JnObject` round-trip, the DMA/SHA/VCL/CFG
real-data parse tests, and the Java-reference constants cited throughout the
entity ports. A live A/B against the Java jar is **out of scope** (needs a JVM +
the jar); note it as optional manual validation.

### 5. `data verify` scope

`data verify` already discovers and **lists** the additional `*.JN1` level files
(beyond intro/map) in its report, but keeps them out of the fixed
`required_files` set, so they do not affect the pass/fail `ok()` result. Keep
this behavior; document in the run guide that the level JNs appear as
informational "discovered" entries while only the core files gate the result.

### 6. Acceptance evidence

Record the exact commands + expected output for each acceptance criterion (see
below) in the run guide, so the epic can be signed off reproducibly.

## Tests and acceptance checks

| Criterion | Evidence |
|-----------|----------|
| Full episode 1 completes from original data | manual playtest + the gated smoke test (boot -> menu -> map -> level, no panic) |
| `cargo test` passes without proprietary data | `cargo test --workspace` (real-data tests self-skip) |
| Optional real-data checks pass with `OPENJILL_DATA_DIR` | the `*_original_data.rs` suite + new smoke test |
| `data verify` reports required files present | `openjill data verify --data-dir <path>` exit 0 + per-file OK |
| `run` opens a window via wgpu | `openjill run --data-dir <path>` (manual) |
| Saves / high scores persist | epic 7 tests + manual save/restore |
| Audio routes through rodio, unsupported documented | epic 8 + #209 + the known-limitations section |
| Run instructions complete | the rewritten README |

## Child issues (suggested split)

1. **This subplan.**
2. **Rust-port README + run guide** - replace the Java readme; controls, data
   placement, CLI, troubleshooting, known limitations.
3. **Full-game smoke test** - gated integration test, boot -> menu -> map ->
   level over many ticks.
4. **Parity-gaps / known-limitations** - consolidate (may fold into the README
   PR if small) and cross-link tracking issues.

## Risks and handoff notes

- **No automated full playthrough**: the smoke test guards integration, not
  level-completion correctness; full clears stay manual.
- **Live OpenJill A/B is out of scope** (needs the Java jar + JVM); parity is
  argued structurally + by manual playtest.
- **Original data is never committed**; all real-data checks self-skip without it.
- Episodes 2/3, background music, and the full sound mapping (#209) remain known
  gaps, not epic-9 blockers.
