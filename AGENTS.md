# AGENTS.md

Guidance for AI coding agents working in this repository.

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

## Related tasks

- `task binary:fetch` — download and unpack original game ZIP into
  `data/original/`. Requires `ZIP_LINK` and `ZIP_PASS` (sourced from `.env`)
  and `7z` (`p7zip-full`).
- `task data:clean` — remove `data/original` and any cached ZIP.
