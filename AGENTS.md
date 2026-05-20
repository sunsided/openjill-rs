# AGENTS.md

Guidance for AI coding agents working in this repository.

## Format reference

For byte-level layout of the original Jill data files (DMA, SHA, JN, MAC,
CFG, EXE palette offsets), consult
[`docs/port/00-format-reference.md`](docs/port/00-format-reference.md) before
touching parsers or anything that reads original bytes. It cites the
ModdingWiki sources and is the canonical answer for "what does this byte
mean?". Phase subplans and `PORT.md` link into it rather than duplicating
the spec.

## Documentation comments

Every module, type, field, function, and method must carry a doc comment,
regardless of whether it is `pub` or private. This applies to inherent impl
items, trait impls authored in this repository, enum variants, struct fields
(public and private alike), constants, type aliases, and free functions. The
rule does not relax for items that "look obvious" — the goal is for every
identifier to document its purpose, not just exposed surface.

Tests must carry a doc comment when they are nontrivial to reason about. For
those tests the doc comment must spell out:

- the unit under test,
- the preconditions the test sets up, and
- the invariants the test asserts.

Self-explanatory tests whose intent is fully captured by their name and a
short body do not need a doc comment, but err on the side of writing one
whenever a reader would otherwise have to derive intent from the test body.

## Integration tests for file-based operations

Any code that operates on real game-data files (parsers, extractors, asset
pipelines, anything that consumes bytes from `data/original/`) must ship with
an integration test that exercises the operation against the actual game
files, in addition to whatever synthetic-fixture unit tests already exist.

Use the existing integration tests in `openjill-data` as the pattern:

- `crates/openjill-data/tests/dma_original_data.rs`
- `crates/openjill-data/tests/vcl_original_data.rs`

Required behaviour for a real-data integration test:

- Resolve the data directory from `OPENJILL_DATA_DIR` first, then fall back
  to the workspace-relative `data/original/JILL1` path.
- Self-skip cleanly (print a skip message and return) when neither location
  is available, so machines without the original data still pass CI.
- Open files via `DataDirectory::open_reader` (or an equivalent
  case-insensitive resolver) so capitalisation differences across hosts do
  not break the test.
- Assert structural invariants the parser is supposed to uphold
  (non-empty results, in-range offsets, consistent counts, monotonic
  ordering, etc.) — not just that parsing returned `Ok`.

The data itself stays out of the repository; the test merely verifies the
parser against locally fetched bytes when those bytes are available.

## Original game data

Some work on the port requires the original Jill of the Jungle game data. The
canonical location for that data is:

```
data/original/JILL1
```

### When to ensure data is present

Before starting work that depends on the original data files (e.g. running
extractors, exercising parsers against real input, reproducing in-game
behavior, validating asset pipelines), the agent **must**:

1. Check whether `data/original/JILL1` exists.
2. If it does not exist, run `task binary:fetch` to download and unpack it.
3. Only proceed with the data-dependent work once the directory is in place.

If `task binary:fetch` fails (missing `7z`, missing env vars in `.env`, network
failure, etc.), surface the failure to the user and stop — do not fabricate
data, stub out files, or try to bypass the fetch step.

### When NOT to fetch

Do not run `task binary:fetch` if any of the following is true:

- The user or surrounding context has explicitly instructed not to fetch,
  download, or touch `data/`.
- The task does not require the original data (e.g. pure refactoring, doc
  edits, build/CI changes, code that operates only on synthetic fixtures or
  in-memory inputs).
- The agent is operating in an offline/sandboxed environment where outbound
  network access is disallowed.

In those cases, the absence of `data/original/JILL1` is silently fine — proceed
with the task as if it were not relevant.

### Rule of thumb

- Work needs data → ensure data exists, fetch if missing (unless told not to).
- Work doesn't need data → ignore `data/` entirely.

### Never commit or leak original game data

The original Jill of the Jungle bytes are copyrighted. They live under
`data/original/` (and any derivatives under `data/extracted/`) **only as
in-flight, locally fetched material**. They must never enter the repository or
any artifact published from it.

Hard rules for agents:

- Do **not** `git add`, `git commit`, or otherwise stage anything under
  `data/original/` or `data/extracted/`. Both paths are already in
  `.gitignore` — keep them there.
- Do **not** remove, narrow, or override those `.gitignore` entries (no
  `git add -f`, no `!data/original/...` negations, no per-subdir
  `.gitignore` exceptions).
- Do **not** copy original game bytes into other tracked locations
  (`crates/`, `tools/`, test fixtures, docs, screenshots, etc.) to "work
  around" the ignore. Original bytes stay under `data/`, period.
- Do **not** embed original bytes (or close derivatives such as raw tile
  PNG dumps) into source files, test data, generated code, or commit
  messages. Tests must use the small synthetic fixtures the project
  already ships, not slices of real game data.
- Do **not** push, attach, paste, or upload original game bytes anywhere
  outside the local checkout (PR descriptions, issues, gists, chat logs,
  CI artifacts, third-party services).
- If a task seems to require committing original data to make progress,
  stop and surface the conflict to the user instead of finding a
  workaround.

The intended lifecycle is: `task binary:fetch` populates `data/original/`
on demand → code reads from it locally → `task data:clean` (or manual
deletion) removes it. Nothing in between should leave the working tree.

## Related tasks

- `task binary:fetch` — download and unpack original game ZIP into
  `data/original/`. Requires `ZIP_LINK` and `ZIP_PASS` (sourced from `.env`)
  and `7z` (`p7zip-full`).
- `task data:clean` — remove `data/original` and any cached ZIP.

## Taskfile utility commands

When adding or changing a user-facing utility command in `openjill-rs`, update
`Taskfile.dist.yaml` with a matching task so contributors can run it through the
project's standard task interface. Keep command semantics in the Rust CLI and
use shared Taskfile wrappers instead of per-command shell logic:

- Use the `data:<command>` namespace for utilities that operate on original
  game data, such as `task data:verify` and `task data:dump`.
- Route data utility tasks through the shared internal data-command runner in
  `Taskfile.dist.yaml` so fetch and override handling stay consistent.
- In the CLI implementation, respect explicit `--data-dir` flags, the
  task-runner `DATA_DIR` override, and `OPENJILL_DATA_DIR` before falling back
  to `data/original/JILL1`.
- If the Taskfile task needs original data and no override is set, ensure
  `data/original/JILL1` exists by calling `task binary:fetch` before running
  the command.
- Pass through extra command arguments with `{{.CLI_ARGS}}` where the CLI
  command supports additional flags or subcommands.
