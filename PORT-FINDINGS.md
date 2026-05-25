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

### Enemy patrol must skip gap-detection during airborne phases

- **Symptom**: `FrogEntity` jumps straight up instead of arcing forward. Any
  enemy with a jumping mechanic reverses horizontal direction on every airborne
  tick if it applies `floor_under_next` unconditionally.
- **Root cause**: `floor_under_next` probes for a solid cell immediately below
  the entity's *next* horizontal position. When the entity is airborne its feet
  are not adjacent to any floor cell, so `floor_under_next` always returns
  `false`, triggering direction reversal every tick. The result is zero net
  horizontal displacement and a purely vertical arc.
- **Resolution**: gate the gap-detection probe on the grounded state. When
  `y_speed == 0` (grounded), apply the full `blocked_ahead && floor_under_next`
  check. When `y_speed != 0` (airborne), use only `blocked_ahead` for wall
  bouncing. The Java `FrogManager` implicitly encodes this structure by entering
  a separate `stateOnJump` code path that calls only `moveObjectRight` /
  `moveObjectLeft` with no gap check.
- **Applies to**: any `ObjectEntity` that patrols on the floor and has a jump
  or gravity mechanic. Floor-aware probes are only valid when the entity is
  grounded.
- **Reference**: discovered during episode-1 playthrough verification; fixed
  after PR #95.

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

### Enemy hit must deduct health, not kill the player outright

- **Symptom**: touching any enemy immediately kills the player and restarts
  the level, regardless of remaining health. The health bar never decrements.
- **Root cause**: `dispatch_player_touches` called `player.on_kill(1, kind)`
  directly. `PlayerEntity::on_kill` ignores the `_damage: i32` argument and
  immediately arms the `Die` sub-state. No health deduction occurred anywhere
  in the path.
- **Resolution**: `dispatch_player_touches` now takes `&mut RuntimeState`.
  On a pending kill it decrements `state.health` by 1 and calls
  `player.on_kill(1, kind)` only when `state.health` reaches zero. This
  mirrors Java's `AbstractHitPlayerObjectEntity.hitPlayer()` flow:
  `INVENTORY_LIFE(-1)` reduces the health bar, then `isPlayerDead()` is
  checked before `killPlayer()` is invoked.
- **Applies to**: any future code path that wants to damage the player. Never
  call `player.on_kill` directly from a touch handler; always route through
  `state.health` first.
- **Reference**: bug found during episode-1 playthrough verification; fixed
  after PR #95.

### Inventory and health must be restored from a level-entry snapshot on restart

- **Symptom**: items picked up during a failed run persist across death and
  respawn. After one death the player already has all items from the failed
  attempt; after two deaths duplicates accumulate without limit.
- **Root cause**: `GameOrchestrator::apply_transition(RestartLevel)` recreated
  the `LevelScreen` from the cached JN bytes but never touched `self.state`.
  `RuntimeState` is orthogonal to the level data; pickups are appended to
  `state.inventory` via `StatusUpdate::Item` messages and those changes survive
  any number of level reloads.
- **Resolution**: the orchestrator now stores a `level_entry_state:
  Option<RuntimeState>` snapshot the moment a `Level` transition succeeds
  (i.e. when the player first enters the level). On `RestartLevel` it restores
  `state.health` and `state.inventory` to the snapshot values and decrements
  `state.lives` by 1. `state.score` is intentionally NOT restored: Jill of the
  Jungle retains score across deaths.
- **Applies to**: any future state that should reset on death but not on level
  exit (e.g. a rage-meter, a per-level power-up). Anything that should be
  preserved beyond a death belongs in `RuntimeState`; anything that should
  reset belongs in the snapshot-restore path.
- **Reference**: bug found during episode-1 playthrough verification; fixed
  after PR #95.

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

See [`docs/port/00-format-reference.md`](docs/port/00-format-reference.md)
and the `jill-data-formats` skill for the canonical byte-layout
reference; record only deviations / reviewer-flagged surprises here.

### SHA tileset indices in `object_conf.json` are header-table indices, not sequential counts

- **Symptom**: enemy entities render the wrong sprite. Examples before the fix:
  a gator looked like a giant ant, a firebird enemy looked like a blue flag, a
  snake rendered as a different enemy. All shared the same root cause: their
  `TILESET_INDEX` constant was derived from a sequential SHA dump rather than
  from the header table.
- **Root cause**: the SHA file starts with a 128-entry header table (128 x u32
  offsets + 128 x u16 sizes = 768 bytes). Entry 0 is conventionally invalid;
  valid tilesets begin at index 1. A one-off dump script that enumerated only
  valid entries and numbered them 0, 1, 2, ... produced `seq k = header index
  k+1`. Any entity constant derived from that script is off by at least one
  position. `object_conf.json` always stores the true header entry index (e.g.
  `"tileSet": 39` reads from `offsets[39]`).
- **Resolution**: treat `object_conf.json` `"tileSet"` values as the ground
  truth header index; do not derive them from sequential SHA dumps. The
  `openjill-data` SHA parser is correct (it uses the header table); the bug was
  in a one-off inspection script. Confirmed corrections for all episode-1
  entities: FirebirdEnemy 5->11, Gator 10->39, GiantAnt 6->10, Crab 9->38,
  Ghost 12->50, Skull 11->47, Bees 8->37, Snake 7->15, Eyes 13->62.
- **Applies to**: every entity that hard-codes a `TILESET_INDEX` constant. When
  porting a new entity, always cross-check the constant against
  `object_conf.json` (not a sequential SHA dump).
- **Reference**: discovered during episode-1 playthrough verification; fixed
  after PR #95.

---

## Java reference bugs

Use this section for behaviours in the Java OpenJill reference that
turn out to be bugs (flipped coordinates, off-by-one, accidental
mutation, etc.) so the Rust port does not faithfully reproduce them.

_No findings yet._

---

## Tooling and CI

_No findings yet._

---

## Open issues (unresolved - observed during playthrough)

These are observed deviations from original game behaviour that have not
yet been investigated or fixed. Move each entry to the appropriate
resolved section once root cause and resolution are known.

### Firebird: on-touch explosion with gem scatter not implemented

- **Symptom**: touching a firebird deals damage but the firebird does not
  explode and gems do not splatter around on contact.
- **Expected**: firebird explodes on player contact; several gems scatter
  from the explosion position; player loses health.
- **Likely applies to**: other enemies that have an on-death spawn effect.
- **Status**: unresolved.

### Gator: repeated contact damage instead of single-hit invincibility window

- **Symptom**: while Jill stands on or touches the gator, damage is applied
  every tick. Touching the gator roughly five times kills Jill before the
  gator is defeated.
- **Expected**: the first contact deals one point of damage; subsequent
  frames while still touching the same enemy must not re-trigger damage
  (invincibility / grace period). The gator should be defeatable without
  Jill dying from repeated contact.
- **Likely applies to**: all enemies - the invincibility window after a
  player hit is probably a global mechanic, not gator-specific.
- **Status**: unresolved.

### Input: SPACE and ALT both jump; SHIFT should jump, ALT should throw knife

- **Symptom**: both SPACE and ALT trigger a jump. SHIFT has no binding.
  Throwing the knife (ALT in original) is unbound or incorrectly mapped.
- **Expected**: SHIFT = jump, ALT = throw knife (original DOS key layout).
  SPACE may remain as an alias or be removed.
- **Status**: unresolved.

### Frog: does not chase player

- **Symptom**: the frog enemy patrols without reacting to player proximity.
- **Expected**: frog detects the player and hops toward them (chasing
  behaviour as in the original game).
- **Status**: unresolved.
