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

## Player physics

### `FLAG_NOT_STAIR` is an opt-out bit - most tiles are stairs by default

- **Symptom**: player on MAP.JN1 is in suspended animation - can run, climb,
  and jump in place but never falls. Decorative shade tiles (BLSHADE1-8, map
  codes 65-72) block all movement despite being marked passthrough.
- **Root cause**: DMA `FLAG_NOT_STAIR = 0x02` is an opt-out flag. Tiles that
  do not set this bit are stairs by default. BLSHADE tiles have `flags=0x0201`
  (`FLAG_PLAYER_THRU | FLAG_STAIR_DEFAULT`), so `is_stair()` returns `true`
  for them. All three collision probes (`has_floor_below`, `collides_vertical`,
  `collides_horizontal`) had `|| cell.is_stair()` which made passthrough stair
  tiles solid, overriding the passthrough flag.
- **Resolution**: remove `|| cell.is_stair()` from all collision probes.
  `cell.blocks_vertical(dy)` already encodes the correct semantics (passthrough
  overrides stair); `is_stair()` must never be used standalone as a collision
  predicate.
- **Applies to**: every collision probe that tests `is_stair()` directly.
  Use `blocks_vertical(dy)` / `is_passthrough()` instead.
- **Reference**: map physics fix, commit after PR #95.

### Vertical movement must snap to floor, not all-or-nothing

- **Symptom**: run-fall-run-fall oscillation. When falling at speed > one
  tile per tick, `try_move_vertical` rejects the whole step on collision and
  leaves the player 1-15 px above the floor. `has_floor_below` probes at the
  current feet position (not at the actual floor), so it returns `false`,
  transitioning to Jumping. Sub-state resets to 0, gravity builds again, the
  same collision fires, repeat.
- **Root cause**: all-or-nothing vertical movement cannot land when the step
  overshoots the floor. Java's `moveObjectDown` (in `UtilityObjectEntity`)
  scans rows from current feet to destination feet and snaps
  `y = blocking_row * BLOCK_SIZE - height` on the first solid row.
- **Resolution**: `try_move_vertical` now uses a row-scan-and-snap approach
  for downward motion (`dy > 0`). Upward motion (`dy < 0`) retains the
  all-or-nothing check (matching Java `moveObjectUp` semantics where the snap
  is to ceiling bottom, which is rarely reached).
- **Applies to**: any physics step that moves an entity downward by more than
  one pixel per tick. The same pattern is needed for enemy entities that use
  gravity.
- **Reference**: run-fall oscillation fix, commit after PR #95.

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
