# PORT-FINDINGS

Pattern-level lessons learned from porting Jill of the Jungle to Rust.
Every reviewer-flagged issue that points at a recurring pattern, an
engine invariant, a data-file quirk, or a Java-reference bug lands here
together with its resolution, so the same mistake is not made twice.

This file is **not** a changelog and **not** a per-function todo list.
Entries are about *patterns* (or about the use of a specific function /
trait method that is easy to misuse). If a finding only applies to one
local fix and teaches nothing reusable, do not add it.

## How to use this file

- **Before starting non-trivial gameplay, parser, or engine work**, skim
  the relevant section so any prior finding shapes the new code from the
  start.
- **After a PR review**, fold every reviewer-flagged finding into the
  matching section (or create a new one) with: symptom, root cause,
  resolution, applies-to, reference (PR / commit / file).
- **When a Java reference behaviour turns out to be a bug** (flipped
  coordinates, off-by-one, accidental mutation), record it here so the
  Rust port does not faithfully reproduce it.

Findings are grouped by area, not chronology, and link back to the
`docs/port/*.md` sub-plans where the topic is already covered.

---

## Gameplay engine

### Hazard kills must arm `player.on_kill`, not dispatch `DieRestartLevel`

- **Symptom**: a hazard touch starts the level-transition message-box
  overlay on the touch frame and hides the player's die animation.
- **Root cause**: `PlayerEntity::tick_die` already dispatches
  `MessageType::DieRestartLevel` after `STATECOUNT_MAX_TO_RESTART_GAME`
  ticks once the player enters its `Die` sub-state. Any handler that
  sends `DieRestartLevel` directly (in addition to, or instead of,
  arming the player) duplicates the message and triggers the transition
  before the animation runs.
- **Resolution**: hazard touch handlers must only call
  `player.on_kill(damage, DeathKind::...)`. The player drives the
  restart timing; do not send `DieRestartLevel` from a hazard.
- **Applies to**: every `BackgroundEntity::on_player_touch` and every
  `ObjectEntity::on_touch` that represents a lethal hazard. Also applies
  to enemies and projectiles when they kill the player.
- **Reference**: PR #90 review comments
  [r3292585152](https://github.com/sunsided/openjill-rs/pull/90#discussion_r3292585152),
  [r3292585161](https://github.com/sunsided/openjill-rs/pull/90#discussion_r3292585161),
  [r3292585165](https://github.com/sunsided/openjill-rs/pull/90#discussion_r3292585165),
  [r3292585167](https://github.com/sunsided/openjill-rs/pull/90#discussion_r3292585167);
  fix in commit `e2753c1`.

### Object hazards reach the player via `take_player_kill`, not via the message bus

- **Symptom**: an `ObjectEntity` hazard wants to kill the player but
  `on_touch(&mut self, state, dispatcher)` does not expose a player
  reference. Sending `DieRestartLevel` from `on_touch` (the obvious
  workaround) hits the bug recorded above.
- **Root cause**: the touch dispatch loop in
  `LevelScreen::dispatch_player_touches` calls `obj.on_touch(...)` with
  no `&mut dyn ObjectEntity` for the player; only `BackgroundEntity` has
  the symmetric `on_player_touch(&mut dyn ObjectEntity, ...)` signature.
- **Resolution**: hazards stash a pending `DeathKind` in an internal
  field during `on_touch` and override
  `ObjectEntity::take_player_kill() -> Option<DeathKind>` (default
  `None`) to drain it. `LevelScreen::dispatch_player_touches` collects
  the first pending kill across the touch pass and applies
  `player.on_kill(1, kind)` once, after which the player's `Die`
  sub-state drives the restart per the finding above.
- **Applies to**: every `ObjectEntity` implementation that kills or
  damages the player on contact (hazards, enemies, hostile
  projectiles).
- **Reference**: PR #90 review comments
  [r3292585170](https://github.com/sunsided/openjill-rs/pull/90#discussion_r3292585170),
  [r3292585178](https://github.com/sunsided/openjill-rs/pull/90#discussion_r3292585178),
  [r3292585185](https://github.com/sunsided/openjill-rs/pull/90#discussion_r3292585185),
  [r3292585189](https://github.com/sunsided/openjill-rs/pull/90#discussion_r3292585189);
  fix in commit `e2753c1`.

---

## Renderer

_No findings yet._

---

## Data files (DMA, SHA, JN, MAC, CFG, VCL, CMF)

_No findings yet._

See [`docs/port/00-format-reference.md`](docs/port/00-format-reference.md)
and the `jill-data-formats` skill for the canonical byte-layout
reference; record only deviations / reviewer-flagged surprises here.

---

## Java reference bugs

Use this section for behaviours in the Java OpenJill reference that
turn out to be bugs (flipped coordinates, off-by-one, accidental
mutation, etc.) so the Rust port does not faithfully reproduce them.

_No findings yet._

---

## Tooling and CI

_No findings yet._
