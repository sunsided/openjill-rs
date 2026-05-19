# Epic 3 Subplan: Asset Verification and Debug Dump Tools

## Inspected OpenJill modules and Rust crates

This phase builds on the file-format work from `docs/port/02-original-data-parsers.md`
and uses the Java extractor modules only as behavior references:

- `dma-file`, `dma-file-api`, and `dma-file-extractor` for `JILL.DMA` table
  metadata and flag interpretation.
- `vcl-file`, `vcl-file-api`, and VCL extractor references for decoded
  `JILL1.VCL` text entries. VCL sound data remains out of scope.
- `sha-file`, `sha-file-api`, and `sha-file-extractor` for `JILL1.SHA`
  header entries, tilesets, image dimensions, indexed pixels, and color-map
  metadata.
- `jn-file`, `jn-file-api`, and `jn-file-extractor` for `INTRO.JN1`,
  `MAP.JN1`, level/save-shaped `*.JN1` background, object, save-data, and
  string-stack metadata.
- `openjill-core` and `OpenJill/src/main/resources` only for episode 1 file
  names and later renderer/gameplay handoff needs.

The Rust implementation should inspect and touch these areas:

- `crates/openjill-data`: parser APIs, `DataDirectory`, case-insensitive file
  resolution, file discovery helpers if needed, and checksum helpers if they
  belong below the CLI.
- `crates/openjill-cli`: user-facing `openjill-rs data verify` and
  `openjill-rs dump ...` commands.
- `tools/openjill-dump`: developer convenience binary only. The canonical
  command shape for this epic is `openjill-rs`; `openjill-dump` may either
  delegate to shared dump code if a small shared module emerges or stay a thin
  stub that points developers at `openjill-rs dump`. If `openjill-rs dump` is functional,
  there is no need for an additional `tools/openjill-dump` binary and it can be removed.
- `docs/port/03-asset-debug-tools.md`: this decision-complete subplan.

Do not add dependencies from `openjill-data` to `openjill-cli`,
`openjill-core`, `openjill-game`, `openjill-render`, `openjill-audio`, `wgpu`,
`winit`, or `rodio`.

## Command ownership and shared behavior

`openjill-rs` owns all user-facing asset tooling in this epic. The commands are:

```text
openjill-rs data verify [--data-dir <path>] [--format text|json]
openjill-rs dump dma [--data-dir <path>] [--output <path>] [--format json] [--force]
openjill-rs dump vcl [--data-dir <path>] [--output <path>] [--format json] [--force]
openjill-rs dump sha [--data-dir <path>] [--output <dir>] [--format json] [--force]
openjill-rs dump jn [--data-dir <path>] [--output <dir>] [--format json] [--force]
```

`--data-dir` resolution is shared by verification and dumps:

1. Use the explicit `--data-dir` when provided.
2. Otherwise use `OPENJILL_DATA_DIR` when set.
3. Otherwise use workspace-relative `data/original/JILL1`.

The default should change from the current lowercase stub path to
`data/original/JILL1` to match the repository data convention. All original-data
opens must go through `DataDirectory::open_reader` or an equivalent
case-insensitive resolver.

`--format text` is allowed only for `data verify` and is the human default.
`--format json` is the machine-readable default for dumps. Dump output must be
deterministic: stable field order in serializers, sorted file discovery, sorted
entry emission by parser order, and lowercase hexadecimal checksums.

## Output path rules

Verification is read-only. It must never create or modify files.

Dump commands may write only to:

- an explicit user-selected `--output` path outside the workspace; or
- an explicit or default path under ignored `data/extracted/`.

When `--output` is omitted, the default dump root is
`data/extracted/JILL1/debug/` with these outputs:

- `dump dma`: `data/extracted/JILL1/debug/dma.json`
- `dump vcl`: `data/extracted/JILL1/debug/vcl-text.json`
- `dump sha`: `data/extracted/JILL1/debug/sha/metadata.json` plus atlas files
  in the same directory
- `dump jn`: `data/extracted/JILL1/debug/jn/metadata.json`

Safe-write checks are part of the shared dump framework:

- Reject any output path inside `data/original/`.
- Reject any output path inside the workspace unless it is under
  `data/extracted/`.
- Reject writing directly over an existing file unless `--force` is supplied.
- Create missing output directories only after the path has passed safety
  checks.
- Write through a temporary file in the same directory and then rename it into
  place so interrupted dumps do not leave partial JSON or atlas files.
- Never copy, move, rewrite, or normalize original input files.

These rules intentionally make `data/extracted/` the only in-repository dump
location. If a developer wants dumps elsewhere, they must choose a path outside
the workspace.

## Verification behavior

`openjill-rs data verify` validates the episode 1 data directory and exits with
a failing status when any required file is missing or invalid.

Required fixed files:

- `JILL.DMA`
- `JILL1.VCL`
- `JILL1.CFG`
- `JILL1.SHA`
- `INTRO.JN1`
- `MAP.JN1`

Episode level/save discovery:

- Discover `*.JN1` entries in the data directory case-insensitively.
- Sort discovered paths case-insensitively by file name, then by resolved path
  as a tie-breaker.
- Report `INTRO.JN1` and `MAP.JN1` in their fixed-file slots and report all
  other discovered `*.JN1` files under the `jn_files` section.
- Treat no additional discovered `*.JN1` files as a verification failure,
  because episode 1 gameplay needs level/save-shaped JN data beyond intro and
  map.

Per-file metadata:

- requested file name;
- resolved path relative to the supplied data directory when present;
- status: `present`, `missing`, or `invalid`;
- byte length for present files;
- SHA-256 checksum for present files, rendered as lowercase hex;
- parser domain (`DMA`, `VCL`, `CFG`, `SHA`, or `JN`);
- parser validity and parser error text for invalid files;
- structural summary when parsing succeeds.

Parser summaries:

- `DMA`: entry count, first and last map code, duplicate map-code count.
- `VCL`: text entry count, minimum and maximum source offsets, total decoded
  text byte count.
- `CFG`: high-score count, save-slot count, setup flags, and save-prefix-derived
  file names.
- `SHA`: header entry count, valid header entry count, tileset count, tile
  count, and color-map entry counts.
- `JN`: background dimensions, object count, string count, save-data offset, and
  map-code minimum/maximum when the background is non-empty.

Verification should print one line per required fixed file in text mode, then a
short discovered-`*.JN1` summary. JSON mode should expose the same data as a
single object with `data_dir`, `required_files`, `jn_files`, and `ok` fields.
The command must not compare checksums against hard-coded proprietary values.

## Dump command framework

The shared dump framework should provide:

- data-dir resolution with `OPENJILL_DATA_DIR` fallback;
- case-insensitive opening through `DataDirectory`;
- deterministic `*.JN1` discovery;
- safe output path validation;
- optional overwrite through `--force`;
- file-context wrapping for missing input, parser, serialization, and write
  errors;
- a single place to create parent directories and perform temporary-file
  writes.

The framework should be implemented before individual dump payloads so later
issues can plug in without changing command shapes.

Use `serde`/`serde_json` for structured dump output. Use `sha2` or an equivalent
well-maintained crate for SHA-256. For SHA atlas image files, use a small PNG
encoder dependency such as `png`; do not add renderer/gameplay dependencies to
produce debug artifacts.

## DMA dump

Command:

```text
openjill-rs dump dma [--data-dir <path>] [--output <path>] [--force]
```

Input parser dependencies:

- `DataDirectory::open_reader("JILL.DMA")`
- `openjill_data::dma::DmaFile`
- `DmaEntry` accessors for index, source offset, map code, tile, tileset, flags,
  helper flag booleans, and name

Output metadata fields:

- `source_file`: `JILL.DMA`
- `source_size`
- `source_sha256`
- `entry_count`
- `entries[]` with `index`, `source_offset`, `map_code`, `map_code_hex`,
  `tile`, `tileset`, `flags`, `flags_hex`, `is_msg_touch`, `is_msg_draw`,
  `is_msg_update`, `is_player_thru`, `is_stair`, `is_vine`, and `name`

Entries are emitted in parser order. The dump contains decoded metadata only,
not raw byte slices.

## VCL dump

Command:

```text
openjill-rs dump vcl [--data-dir <path>] [--output <path>] [--force]
```

Input parser dependencies:

- `DataDirectory::open_reader("JILL1.VCL")`
- `openjill_data::vcl::VclFile`
- `VclTextEntry` accessors for text and source offset

Output metadata fields:

- `source_file`: `JILL1.VCL`
- `source_size`
- `source_sha256`
- `text_entry_count`
- `entries[]` with `index`, `source_offset`, `declared_length`, and `text`

If `VclTextEntry` does not currently expose declared length, add it to
`openjill-data` rather than recomputing it from serialized text. Sound entries
remain skipped and should be represented only by a top-level
`sound_entries_supported: false` field.

## SHA dump

Command:

```text
openjill-rs dump sha [--data-dir <path>] [--output <dir>] [--force]
```

Input parser dependencies:

- `DataDirectory::open_reader("JILL1.SHA")`
- `openjill_data::sha::ShaFile`
- `ShaHeader`, `ShaHeaderEntry`, `ShaTileSet`, `ShaColorMapEntry`, and
  `ShaTile` accessors

Output files:

- `metadata.json`
- `atlas-indexed.png`
- optionally `atlas-vga.png` once palette expansion is documented well enough

`metadata.json` fields:

- `source_file`: `JILL1.SHA`
- `source_size`
- `source_sha256`
- `header_entry_count`
- `valid_header_entry_count`
- `tileset_count`
- `tilesets[]` with `entry_index`, `source_offset`, `declared_size`,
  `tile_count`, `rotations`, `cga_size`, `ega_size`, `vga_size`, `bit_depth`,
  `flags`, `is_font`, `is_level_tileset`, `color_map_entry_count`, and
  `tiles[]`
- `tiles[]` entries with `image_index`, `source_offset`, `width`, `height`,
  `data_format`, `indexed_byte_count`, `atlas_x`, `atlas_y`, `atlas_width`, and
  `atlas_height`
- `atlas_files[]` with file name, pixel format, width, height, and tile padding

Atlas layout should be deterministic row-major packing with fixed padding. The
first implementation may write `atlas-indexed.png` as grayscale indexed values
for renderer inspection. If VGA expansion is added in this epic, document the
palette source in `metadata.json`; do not move palette conversion into
`openjill-data`.

## JN dump

Command:

```text
openjill-rs dump jn [--data-dir <path>] [--output <dir>] [--force]
```

Input parser dependencies:

- `DataDirectory::open_reader("INTRO.JN1")`
- `DataDirectory::open_reader("MAP.JN1")`
- deterministic case-insensitive discovery of other `*.JN1` files
- `openjill_data::jn::JnFile`
- `JnBackgroundLayer`, `JnObject`, `JnSaveData`, and `JnString` accessors

Output file:

- `metadata.json`

Top-level fields:

- `data_dir`
- `files[]`

Per-file fields:

- `requested_file`
- `resolved_file`
- `source_size`
- `source_sha256`
- `background` with `width`, `height`, `cell_count`, `map_code_min`,
  `map_code_max`, and `nonzero_cell_count`
- `objects` with `count` and `entries[]`
- `save_data` with `source_offset`, `level`, `health`, `score`, and
  `inventory_word_count`
- `strings` with `count`, `total_bytes_in_file`, and `entries[]`

Object entries include `index`, `source_offset`, `object_type`, `x`, `y`,
`x_speed`, `y_speed`, `width`, `height`, `state`, `sub_state`, `state_count`,
`counter`, `flags`, `pointer`, `info1`, `zap_hold`, and `string_index`.

String entries include `index`, `source_offset`, `size_in_file`, `terminator`,
and decoded `text`. The dump must not include raw background grids or raw object
byte slices; summarize grids through counts and ranges.

## Tests and acceptance checks

Use synthetic/temp data for committed tests. Do not commit original game bytes,
extracted JSON from original files, PNG atlases from original files, or
screenshots produced from original data.

Required tests by child issue:

- `#29 data verify`: CLI parsing, explicit `--data-dir`, `OPENJILL_DATA_DIR`
  fallback, missing-file reporting, invalid-file reporting, checksum stability,
  and read-only behavior.
- `#30 dump framework`: CLI parsing for `dma`, `vcl`, `sha`, and `jn`; default
  `data/extracted/JILL1/debug/` paths; explicit outside-workspace output
  acceptance; rejection of `data/original/`; rejection of tracked workspace
  paths outside `data/extracted/`; overwrite behavior with and without
  `--force`.
- `#28 dma/vcl`: deterministic JSON for synthetic DMA and VCL files, parser
  error context for malformed files, and stable entry ordering.
- `#27 sha`: metadata for multiple synthetic tilesets/images, deterministic
  atlas layout, atlas file dimensions, and no render/gameplay dependency.
- `#31 jn`: metadata for synthetic background, objects, save data, and strings;
  deterministic multi-file `*.JN1` discovery; missing required JN errors; and
  no raw proprietary byte slices in output.

Run targeted crate tests during each child issue and finish the epic with the
workspace checks defined by the Taskfile.

## Proprietary data safeguards

- Original data remains under `data/original/` and must never be staged,
  committed, copied into fixtures, uploaded, or pasted into logs.
- Generated dumps from original data remain under ignored `data/extracted/` or
  outside the workspace at a user-selected path.
- Tests use synthetic bytes constructed in test code or temporary directories.
- Verification may print sizes and checksums but must not hard-code expected
  checksums for copyrighted files.
- Dump output should contain decoded metadata and development summaries only.
  Text decoded from `JILL1.VCL` and `*.JN1` is allowed for local inspection, but
  generated files containing that text still count as extracted data and must
  not be committed.
- Commands must never mutate `JILL1.CFG`, `*.JN1`, or any other original file.

## Handoff notes

- Implement child issues in the order listed by epic `#3`: `#26`, `#29`, `#30`,
  `#28`, `#27`, then `#31`.
- Add doc comments for every new module, type, field, function, and method per
  `AGENTS.md`.
- Keep command errors actionable by naming the requested file, resolved path
  when available, parser domain, and output path involved.
- If a dump needs parser metadata that is not currently exposed, extend
  `openjill-data` accessors without adding rendering or gameplay behavior to
  that crate.
