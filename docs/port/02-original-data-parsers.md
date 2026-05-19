# Epic 2 Subplan: Original Data Parsers

## Inspected OpenJill modules

From `PORT.md` and the Java sources, this phase maps the original data readers
into the Rust `openjill-data` crate:

- `abstractfile*`: byte-oriented reader behavior and DOS-compatible file access
  patterns.
- `dma-file` and `dma-file-api`: `JILL.DMA` map-code to tile/picture metadata.
- `vcl-file` and `vcl-file-api`: `JILL1.VCL` text table parsing. Sound data is
  present in the file but stays out of scope for this phase.
- `cfg-file` and `cfg-file-api`: `JILL1.CFG` high scores, save names, and setup
  data.
- `sha-file` and `sha-file-api`: `JILL1.SHA` image/tile/font data and color-map
  metadata.
- `jn-file` and `jn-file-api`: `INTRO.JN1`, `MAP.JN1`, level/save background
  grids, object records, save data, and string stacks.
- Runtime usage references in `openjill-core` and `OpenJill/src/main/resources`
  for episode 1 file names, the `JN1` save prefix, and later handoff needs.

Extractor, editor, render, audio, gameplay, and cache modules were reference
material only. They did not drive runtime dependencies in `openjill-data`.

## Rust modules and types added

All parser implementation lives in `crates/openjill-data`:

- `lib.rs`
  - `DataDirectory` for case-insensitive original-data file resolution.
  - Shared byte/string helpers used by parser modules.
  - Filesystem-facing errors for missing paths, non-directory roots, and failed
    opens.
- `dma.rs`
  - `DmaFile`, `DmaEntry`, and `DmaParseError`.
  - Preserves raw tile metadata, masks tileset bits the same way the Java reader
    does, and keeps source offsets for parsed entries.
- `vcl.rs`
  - `VclFile`, `VclTextEntry`, and `VclParseError`.
  - Parses only the text-entry table after the sound-entry header area.
- `cfg.rs`
  - `CfgFile`, high-score records, save-slot records, setup/config fields, and
    `CfgParseError`.
  - Reads the fixed-size episode config data and derives save filenames from the
    supplied save prefix such as `JN1`.
- `sha.rs`
  - `ShaFile`, tilesets, images, indexed pixel data, color-map metadata, and
    `ShaParseError`.
  - Preserves indexed image data for later renderer-owned palette conversion.
- `jn.rs`
  - `JnFile`, background cells, object records, save data, string-stack entries,
    and `JnParseError`.
  - Uses one parser path for intro, map, level, and save-shaped `*.JN1` data.

`openjill-data` remains independent from render, audio, game runtime, gameplay,
`wgpu`, `winit`, and `rodio`.

## Parser error model

Each parser owns a domain-specific error enum instead of returning bare I/O
errors. Parse failures include:

- the file format or parser domain (`DMA`, `VCL`, `CFG`, `SHA`, or `JN`);
- the field being read;
- the byte offset where the parser expected that field;
- indexed context when useful, such as object, string, tile, or table entry
  indexes;
- the underlying I/O or validation failure.

This keeps CLI and future tooling errors actionable without coupling parser
types to CLI presentation. The parser APIs also preserve enough source metadata
to let dump tooling report where entries came from.

## Synthetic fixture strategy

Committed tests use small synthetic byte buffers, not slices of proprietary
game files. Fixtures are constructed inside unit tests so the intended binary
layout is visible next to the assertions.

The synthetic tests cover:

- successful parsing of representative records for every format;
- malformed/truncated input with offset-aware error reporting;
- important format invariants such as fixed counts, sorted offsets, string
  decoding, tile/palette metadata, and save-prefix-derived names;
- case-insensitive data-directory resolution using temporary files.

No original game bytes, extracted assets, generated dumps, screenshots, or raw
tile exports are committed.

## Optional real-data checks

Each parser has a real-data integration test under
`crates/openjill-data/tests/`:

- `dma_original_data.rs` for `JILL.DMA`;
- `vcl_original_data.rs` for `JILL1.VCL`;
- `cfg_original_data.rs` for `JILL1.CFG`;
- `sha_original_data.rs` for `JILL1.SHA`;
- `jn_original_data.rs` for `INTRO.JN1`, `MAP.JN1`, and discovered episode 1
  `*.JN1` files.

The tests resolve the data directory from `OPENJILL_DATA_DIR` first, then fall
back to workspace-relative `data/original/JILL1`. If neither exists, they print
a skip message and return successfully so CI can pass without proprietary data.

When data is present, the tests open files through `DataDirectory::open_reader`
so host capitalization does not matter. They assert structural invariants such
as non-empty parsed content, expected fixed counts, monotonic offsets, in-range
metadata, valid save-prefix names, and required episode 1 file coverage.

## Scope boundaries

This phase deliberately excludes:

- rendering, RGBA conversion, atlas generation, or framebuffer output;
- audio playback or VCL sound decoding;
- gameplay behavior, collisions, entities, level transitions, or save restore;
- high-score, config, or save-file mutation;
- CLI `data verify` behavior and dump-tool output beyond parser APIs.

## Handoff notes

- Asset tooling can build `data verify` and dump commands directly on the
  parser APIs and `DataDirectory`; it should keep generated dumps out of tracked
  source unless they are synthetic.
- Rendering should consume `ShaFile` indexed image data and palette/color-map
  metadata at the render boundary instead of adding color conversion to
  `openjill-data`.
- Runtime/core loading can compose `DmaFile`, `ShaFile`, `VclFile`, `CfgFile`,
  and `JnFile` by episode file names while keeping gameplay state separate from
  parser records.
- Gameplay can rely on parsed JN background/object/string/save structures, but
  collision rules, entity behavior, scrolling, and transitions belong to later
  core/gameplay epics.
- Save/config work should add writing through separate APIs after read behavior
  is proven; original `JILL1.CFG` and `JN1SAVE.*` files should not be mutated by
  parser tests.
- Audio should investigate the skipped VCL sound region in a dedicated phase
  and document unsupported formats before wiring `rodio`.

## Risks and mitigations

- Risk: Parser code grows runtime behavior.
  - Mitigation: Keep `openjill-data` dependency-free from runtime/render/audio
    crates and expose parsed records only.
- Risk: Original data leaks into the repository.
  - Mitigation: Use synthetic fixtures for committed tests and keep real-data
    tests gated by local directories.
- Risk: Error messages become too vague for CLI users.
  - Mitigation: Preserve parser domain, field names, indexes, offsets, and
    underlying causes in parser error types.
- Risk: Case differences across filesystems break real-data checks.
  - Mitigation: Open all original files through case-insensitive
    `DataDirectory` resolution.

## Completion status

Epic 2 parser implementation is complete as of this document: all child parser
issues are resolved, the parser modules are present in `openjill-data`, and
optional real-data integration tests cover the required episode 1 files without
requiring proprietary data in CI.
