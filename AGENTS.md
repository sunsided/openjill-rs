# AGENTS.md

Guidance for AI coding agents working in this repository.

## Format reference

For byte-level layout of the original Jill data files (DMA, SHA, JN, MAC,
CFG, VCL, CMF/`*.DDT`, Crunched Screen Image, EXE palette offsets),
consult [`docs/port/00-format-reference.md`](docs/port/00-format-reference.md)
before touching parsers or anything that reads original bytes. It cites
the ModdingWiki sources and is the canonical answer for "what does this
byte mean?". Phase subplans and `PORT.md` link into it rather than
duplicating the spec.

Agents working in this repo also have the
[`jill-data-formats`](.claude/skills/jill-data-formats/SKILL.md) skill
available. Invoke it (or let the model pick it up automatically) for
parser/dumper/extractor work, format quirks, iType questions, and
byte-layout debugging - it routes into the reference doc and keeps the
high-leverage pitfalls (`KILLME` sentinel, malformed `dan.cmf`
end-of-track, preserved-unknown-bytes rule) in one place.

## Port findings: read before, write after

The repo keeps a running list of recurring port-level pitfalls in
[`PORT-FINDINGS.md`](PORT-FINDINGS.md). It is the central place for
reviewer-flagged patterns, engine invariants, data-file quirks, and Java
reference bugs the Rust port must not faithfully reproduce.

Two non-optional habits:

1. **Cross-check before writing code.** Before starting non-trivial
   work on the gameplay engine, renderer, parsers, or anything that
   consumes Jill data bytes, skim the relevant `PORT-FINDINGS.md`
   section so any prior finding shapes the new code from the start.
   If a finding applies, follow its resolution; if it conflicts with
   the task, surface the conflict to the user rather than silently
   working around it.
2. **Record reviewer-flagged findings after the PR lands.** When a PR
   review (human or bot) flags something that points at a recurring
   pattern, an engine invariant, a data-file quirk, or a Java
   reference bug, add an entry to the matching section of
   `PORT-FINDINGS.md` together with its resolution. Format per entry:
   symptom, root cause, resolution, applies-to, reference (PR /
   commit / file). Do *not* record per-function bug fixes that teach
   nothing reusable - findings are about *patterns*, not changelog
   entries. If a finding is about the correct use of a specific
   function or trait method, that is fine; if it is just "function X
   had bug Y, fixed in commit Z", it belongs in the commit message,
   not in `PORT-FINDINGS.md`.

A finding that lives only in a PR comment or commit message will be
forgotten by the next contributor (human or agent); a finding in
`PORT-FINDINGS.md` will not.

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

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **openjill-rs** (6967 symbols, 18842 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## When Debugging

1. `gitnexus_query({query: "<error or symptom>"})` — find execution flows related to the issue
2. `gitnexus_context({name: "<suspect function>"})` — see all callers, callees, and process participation
3. `READ gitnexus://repo/openjill-rs/process/{processName}` — trace the full execution flow step by step
4. For regressions: `gitnexus_detect_changes({scope: "compare", base_ref: "main"})` — see what your branch changed

## When Refactoring

- **Renaming**: MUST use `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` first. Review the preview — graph edits are safe, text_search edits need manual review. Then run with `dry_run: false`.
- **Extracting/Splitting**: MUST run `gitnexus_context({name: "target"})` to see all incoming/outgoing refs, then `gitnexus_impact({target: "target", direction: "upstream"})` to find all external callers before moving code.
- After any refactor: run `gitnexus_detect_changes({scope: "all"})` to verify only expected files changed.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Tools Quick Reference

| Tool | When to use | Command |
|------|-------------|---------|
| `query` | Find code by concept | `gitnexus_query({query: "auth validation"})` |
| `context` | 360-degree view of one symbol | `gitnexus_context({name: "validateUser"})` |
| `impact` | Blast radius before editing | `gitnexus_impact({target: "X", direction: "upstream"})` |
| `detect_changes` | Pre-commit scope check | `gitnexus_detect_changes({scope: "staged"})` |
| `rename` | Safe multi-file rename | `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` |
| `cypher` | Custom graph queries | `gitnexus_cypher({query: "MATCH ..."})` |

## Impact Risk Levels

| Depth | Meaning | Action |
|-------|---------|--------|
| d=1 | WILL BREAK — direct callers/importers | MUST update these |
| d=2 | LIKELY AFFECTED — indirect deps | Should test |
| d=3 | MAY NEED TESTING — transitive | Test if critical path |

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/openjill-rs/context` | Codebase overview, check index freshness |
| `gitnexus://repo/openjill-rs/clusters` | All functional areas |
| `gitnexus://repo/openjill-rs/processes` | All execution flows |
| `gitnexus://repo/openjill-rs/process/{name}` | Step-by-step execution trace |

## Self-Check Before Finishing

Before completing any code modification task, verify:
1. `gitnexus_impact` was run for all modified symbols
2. No HIGH/CRITICAL risk warnings were ignored
3. `gitnexus_detect_changes()` confirms changes match expected scope
4. All d=1 (WILL BREAK) dependents were updated

## Keeping the Index Fresh

After committing code changes, the GitNexus index becomes stale. Re-run analyze to update it:

```bash
npx gitnexus analyze
```

If the index previously included embeddings, preserve them by adding `--embeddings`:

```bash
npx gitnexus analyze --embeddings
```

To check whether embeddings exist, inspect `.gitnexus/meta.json` — the `stats.embeddings` field shows the count (0 means no embeddings). **Running analyze without `--embeddings` will delete any previously generated embeddings.**

> Claude Code users: A PostToolUse hook handles this automatically after `git commit` and `git merge`.

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
