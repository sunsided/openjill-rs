# Epic 1 Subplan: Workspace and Foundation

## Inspected OpenJill modules

From `PORT.md`, this phase references the Java modules below to map initial Rust crates:

- Runtime-critical references: `abstractfile*`, `dma-file*`, `sha-file*`, `jn-file*`, `vcl-file*`, `cfg-file*`, `openjill-core-api`, `openjill-core`, `open-jill-object-background`, and `OpenJill/src/main/resources`.
- Lower-priority/reference modules: `simplegame`, extractor modules, `openjill-object-manager`, `openjill-background-manager`, and `openjill-cache-manager`.

## Crates/files to touch in this epic

- Root workspace files:
  - `Cargo.toml` (workspace members, resolver, shared package/dependencies)
  - `.cargo/config.toml` (shared build/test/lint aliases)
  - `.gitignore` (ignore proprietary data paths)
- New crate skeletons:
  - `crates/openjill-data`
  - `crates/openjill-core`
  - `crates/openjill-game`
  - `crates/openjill-render`
  - `crates/openjill-audio`
  - `crates/openjill-cli`
  - `tools/openjill-dump`
- Port planning docs location:
  - `docs/port/`
  - `docs/port/01-workspace-foundation.md`

## Interfaces added (stub-only)

- Crate-level placeholder types to establish dependency direction:
  - `openjill_data::DataDirectory`
  - `openjill_core::CoreState`
  - `openjill_game::GameApp`
  - `openjill_render::Renderer`
  - `openjill_audio::AudioBackend`
- CLI command shape in `openjill-rs`:
  - `run`
  - `data verify`
  - `dump`

All commands are explicitly non-gameplay stubs and only validate CLI/workspace wiring.

## Tests in this epic

- Add focused CLI parser tests in `crates/openjill-cli/src/main.rs` to verify command names are accepted:
  - parses `run`
  - parses `data verify`
  - parses `dump`
- Validate with targeted crate tests and workspace build/lint checks.

## Risks and mitigations

- Risk: Accidentally implying gameplay/data parsers exist.
  - Mitigation: Keep placeholder types and explicit stub output messages; no parser/renderer/audio/gameplay implementation.
- Risk: Workspace command drift across contributors.
  - Mitigation: Provide shared cargo aliases for build/test/lint at workspace root.
- Risk: Proprietary game data accidentally committed.
  - Mitigation: Add `data/original/` and `data/extracted/` to `.gitignore`.

## Handoff notes

- Next epic should implement real `openjill-data` parser modules first (DMA/VCL/CFG) with synthetic fixtures.
- Keep `openjill-core` deterministic and testable without rendering/audio backends.
- Extend `openjill-cli` behavior incrementally, preserving current command names and avoiding claims of implemented gameplay until corresponding crates are functional.
