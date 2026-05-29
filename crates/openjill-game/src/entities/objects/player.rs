//! Player object entity (JN object type 0).
//!
//! Rust translation of the Java reference's
//! `PlayerManager` + `AbstractPlayerManager` + `AbstractPlayerInteractionManager`
//! (see `open-jill-object-background/src/main/java/org/jill/game/entities/obj/player/`).
//!
//! Implements the subset of the player state machine required by epic 6
//! child issue 2 (`docs/port/06-episode-1-gameplay.md`): `Stand`, `Jumping`,
//! `Climbing`, and `Die`.  The `Begin` and `Still` entry states are modelled
//! but kept terse pending the level-entry animation work.
//!
//! Movement constants, sub-state semantics, sprite tile indices, and the die
//! burst behavior mirror the reference exactly.  Background collision relies
//! on the `BackgroundEntity` flags exposed by `openjill-core` (`is_passthrough`,
//! `is_climbable`, `is_stair`) and the [`BackgroundGrid`] cell lookup.

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, InputCommand, InventoryObject, MessageDispatcher,
    MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand, RuntimeState,
};

/// Ticks between successive player shots.
///
/// Mirrors `PlayerStandConst.FIRE_COOLDOWN` from the Java reference: the
/// player can fire once every 8 ticks at the standard rate.
const FIRE_COOLDOWN_TICKS: i32 = 8;

/// Horizontal speed of a player-fired bullet in pixels per tick.
///
/// Matches the speed used by `BulletManager` in the Java reference.
const BULLET_SPEED_PX: i32 = 8;
/// Downward spawn offset applied to a thrown knife, in pixels.
///
/// Matches Java `KniveManager` `initY = 2` (`this.y += initY` on launch).
const KNIFE_SPAWN_Y_OFFSET: i32 = 2;
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

// ---------------------------------------------------------------------------
// Movement constants (mirrored from the Java `Player*Const` interfaces)
// ---------------------------------------------------------------------------

/// SHA tileset index that owns every player frame.
///
/// Matches `PlayerStandConst.TILESET_INDEX = 8` and the same value re-declared
/// on the jumping, climbing, and die0 const interfaces.
const TILESET_INDEX: u8 = 8;

/// Stand and jump-run horizontal step in pixels per tick.
///
/// Matches `PlayerStandConst.PLAYER_MOVE_SIZE = 8`.
const PLAYER_MOVE_SIZE: i32 = 8;

/// Negative `y_speed` magnitude applied at the start of a ground jump.
///
/// Matches `PlayerJumpingConst.JUMP_INIT_SIZE = 16`; the player's `y_speed`
/// becomes `-(JUMP_INIT_SIZE + high_jump_size)` on jump start.
const JUMP_INIT_SIZE: i32 = 16;

/// Per-tick acceleration applied to `y_speed` during the fall portion of a
/// jump (`PlayerJumpingConst.JUMP_INCREMENT_VALUE = 2`).
const JUMP_INCREMENT_VALUE: i32 = 2;

/// Terminal fall speed cap (`PlayerJumpingConst.JUMP_FALLING_SPEED_LIMIT = 16`).
const JUMP_FALLING_SPEED_LIMIT: i32 = 16;

/// Negative `y_speed` magnitude applied to a jump initiated from a climb
/// (`PlayerJumpingConst.JUMP_INIT_SIZE_FOR_CLIMB = 12`).
const JUMP_INIT_SIZE_FOR_CLIMB: i32 = 12;

/// Per-tick downward step while climbing
/// (`PlayerClimbConst.PLAYER_MOVE_SIZE_CLIMB_DOWN = 4`).
const PLAYER_MOVE_SIZE_CLIMB_DOWN: i32 = 4;

/// Per-tick vertical step while climbing up, indexed by climb sub-state
/// (`PlayerClimbConst.PLAYER_MOVE_SIZE_CLIMB_UP`).
///
/// The first two entries are 0 because they cover the climb-stop and the
/// climb-into transition; the rising frames step `-6, -4, -4, -4`.
const CLIMB_UP_STEPS: [i32; 6] = [0, 0, -6, -4, -4, -4];

/// Climb sub-state set when the player is stationary on a vine
/// (`PlayerClimbConst.SUBSTATE_JUMP_STOP = 0`).
const CLIMB_SUBSTATE_STOP: i32 = 0;

/// Climb sub-state set the moment the player enters the climbing state from
/// stand or jump (`PlayerClimbConst.SUBSTATE_JUMP_UP = 2`).
const CLIMB_SUBSTATE_ENTER: i32 = 2;

/// Climb sub-state set when the player presses down while climbing
/// (`PlayerClimbConst.PLAYER_SUBSTATE_DOWN = 2`).
const CLIMB_SUBSTATE_DOWN: i32 = 2;

/// Jumping sub-state threshold below which the player still plays the rising
/// silhouette animation and skips both gravity and horizontal influence
/// (`PlayerStandConst.SUBSTATE_VALUE_TO_FALL = 3`).
const SUBSTATE_VALUE_TO_FALL: i32 = 3;

/// Initial `y_speed` applied on die transitions
/// (`PlayerDie0Const.START_YD = -12`).  The water and other-background death
/// kinds use the same magnitude in the reference port's `Die1Const`/`Die2Const`.
const DIE_START_YD: i32 = -12;

/// Maximum `state_count` value before the die animation dispatches
/// `DieRestartLevel` (`PlayerDie0Const.STATECOUNT_MAX_TO_RESTART_GAME = 20`).
const STATECOUNT_MAX_TO_RESTART_GAME: i32 = 20;

/// Number of colored bullets emitted by the die burst
/// (`PlayerDie0Const.NB_COLORED_BULLET = 10`).
const NB_COLORED_BULLET: i32 = 10;

// ---------------------------------------------------------------------------
// Sprite tile indices (all in tileset 8)
// ---------------------------------------------------------------------------

/// Stand facing right.
///
/// The Java reference's `PlayerStandConst.TILE_RIGHT_INDEX = 20` and
/// `TILE_LEFT_INDEX = 21` constants are misnomers: in-engine playback
/// against the real `JILL1.SHA` tileset 8 shows tile 20 renders the
/// *left*-facing stand sprite and tile 21 the *right*-facing one, which
/// matched a user-reported "Jill snaps to the opposite direction when
/// running stops" bug.  The Rust port binds the tile constants to the
/// actual rendered facing rather than the Java field name so the stand
/// frame after a left run does not flip Jill's silhouette.
const TILE_STAND_RIGHT: u16 = 21;

/// Stand facing left.  See [`TILE_STAND_RIGHT`] for the swap rationale.
const TILE_STAND_LEFT: u16 = 20;

/// Stand facing forward (`PlayerStandConst.TILE_MIDDLE_INDEX = 16`).
const TILE_STAND_MIDDLE: u16 = 16;

/// Falling silhouette used while airborne with positive y_speed
/// (`PlayerStandConst.TILE_FALL_INDEX = 60`).
const TILE_FALL: u16 = 60;

/// Centre jump frame base (`PlayerJumpingConst.TILE_MIDDLE_INDEX = 56`).
const TILE_JUMP_MIDDLE_BASE: u16 = 56;

/// Left-facing jump frame base (`PlayerJumpingConst.TILE_LEFT_INDEX = 32`).
const TILE_JUMP_LEFT_BASE: u16 = 32;

/// Right-facing jump frame base (`PlayerJumpingConst.TILE_RIGHT_INDEX = 40`).
const TILE_JUMP_RIGHT_BASE: u16 = 40;

/// Left running frame base (`PlayerStandConst.TILE_LEFT_RUNNING_INDEX = 8`).
///
/// The Java reference rebinds `stStandLeftRunning[i] = pictureCache.getImage(8, i)`,
/// indexing tiles 0..7.  `TILE_LEFT_RUNNING_INDEX = 8` is therefore the
/// *right*-running base; preserve the same convention by naming our right base
/// off it and the left base off the start of the tileset.
// FIXME(epic-6): re-verify the left/right run tile bases against the actual
// SHA tileset once the game is runnable end-to-end.  The Java constant name
// (`TILE_LEFT_RUNNING_INDEX`) is misleading vs. how `PlayerManager.init`
// populates the arrays, and only an in-engine check against the original
// JILL1.SHA frames can confirm which base belongs to which facing.
const TILE_RUN_LEFT_BASE: u16 = 0;

/// Right running frame base (`PlayerStandConst.TILE_LEFT_RUNNING_INDEX = 8`).
// FIXME(epic-6): see TILE_RUN_LEFT_BASE.  Confirm against in-engine playback.
const TILE_RUN_RIGHT_BASE: u16 = 8;

/// Climb frame sequence translated from `PlayerManager.initClimbPicture`.
///
/// The Java code populates the array with tiles `[24, 24, 25, 26, 26, 25]`
/// (the last two are aliases of the previous frames); preserve the same
/// ordering so `sub_state` indexing matches the reference.
const TILE_CLIMB: [u16; 6] = [24, 24, 25, 26, 26, 25];

/// Die-by-enemy base tile (`PlayerDie0Const.TILE_INDEX = 48`); six frames
/// follow at 48..53.
const TILE_DIE_BASE: u16 = 48;

/// Number of die frames (`PlayerDie0Const.IMAGE_NUMBER = 6`).
const DIE_FRAME_COUNT: i32 = 6;

/// State count step before the die animation advances to the next frame
/// (`PlayerDie0Const.STATECOUNT_STEP_TO_CHANGE_PICTURE = 4`).
const DIE_STATECOUNT_STEP: i32 = 4;

// ---------------------------------------------------------------------------
// Player state machine
// ---------------------------------------------------------------------------

/// Top-level player state.
///
/// Mirrors the integer constants in `PlayerState.java`; the Rust port models
/// them as a small enum and tracks the sub-state separately in
/// [`PlayerEntity::sub_state`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerStateKind {
    /// Level-entry animation; lasts a fixed number of ticks then enters [`PlayerStateKind::Stand`].
    Begin,
    /// On the ground, running, ducking, or idle.
    Stand,
    /// Idle stand alias used by the Java reference; folded into [`PlayerStateKind::Stand`] here
    /// for transition routing.
    Still,
    /// Airborne following a jump or a no-floor fall-through.
    Jumping,
    /// Climbing on a vine cell.
    Climbing,
    /// Dying animation; emits `DieRestartLevel` after [`STATECOUNT_MAX_TO_RESTART_GAME`] ticks.
    Die,
}

impl PlayerStateKind {
    /// Encodes this state as the JN `state` integer.
    ///
    /// Matches the constants in the Java reference's `PlayerState.java`
    /// (`STAND=0, STILL=1, JUMPING=2, CLIMBING=3, BEGIN=4, DIE=5`), so the
    /// player's state round-trips through the JN object record.
    fn to_state_code(self) -> i16 {
        match self {
            PlayerStateKind::Stand => 0,
            PlayerStateKind::Still => 1,
            PlayerStateKind::Jumping => 2,
            PlayerStateKind::Climbing => 3,
            PlayerStateKind::Begin => 4,
            PlayerStateKind::Die => 5,
        }
    }

    /// Decodes a JN `state` integer into a player state.
    ///
    /// Inverse of [`PlayerStateKind::to_state_code`]; unrecognized codes fall
    /// back to [`PlayerStateKind::Stand`] (the level-authoring default).
    fn from_state_code(code: i16) -> Self {
        match code {
            1 => PlayerStateKind::Still,
            2 => PlayerStateKind::Jumping,
            3 => PlayerStateKind::Climbing,
            4 => PlayerStateKind::Begin,
            5 => PlayerStateKind::Die,
            _ => PlayerStateKind::Stand,
        }
    }
}

/// Player object entity.
pub struct PlayerEntity {
    /// World X position in pixels (top-left of the bounding box).
    x: i32,
    /// World Y position in pixels (top-left of the bounding box).
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Current top-level player state.
    state: PlayerStateKind,
    /// Per-state sub-state counter (animation frame index or fall-progress).
    sub_state: i32,
    /// Generic counter incremented each tick by the per-state update routines.
    state_count: i32,
    /// Horizontal direction: `-1` left, `0` neutral, `+1` right.
    x_speed: i32,
    /// Vertical speed in pixels per tick (positive = falling).
    y_speed: i32,
    /// Last horizontal direction the player faced; preserved across stops so
    /// the idle sprite keeps the correct facing.
    info1: i32,
    /// Player-side touch-cooldown counter mirroring `JillConst.zapholdValueAfterTouchPlayer`.
    zaphold: i32,
    /// Death classification while in [`PlayerStateKind::Die`]; `None` otherwise.
    death_kind: Option<DeathKind>,
    /// `true` when [`PlayerEntity::on_kill`] has set up the die transition but
    /// the matching die-state update has not yet emitted the bullet burst.
    die_pending: bool,
    /// Ticks remaining before the player may fire again.
    ///
    /// Set to [`FIRE_COOLDOWN_TICKS`] after each shot; decremented once per
    /// tick.  Prevents holding the fire key from creating a bullet every
    /// tick, matching the Java reference's rate-limiting behavior.
    fire_cooldown: i32,
    /// The JN object record this entity was built from.
    ///
    /// Cloned at construction and re-emitted (with the live fields overwritten)
    /// by [`ObjectEntity::snapshot`] so the authored fields the player model
    /// does not track (`counter`, `flags`, `pointer`, string association)
    /// survive a save-game round-trip untouched.
    origin: JnObject,
}

impl PlayerEntity {
    /// Builds a player entity from a JN object record.
    ///
    /// `cache` is accepted to align with the factory signature; future child
    /// issues will use it to seed the SHA tileset reference for bound-box
    /// derivation from the actual begin-state sprite dimensions.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        let state = PlayerStateKind::from_state_code(item.state());
        let sub_state = i32::from(item.sub_state());
        // While dying, the JN sub-state carries the death classification
        // (`enter_die_state`); recover it so a mid-death restore resumes the
        // correct die animation. Other states have no death classification.
        let death_kind = if matches!(state, PlayerStateKind::Die) {
            match item.sub_state() {
                1 => Some(DeathKind::Water),
                2 => Some(DeathKind::OtherBackground),
                _ => Some(DeathKind::Enemy),
            }
        } else {
            None
        };
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            state,
            sub_state,
            state_count: i32::from(item.state_count()),
            x_speed: i32::from(item.x_speed()),
            y_speed: i32::from(item.y_speed()),
            info1: i32::from(item.info1()),
            zaphold: i32::from(item.zap_hold()),
            death_kind,
            // `die_pending` and `fire_cooldown` are transient ticks-scoped
            // members with no JN field; they re-derive during play.
            die_pending: false,
            fire_cooldown: 0,
            origin: item.clone(),
        }
    }

    /// Returns the current top-level state.
    pub fn state(&self) -> PlayerStateKind {
        self.state
    }

    /// Returns the current sub-state counter.
    pub fn sub_state(&self) -> i32 {
        self.sub_state
    }

    /// Returns the player's vertical speed (positive = falling).
    pub fn y_speed(&self) -> i32 {
        self.y_speed
    }

    /// Returns the player's horizontal direction (`-1`/`0`/`+1`).
    pub fn x_speed(&self) -> i32 {
        self.x_speed
    }

    /// Returns the world `(x, y)` position of the player's bounding box origin.
    pub fn position(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    /// Sets the player's state to `state` and resets the sub-state counter.
    ///
    /// Exposed primarily for tests that want to drop the player into a
    /// specific state without driving the full transition sequence.
    pub fn set_state(&mut self, state: PlayerStateKind) {
        self.state = state;
        self.sub_state = 0;
        self.state_count = 0;
    }

    /// Overwrites the vertical speed.
    ///
    /// Exposed for tests that need to seed a falling or rising player without
    /// driving the full jump entry path.
    pub fn set_y_speed(&mut self, value: i32) {
        self.y_speed = value;
    }

    /// Returns `true` when the player's current state allows firing a weapon.
    ///
    /// Mirrors `AbstractPlayerInteractionManager.canFire` from the Java
    /// reference: firing is permitted in `Stand`, `Still`, and `Jumping`
    /// states but not while `Climbing` (both hands on the vine) or `Die`.
    pub fn can_fire(&self) -> bool {
        matches!(
            self.state,
            PlayerStateKind::Stand | PlayerStateKind::Still | PlayerStateKind::Jumping
        )
    }
}

impl ObjectEntity for PlayerEntity {
    /// Advances the player state machine by one tick.
    ///
    /// The dispatch order mirrors `PlayerManager.msgUpdate`: first the
    /// per-state movement / input pass, then the per-state animation /
    /// counter pass.  State transitions inside the movement pass are visible
    /// to the counter pass so newly entered states bump their `state_count`
    /// from `0` on the same tick.
    fn update(
        &mut self,
        input: &ActiveInput,
        state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        dispatcher: &mut MessageDispatcher,
    ) {
        if self.zaphold > 0 {
            self.zaphold -= 1;
        }

        if self.fire_cooldown > 0 {
            self.fire_cooldown -= 1;
        }

        // Fire: dispatch a CreateObject request when the throw/fire key is
        // pressed, the player state allows it, the cooldown is clear, and
        // the player actually carries a knife inventory item. The
        // inventory gate mirrors the Java reference: pressing the action
        // key with no thrown weapon in inventory is a no-op.
        // `info1` holds the last facing direction (-1 left, 0/+1 right); a
        // zero value is treated as right-facing for the projectile origin.
        let has_knife = state.inventory.contains(&InventoryObject::Knife);
        if input.contains(&InputCommand::ThrowItem)
            && self.can_fire()
            && self.fire_cooldown == 0
            && has_knife
        {
            let dir = if self.info1 < 0 { -1 } else { 1 };
            let bullet_x = if dir > 0 {
                self.x + self.w
            } else {
                // Left-facing: place the knife's box just left of the player,
                // sized to the real 10x10 knife sprite (not a full block).
                self.x - crate::entities::objects::bullet::KNIFE_W
            };
            dispatcher.send(
                MessageType::CreateObject,
                MessagePayload::SpawnAt {
                    object_type: 36,
                    x: bullet_x,
                    // Java `KniveManager` applies `this.y += initY` (initY = 2)
                    // on launch so the knife leaves the hand slightly lowered.
                    y: self.y + KNIFE_SPAWN_Y_OFFSET,
                    xd: dir * BULLET_SPEED_PX,
                    yd: 0,
                },
            );
            // Knife temporarily leaves the inventory on throw so the player
            // cannot spam the attack. The `BulletEntity` re-dispatches an
            // `InventoryItem(add Knife)` if its follow phase brings the
            // projectile back into contact with the player.
            dispatcher.send(
                MessageType::InventoryItem,
                MessagePayload::InventoryItem(openjill_core::InventoryItemPayload::remove(
                    InventoryObject::Knife,
                )),
            );
            self.fire_cooldown = FIRE_COOLDOWN_TICKS;
        }

        // Promote a pending die request before any state-specific handlers run
        // so the bullet burst lands on the same tick the kill arrived.
        if self.die_pending {
            self.die_pending = false;
            self.enter_die_state(dispatcher);
        }

        match self.state {
            PlayerStateKind::Stand | PlayerStateKind::Still => {
                self.tick_stand(input, backgrounds);
            }
            PlayerStateKind::Jumping => {
                self.tick_jumping(input, backgrounds);
            }
            PlayerStateKind::Climbing => {
                self.tick_climbing(input, backgrounds);
            }
            PlayerStateKind::Begin => {
                self.tick_begin();
            }
            PlayerStateKind::Die => {
                self.tick_die(dispatcher);
            }
        }
    }

    /// Returns the player's render command for the current frame.
    ///
    /// Sprite selection mirrors `PlayerManager.msgDraw`: the active state
    /// chooses the base tile and the sub-state / facing index advances within
    /// the per-state frame range.
    fn draw(&self) -> Option<RenderCommand> {
        let tile = self.current_tile()?;
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Player touch dispatch is handled by the touching object's `on_touch`
    /// path (enemies push `InventoryLifeMessage`); the player itself is inert.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Begins the die animation.
    ///
    /// Stores the classification for the upcoming `Die` transition and arms
    /// the bullet burst.  The trait's `on_kill` signature lacks a dispatcher
    /// so the actual state mutation and `CreateObject` dispatch run on the
    /// next [`ObjectEntity::update`] call via [`Self::die_pending`].
    fn on_kill(&mut self, _damage: i32, death_kind: DeathKind) {
        if matches!(self.state, PlayerStateKind::Die) {
            return;
        }
        self.death_kind = Some(death_kind);
        self.die_pending = true;
    }

    /// Returns the player's bounding box for collision tests and culling.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Always active so the player keeps ticking outside the viewport border.
    fn always_active(&self) -> bool {
        true
    }

    /// Returns `true`: this entity represents the controllable player.
    fn is_player(&self) -> bool {
        true
    }

    /// Applies a platform-driven position delta without collision checking.
    fn apply_platform_move(&mut self, dx: i32, dy: i32) {
        self.x += dx;
        self.y += dy;
    }

    /// Serializes the live player state back into its origin JN record.
    ///
    /// Writes back every field the player model tracks (position, speeds,
    /// state, sub-state, state-count, `info1`, `zap_hold`); the authored fields
    /// the player does not mutate (`counter`, `flags`, `pointer`, and the
    /// dimensions - `new` normalizes `w`/`h` to at least `BLOCK_SIZE_I` for
    /// collision but never changes them afterward) are preserved verbatim from
    /// the cloned origin so saves stay byte-stable.
    fn snapshot(&self) -> Option<JnObject> {
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        obj.set_speed(self.x_speed as i16, self.y_speed as i16);
        obj.set_state(self.state.to_state_code());
        obj.set_sub_state(self.sub_state as u16);
        obj.set_state_count(self.state_count as u16);
        obj.set_info1(self.info1 as i16);
        obj.set_zap_hold(self.zaphold as u16);
        Some(obj)
    }
}

impl PlayerEntity {
    /// Applies a `PlayerMove` delta dispatched by a lift or other moving
    /// platform.
    ///
    /// Called by the level loop when a [`MessageType::PlayerMove`] message
    /// arrives with a [`MessagePayload::Move`] payload.  The delta is applied
    /// directly to the player's world position without collision checking,
    /// matching the Java reference's `AbstractPlayerManager.msgPlayerMove`
    /// behavior where the lift guarantees the move is within open space.
    pub fn apply_platform_move(&mut self, dx: i32, dy: i32) {
        self.x += dx;
        self.y += dy;
    }
}

impl PlayerEntity {
    /// Sets `self.zaphold` to the post-touch cooldown value.
    ///
    /// Exposed for the broader entity layer: when an enemy registers a hit on
    /// the player, the entity calls this so subsequent ticks skip the same
    /// collision pair until the cooldown drains.
    pub fn arm_zaphold(&mut self) {
        self.zaphold = ZAPHOLD_AFTER_TOUCH as i32;
    }

    /// Returns the current zaphold cooldown counter.
    pub fn zaphold(&self) -> i32 {
        self.zaphold
    }

    /// Returns the recorded death classification, if [`Self::on_kill`] has
    /// fired but the die-state transition has not yet run.
    ///
    /// Exposed for tests and debug overlays that need to observe which
    /// hazard arming an on-kill was last seen; production code reads the
    /// final classification off the player's `Die` sub-state once the
    /// transition completes.
    pub fn death_kind(&self) -> Option<DeathKind> {
        self.death_kind
    }

    /// Per-tick handler for the `Stand` and `Still` states.
    ///
    /// Mirrors `moveStdPlayerUpDownStand` + `moveStdPlayerLeftRightStand` from
    /// the Java reference: tests floor presence, dispatches jump / climb /
    /// duck transitions, and otherwise integrates the horizontal run.
    fn tick_stand(&mut self, input: &ActiveInput, backgrounds: &BackgroundGrid) {
        let floor = has_floor_below(backgrounds, self.x, self.y, self.w, self.h);

        if floor {
            if input.contains(&InputCommand::Jump) {
                self.enter_jump_from_ground();
            } else if input.contains(&InputCommand::Up)
                && is_on_climbable(backgrounds, self.x, self.y, self.w, self.h)
            {
                self.enter_climb_from_stand();
                self.state_count = self.state_count.saturating_add(1);
                return;
            } else {
                self.y_speed = 0;
            }
        } else {
            // No floor below: fall.
            self.state = PlayerStateKind::Jumping;
            self.sub_state = 0;
            // Skip the rising animation so falls behave like the Java
            // `setSubState(0)` followed immediately by gravity once the
            // sub-state climbs past `SUBSTATE_VALUE_TO_FALL`.
            self.y_speed = 0;

            if is_on_climbable(backgrounds, self.x, self.y, self.w, self.h)
                && input.contains(&InputCommand::Up)
            {
                // Falling onto a vine is an airborne-to-climb transition, so
                // use the jump-flavored entry (no `CLIMB_UP_STEPS[3]` nudge)
                // to match the rest of the jump→climb path.
                self.enter_climb_from_jump();
                self.state_count = self.state_count.saturating_add(1);
                return;
            }
        }

        if matches!(self.state, PlayerStateKind::Stand | PlayerStateKind::Still) {
            self.run_horizontal_stand(input, backgrounds);
        }

        self.state_count = self.state_count.saturating_add(1);
    }

    /// Per-tick handler for the `Jumping` state.
    ///
    /// Mirrors `moveStdPlayerUpDownJumping` + `moveStdPlayerLeftRightJumping`
    /// plus the `msgUpdateJumping` counter bumps: applies gravity once the
    /// initial rise animation has played out, attempts the vertical move, and
    /// reacts to wall / floor contacts.
    fn tick_jumping(&mut self, input: &ActiveInput, backgrounds: &BackgroundGrid) {
        let mut moved = false;
        let mut land = false;

        if self.y_speed > 0 {
            // Falling phase: attempt the down step; if blocked, land.
            if try_move_vertical(
                backgrounds,
                self.x,
                &mut self.y,
                self.w,
                self.h,
                self.y_speed,
            ) {
                moved = true;
            } else {
                land = true;
            }
        } else if self.y_speed < 0 && self.sub_state >= SUBSTATE_VALUE_TO_FALL {
            // Rising phase past the bobble: integrate by `y_speed + JUMP_INCREMENT_VALUE`
            // (the Java reference uses the same expression for the move-up
            // amount because the acceleration is applied to the same tick).
            let dy = self.y_speed + JUMP_INCREMENT_VALUE;
            if try_move_vertical(backgrounds, self.x, &mut self.y, self.w, self.h, dy) {
                moved = true;
            } else {
                self.y_speed = 0;
            }
        }

        if land {
            self.state = PlayerStateKind::Stand;
            self.sub_state = 0;
            self.state_count = 0;
            self.y_speed = 0;
            // Drop horizontal influence on landing so the stand-frame
            // selector does not render a one-tick run sprite when no
            // direction key is held this tick.
            self.x_speed = 0;
        }

        // Horizontal influence is only sampled once the rising animation has
        // played out (Java: `if (getSubState() > SUBSTATE_VALUE_TO_FALL)`),
        // matching the reference's "lock direction during the initial frames"
        // feel.
        if matches!(self.state, PlayerStateKind::Jumping) && self.sub_state > SUBSTATE_VALUE_TO_FALL
        {
            if input.contains(&InputCommand::MoveLeft) {
                try_move_vertical_horizontal(
                    backgrounds,
                    &mut self.x,
                    self.y,
                    self.w,
                    self.h,
                    -PLAYER_MOVE_SIZE,
                );
                self.x_speed = -1;
                self.info1 = -1;
            } else if input.contains(&InputCommand::MoveRight) {
                try_move_vertical_horizontal(
                    backgrounds,
                    &mut self.x,
                    self.y,
                    self.w,
                    self.h,
                    PLAYER_MOVE_SIZE,
                );
                self.x_speed = 1;
                self.info1 = 1;
            } else {
                self.x_speed = 0;
            }
        }

        // Vine grab while airborne mirrors the Java fall-into-climb branch in
        // `moveStdPlayerUpDownJumping`.
        if moved
            && matches!(self.state, PlayerStateKind::Jumping)
            && is_on_climbable(backgrounds, self.x, self.y, self.w, self.h)
            && input.contains(&InputCommand::Up)
        {
            self.enter_climb_from_jump();
        }

        // Gravity accumulation past the bobble.
        if matches!(self.state, PlayerStateKind::Jumping)
            && self.sub_state >= SUBSTATE_VALUE_TO_FALL
            && self.y_speed < JUMP_FALLING_SPEED_LIMIT
        {
            self.y_speed += JUMP_INCREMENT_VALUE;
            if self.y_speed > JUMP_FALLING_SPEED_LIMIT {
                self.y_speed = JUMP_FALLING_SPEED_LIMIT;
            }
        }

        if matches!(self.state, PlayerStateKind::Jumping) {
            self.sub_state = self.sub_state.saturating_add(1);
            self.state_count = self.state_count.saturating_add(1);
        }
    }

    /// Per-tick handler for the `Climbing` state.
    fn tick_climbing(&mut self, input: &ActiveInput, backgrounds: &BackgroundGrid) {
        // Jump-out from a climb takes priority over up/down movement so the
        // reference's "press jump while climbing" feels responsive.
        if input.contains(&InputCommand::Jump) {
            self.enter_jump_from_climb();
            return;
        }

        if input.contains(&InputCommand::Up) {
            let next_sub = if self.sub_state + 1
                < i32::try_from(CLIMB_UP_STEPS.len()).expect("climb step count fits in i32")
            {
                self.sub_state + 1
            } else {
                1
            };
            self.sub_state = next_sub;
            let dy = CLIMB_UP_STEPS[next_sub as usize];
            try_move_vertical(backgrounds, self.x, &mut self.y, self.w, self.h, dy);
        } else if input.contains(&InputCommand::Duck) {
            self.sub_state = CLIMB_SUBSTATE_DOWN;
            try_move_vertical(
                backgrounds,
                self.x,
                &mut self.y,
                self.w,
                self.h,
                PLAYER_MOVE_SIZE_CLIMB_DOWN,
            );
        }

        if input.contains(&InputCommand::MoveLeft) {
            try_move_vertical_horizontal(
                backgrounds,
                &mut self.x,
                self.y,
                self.w,
                self.h,
                -PLAYER_MOVE_SIZE,
            );
        } else if input.contains(&InputCommand::MoveRight) {
            try_move_vertical_horizontal(
                backgrounds,
                &mut self.x,
                self.y,
                self.w,
                self.h,
                PLAYER_MOVE_SIZE,
            );
        }

        if !is_on_climbable(backgrounds, self.x, self.y, self.w, self.h) {
            // Slid off the vine: drop into Jumping so gravity takes over.
            self.state = PlayerStateKind::Jumping;
            self.sub_state = SUBSTATE_VALUE_TO_FALL;
            self.y_speed = 0;
        }

        self.state_count = self.state_count.saturating_add(1);
    }

    /// Per-tick handler for the `Begin` (level-entry animation) state.
    ///
    /// The reference plays an 18-tick head-up / head-down animation before
    /// dropping into [`PlayerStateKind::Stand`]; this implementation matches
    /// the duration but keeps the animation visual minimal.
    fn tick_begin(&mut self) {
        self.state_count = self.state_count.saturating_add(1);
        if self.state_count >= 18 {
            self.state = PlayerStateKind::Stand;
            self.sub_state = 0;
            self.state_count = 0;
        }
    }

    /// Per-tick handler for the `Die` state.
    ///
    /// Increments the die counter and dispatches `DieRestartLevel` exactly
    /// once when the counter reaches [`STATECOUNT_MAX_TO_RESTART_GAME`].  The
    /// counter advances on every tick (including the dispatching tick) so the
    /// strict equality guard fires a single message even if `LevelScreen`
    /// keeps ticking the entity during the 72-tick message-box hold.
    fn tick_die(&mut self, dispatcher: &mut MessageDispatcher) {
        if self.state_count == STATECOUNT_MAX_TO_RESTART_GAME {
            dispatcher.send(MessageType::DieRestartLevel, MessagePayload::None);
        }
        self.state_count = self.state_count.saturating_add(1);
    }

    /// Enters the `Jumping` state from `Stand`, applying the standard initial
    /// y-speed.
    ///
    /// High-jump pickup support (the `HIGH_JUMP` inventory item that adds
    /// `HIGH_JUMP_STEP_SIZE = 4` to the magnitude) lands with the inventory
    /// hookup in child issue 4 of epic 6.
    fn enter_jump_from_ground(&mut self) {
        self.state = PlayerStateKind::Jumping;
        self.sub_state = 0;
        self.state_count = 0;
        self.y_speed = -JUMP_INIT_SIZE;
    }

    /// Enters the `Jumping` state from `Climbing`, applying the climb-jump
    /// initial y-speed.
    fn enter_jump_from_climb(&mut self) {
        self.state = PlayerStateKind::Jumping;
        self.sub_state = 0;
        self.state_count = 0;
        self.y_speed = -JUMP_INIT_SIZE_FOR_CLIMB;
    }

    /// Enters the `Climbing` state from `Stand`.
    ///
    /// The Java reference nudges the player up by `CLIMB_UP_STEPS[3]` when
    /// the transition originates from stand so the climb sprite hugs the
    /// vine; we mirror that here.
    fn enter_climb_from_stand(&mut self) {
        self.state = PlayerStateKind::Climbing;
        self.sub_state = CLIMB_SUBSTATE_ENTER;
        self.state_count = 0;
        self.x_speed = 0;
        self.y_speed = 0;
        self.y += CLIMB_UP_STEPS[3];
    }

    /// Enters the `Climbing` state from `Jumping`.
    ///
    /// Mirrors the Java reference's `changeToClimbState`: stops vertical
    /// movement, sets the entry sub-state, and uses the `STOP` step (index 0,
    /// value 0) so the player snaps in place.
    fn enter_climb_from_jump(&mut self) {
        self.state = PlayerStateKind::Climbing;
        self.sub_state = CLIMB_SUBSTATE_ENTER;
        self.state_count = 0;
        self.y_speed = 0;
        self.y += CLIMB_UP_STEPS[CLIMB_SUBSTATE_STOP as usize];
    }

    /// Promotes a pending die request: switches to `Die`, fires the bullet
    /// burst, seeds the death frame counters, and resets vertical speed to
    /// the reference's `START_YD`.
    fn enter_die_state(&mut self, dispatcher: &mut MessageDispatcher) {
        self.state = PlayerStateKind::Die;
        self.sub_state = match self.death_kind {
            Some(DeathKind::Enemy) | None => 0,
            Some(DeathKind::Water) => 1,
            Some(DeathKind::OtherBackground) => 2,
        };
        self.state_count = 0;
        self.y_speed = DIE_START_YD;
        self.x_speed = 0;

        // Bullet burst: dispatched as `NB_COLORED_BULLET` separate
        // `CreateObject` messages.  Until child issue 7 lands a structured
        // payload these carry `MessagePayload::None`; downstream handlers
        // will switch on the message type alone.
        for _ in 0..NB_COLORED_BULLET {
            dispatcher.send(MessageType::CreateObject, MessagePayload::None);
        }
    }

    /// Updates horizontal movement for the `Stand` state.
    ///
    /// Mirrors the body of `moveStdPlayerLeftRightStand` + `moveStdPlayerLeftRightStandCommon`
    /// from the Java reference: the first press just turns the sprite; the
    /// second tick (or a sustained press once turned) actually moves.
    fn run_horizontal_stand(&mut self, input: &ActiveInput, backgrounds: &BackgroundGrid) {
        let direction = if input.contains(&InputCommand::MoveLeft) {
            -1
        } else if input.contains(&InputCommand::MoveRight) {
            1
        } else {
            0
        };

        if direction == 0 {
            self.x_speed = 0;
            return;
        }

        if self.info1 != direction {
            // First press in this direction: turn the sprite, no movement.
            self.info1 = direction;
            self.x_speed = 0;
            self.state_count = 0;
            return;
        }

        if self.x_speed == direction {
            // Sustained run: advance the animation frame and step.
            let max_frame = 7; // PICTURE_RUNNING_NUMBER - 1 = 8 - 1
            self.sub_state = if self.sub_state >= max_frame {
                0
            } else {
                self.sub_state + 1
            };

            let dx = direction * PLAYER_MOVE_SIZE;
            try_move_vertical_horizontal(backgrounds, &mut self.x, self.y, self.w, self.h, dx);
        } else {
            // Just-turned to a known direction: start the run on the next tick.
            self.x_speed = direction;
            self.sub_state = 0;
        }

        self.state_count = 0;
    }

    /// Resolves the SHA tile to blit for the current state and sub-state.
    ///
    /// Returns `None` only when no frame is available, which should not
    /// happen in normal play; the caller suppresses the draw command in that
    /// case so the player simply disappears for the affected frame.
    fn current_tile(&self) -> Option<u16> {
        match self.state {
            PlayerStateKind::Stand | PlayerStateKind::Still | PlayerStateKind::Begin => {
                Some(self.tile_for_stand())
            }
            PlayerStateKind::Jumping => Some(self.tile_for_jumping()),
            PlayerStateKind::Climbing => {
                let idx = self.sub_state.clamp(0, TILE_CLIMB.len() as i32 - 1) as usize;
                Some(TILE_CLIMB[idx])
            }
            PlayerStateKind::Die => {
                let idx =
                    (self.state_count / DIE_STATECOUNT_STEP).clamp(0, DIE_FRAME_COUNT - 1) as u16;
                Some(TILE_DIE_BASE + idx)
            }
        }
    }

    /// Returns the stand / still / begin frame's tile index.
    fn tile_for_stand(&self) -> u16 {
        if self.x_speed == 0 {
            match self.info1 {
                d if d < 0 => TILE_STAND_LEFT,
                d if d > 0 => TILE_STAND_RIGHT,
                _ => TILE_STAND_MIDDLE,
            }
        } else if self.x_speed < 0 {
            let frame = (self.sub_state.rem_euclid(8)) as u16;
            TILE_RUN_LEFT_BASE + frame
        } else {
            let frame = (self.sub_state.rem_euclid(8)) as u16;
            TILE_RUN_RIGHT_BASE + frame
        }
    }

    /// Returns the jumping frame's tile index.
    ///
    /// Mirrors `msgDrawJumping`: while rising (`sub_state < SUBSTATE_VALUE_TO_FALL`)
    /// the per-direction frame is selected by `sub_state`; once past the
    /// bobble the player either shows the fall silhouette (positive y_speed)
    /// or the third frame of the directional jump (still rising).
    fn tile_for_jumping(&self) -> u16 {
        if self.sub_state >= SUBSTATE_VALUE_TO_FALL && self.y_speed > 0 {
            return TILE_FALL;
        }
        let frame = self.sub_state.clamp(0, 2) as u16;
        // Fall back to `info1` (last facing) when `x_speed` is zero so the
        // jump sprite preserves facing even when no horizontal key is held
        // this tick.  Mirrors the stand-frame selector's facing fallback.
        let facing = if self.x_speed != 0 {
            self.x_speed
        } else {
            self.info1
        };
        match facing {
            d if d < 0 => TILE_JUMP_LEFT_BASE + frame,
            d if d > 0 => TILE_JUMP_RIGHT_BASE + frame,
            _ => TILE_JUMP_MIDDLE_BASE + frame,
        }
    }
}

// ---------------------------------------------------------------------------
// Background collision helpers
// ---------------------------------------------------------------------------

/// Returns `true` when the cell directly below the player's bounding box is
/// solid for a falling player.
///
/// Mirrors `UtilityObjectEntity.checkIfFloorUnderObject` from the Java
/// reference: probe one pixel below the bottom edge and check every cell
/// covered by the bounding box footprint at that row.  Only cells that report
/// `blocks_vertical(1)` are treated as floor; passthrough cells never count
/// even when `is_stair` is set.  Direction-aware cells (`FROOF` / `FFLOOR`)
/// consult [`openjill_core::BackgroundEntity::blocks_vertical`] with a positive
/// `player_yd` so a falling player lands on `FROOF` but still drops through
/// `FFLOOR`.
fn has_floor_below(grid: &BackgroundGrid, x: i32, y: i32, w: i32, h: i32) -> bool {
    let probe_y = y + h;
    let cell_y = probe_y.div_euclid(BLOCK_SIZE_I);
    if cell_y < 0 || (cell_y as usize) >= grid.height {
        return false;
    }
    let cell_y = cell_y as usize;
    let cell_x_left = x.div_euclid(BLOCK_SIZE_I).max(0);
    let cell_x_right = (x + w - 1).div_euclid(BLOCK_SIZE_I);
    if cell_x_right < 0 {
        return false;
    }
    let cell_x_left = cell_x_left as usize;
    let cell_x_right = (cell_x_right as usize).min(grid.width.saturating_sub(1));
    for cx in cell_x_left..=cell_x_right {
        if let Some(cell) = grid.get(cx, cell_y)
            && cell.blocks_vertical(1)
        {
            return true;
        }
    }
    false
}

/// Returns `true` when the player can grab a vine at this position.
///
/// Faithful port of `UtilityObjectEntity.isClimbing`: the grab only engages
/// when the player's X is aligned to a block column (`x % blockSize == 0`);
/// then any climbable (`isVine`) cell spanned by the bounding box counts.
fn is_on_climbable(grid: &BackgroundGrid, x: i32, y: i32, w: i32, h: i32) -> bool {
    if x.rem_euclid(BLOCK_SIZE_I) != 0 {
        return false;
    }
    let start_x = x.div_euclid(BLOCK_SIZE_I);
    let end_x = (x + w - 1).div_euclid(BLOCK_SIZE_I);
    let start_y = y.div_euclid(BLOCK_SIZE_I);
    let end_y = (y + h - 1).div_euclid(BLOCK_SIZE_I);
    for cx in start_x..=end_x {
        for cy in start_y..=end_y {
            if cx < 0 || cy < 0 {
                continue;
            }
            if grid
                .get(cx as usize, cy as usize)
                .map(openjill_core::BackgroundEntity::is_climbable)
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

/// Returns `true` when the rectangle `[x, x+w) x [y, y+h)` overlaps any cell
/// that is not passable for a vertical step in the direction of `player_yd`.
///
/// Direction-aware cells (`FROOF` / `FFLOOR`) consult
/// [`openjill_core::BackgroundEntity::blocks_vertical`] so a fake floor blocks
/// only upward motion and a fake roof blocks only downward motion. Passthrough
/// cells never collide regardless of `is_stair`.
fn collides_vertical(
    grid: &BackgroundGrid,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    player_yd: i32,
) -> bool {
    let cx_l = x.div_euclid(BLOCK_SIZE_I);
    let cx_r = (x + w - 1).div_euclid(BLOCK_SIZE_I);
    let cy_t = y.div_euclid(BLOCK_SIZE_I);
    let cy_b = (y + h - 1).div_euclid(BLOCK_SIZE_I);
    let cx_l = cx_l.max(0) as usize;
    let cx_r = cx_r.max(0) as usize;
    let cy_t = cy_t.max(0) as usize;
    let cy_b = cy_b.max(0) as usize;
    for cy in cy_t..=cy_b {
        if cy >= grid.height {
            continue;
        }
        for cx in cx_l..=cx_r {
            if cx >= grid.width {
                continue;
            }
            if let Some(cell) = grid.get(cx, cy)
                && cell.blocks_vertical(player_yd)
            {
                return true;
            }
        }
    }
    false
}

/// Returns `true` when the rectangle `[x, x+w) x [y, y+h)` overlaps any cell
/// that blocks horizontal motion.
///
/// Passthrough cells (including passthrough stair tiles like `BLSHADE*`) never
/// block horizontal motion.
fn collides_horizontal(grid: &BackgroundGrid, x: i32, y: i32, w: i32, h: i32) -> bool {
    let cx_l = x.div_euclid(BLOCK_SIZE_I);
    let cx_r = (x + w - 1).div_euclid(BLOCK_SIZE_I);
    let cy_t = y.div_euclid(BLOCK_SIZE_I);
    let cy_b = (y + h - 1).div_euclid(BLOCK_SIZE_I);
    let cx_l = cx_l.max(0) as usize;
    let cx_r = cx_r.max(0) as usize;
    let cy_t = cy_t.max(0) as usize;
    let cy_b = cy_b.max(0) as usize;
    for cy in cy_t..=cy_b {
        if cy >= grid.height {
            continue;
        }
        for cx in cx_l..=cx_r {
            if cx >= grid.width {
                continue;
            }
            if let Some(cell) = grid.get(cx, cy)
                && !cell.is_passthrough()
            {
                return true;
            }
        }
    }
    false
}

/// Attempts a vertical step, snapping flush to floor/ceiling on collision.
///
/// Falling (`dy > 0`): scans rows between current feet and destination feet;
/// on the first blocking row snaps `y` so the player's feet sit exactly on
/// that row's top edge (matching Java `moveObjectDown`).  Returns `false` when
/// a floor was hit so callers can land the player, even though `y` may have
/// advanced part of the way.
/// Rising (`dy < 0`): slides up pixel by pixel and stops flush below the first
/// ceiling cell, mirroring Java `moveObjectUp` (a partial move that snaps to
/// `(block.getY() + 1) * blockSize`).  Returns `true` when `y` changed.
fn try_move_vertical(grid: &BackgroundGrid, x: i32, y: &mut i32, w: i32, h: i32, dy: i32) -> bool {
    if dy > 0 {
        let cx_l = x.div_euclid(BLOCK_SIZE_I).max(0) as usize;
        let cx_r = ((x + w - 1).div_euclid(BLOCK_SIZE_I)).max(0) as usize;
        let feet = *y + h;
        let start_row = feet.div_euclid(BLOCK_SIZE_I);
        let end_row = (feet + dy - 1).div_euclid(BLOCK_SIZE_I);
        for row in start_row..=end_row {
            if row < 0 {
                continue;
            }
            let row_u = row as usize;
            if row_u >= grid.height {
                break;
            }
            let blocked = (cx_l..=cx_r).any(|cx| {
                cx < grid.width
                    && grid
                        .get(cx, row_u)
                        .map(|cell| cell.blocks_vertical(dy))
                        .unwrap_or(false)
            });
            if blocked {
                let snapped = row * BLOCK_SIZE_I - h;
                if snapped != *y {
                    *y = snapped;
                }
                return false;
            }
        }
        *y += dy;
        true
    } else {
        // Rising: advance one pixel at a time so the player snaps flush below a
        // ceiling instead of stopping a whole step short (Java `moveObjectUp`).
        let target = (*y + dy).max(0);
        let mut moved = false;
        while *y > target {
            let next = *y - 1;
            if collides_vertical(grid, x, next, w, h, dy) {
                break;
            }
            *y = next;
            moved = true;
        }
        moved
    }
}

/// Attempts a horizontal step, sliding flush to walls on collision.
///
/// Mirrors Java `moveObjectLeft`/`moveObjectRight`: a partial move that snaps
/// the player against the blocking column instead of refusing the whole step.
/// Advances one pixel at a time and clamps to the map bounds.  Returns `true`
/// when `x` changed.
fn try_move_vertical_horizontal(
    grid: &BackgroundGrid,
    x: &mut i32,
    y: i32,
    w: i32,
    h: i32,
    dx: i32,
) -> bool {
    let max_x = (grid.width as i32) * BLOCK_SIZE_I - w;
    let target = (*x + dx).clamp(0, max_x.max(0));
    let step = (target - *x).signum();
    let mut moved = false;
    while *x != target {
        let next = *x + step;
        if collides_horizontal(grid, next, y, w, h) {
            break;
        }
        *x = next;
        moved = true;
    }
    moved
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_core::{
        BackgroundEntity, BackgroundGrid, DeathKind, InputCommand, MessageDispatcher,
        MessageHandler, MessagePayload, MessageType, ObjectEntity, RuntimeState,
    };
    use openjill_data::jn::JnFile;
    use std::sync::{Arc, Mutex};

    /// Background cell variant used by the tests.
    #[derive(Clone, Copy)]
    enum CellKind {
        /// Open air: passable, not climbable, not solid.
        Air,
        /// Solid block: not passable.
        Solid,
        /// Vine: passable and climbable.
        Vine,
    }

    /// Background cell implementation for tests.
    struct TestCell {
        /// Behavior flag.
        kind: CellKind,
    }

    impl BackgroundEntity for TestCell {
        fn draw(&self, _screen_x: i32, _screen_y: i32) -> Option<RenderCommand> {
            None
        }

        fn update(&mut self, _cell_x: i32, _cell_y: i32, _dispatcher: &mut MessageDispatcher) {}

        fn on_player_touch(
            &mut self,
            _player: &mut dyn ObjectEntity,
            _dispatcher: &mut MessageDispatcher,
        ) {
        }

        fn is_passthrough(&self) -> bool {
            !matches!(self.kind, CellKind::Solid)
        }

        fn is_climbable(&self) -> bool {
            matches!(self.kind, CellKind::Vine)
        }

        fn is_stair(&self) -> bool {
            false
        }
    }

    /// Builds a square synthetic background grid filled with `kind` cells.
    fn synthetic_grid(width: usize, height: usize, kind: CellKind) -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(height);
        for _ in 0..height {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(width);
            for _ in 0..width {
                row.push(Box::new(TestCell { kind }));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    /// Replaces the cell at `(x, y)` with one of the supplied `kind`.
    fn set_cell(grid: &mut BackgroundGrid, x: usize, y: usize, kind: CellKind) {
        grid.cells[y][x] = Box::new(TestCell { kind });
    }

    /// Builds a default `PlayerEntity` for tests at `(x, y)` with a 16x16
    /// bounding box and a synthetic `AssetCache`.
    fn make_player(x: i32, y: i32) -> PlayerEntity {
        const OBJECT_RECORD_BYTES: usize = 31;
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off + 1..record_off + 3].copy_from_slice(&(x as u16).to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&(y as u16).to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        let cache = AssetCache::synthetic();
        PlayerEntity::new(&jn.objects()[0], &cache)
    }

    /// Test helper: records each delivered message into a shared buffer.
    struct Recorder(Arc<Mutex<Vec<(MessageType, MessagePayload)>>>);

    impl MessageHandler for Recorder {
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Unit under test: Stand + Jump input transitions to Jumping with
    /// `y_speed = -16`.
    ///
    /// Preconditions: player at `(16, 0)` with a solid floor in the row
    /// directly below; input set carries [`InputCommand::Jump`].
    ///
    /// Invariants asserted: after one tick the state is [`PlayerStateKind::Jumping`]
    /// and `y_speed = -JUMP_INIT_SIZE`.
    #[test]
    fn stand_jump_transitions_to_jumping_with_initial_yspeed() {
        let mut grid = synthetic_grid(8, 4, CellKind::Air);
        // Floor at cell row 1 below the player.
        for cx in 0..8 {
            set_cell(&mut grid, cx, 1, CellKind::Solid);
        }
        let mut player = make_player(16, 0);
        let runtime = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        let mut input = ActiveInput::new();
        input.insert(InputCommand::Jump);

        player.update(&input, &runtime, &grid, &mut dispatcher);

        assert_eq!(player.state(), PlayerStateKind::Jumping);
        assert_eq!(player.y_speed(), -JUMP_INIT_SIZE);
    }

    /// Unit under test: [`try_move_vertical_horizontal`].
    ///
    /// Invariants asserted: a horizontal step into a wall slides flush against
    /// the blocking column (partial move, Java `moveObjectRight`) instead of
    /// refusing the whole step; once flush, a further step makes no progress.
    #[test]
    fn horizontal_move_slides_flush_to_wall() {
        let mut grid = synthetic_grid(8, 8, CellKind::Air);
        // Solid wall column at cell x = 3 (pixels 48..64).
        for cy in 0..8 {
            set_cell(&mut grid, 3, cy, CellKind::Solid);
        }
        // Player (16 wide) at x = 24; the wall's left edge is at 48, so the
        // player can advance until its right edge is flush at x = 32.
        let mut x = 24;
        let moved = try_move_vertical_horizontal(&grid, &mut x, 16, 16, 16, 8);
        assert!(moved, "player should slide toward the wall");
        assert_eq!(x, 32, "player snaps flush to the wall (32 + 16 == 48)");

        let moved_again = try_move_vertical_horizontal(&grid, &mut x, 16, 16, 16, 8);
        assert!(!moved_again, "flush against the wall: no further progress");
        assert_eq!(x, 32);
    }

    /// Unit under test: [`try_move_vertical`] rising branch.
    ///
    /// Invariants asserted: a rising step into a ceiling snaps the player's top
    /// edge flush below the ceiling cell (partial move, Java `moveObjectUp`)
    /// rather than stopping a whole step short.
    #[test]
    fn rising_move_snaps_flush_below_ceiling() {
        let mut grid = synthetic_grid(8, 8, CellKind::Air);
        // Solid ceiling row at cell y = 1 (pixels 16..32).
        for cx in 0..8 {
            set_cell(&mut grid, cx, 1, CellKind::Solid);
        }
        // Player top at y = 40; rising by 12 would reach y = 28, but the
        // ceiling bottom is at 32, so the top snaps flush to y = 32.
        let mut y = 40;
        let moved = try_move_vertical(&grid, 16, &mut y, 16, 16, -12);
        assert!(moved, "player should rise toward the ceiling");
        assert_eq!(y, 32, "player snaps flush below the ceiling");
    }

    /// Unit under test: jumping `y_speed` accelerates by
    /// [`JUMP_INCREMENT_VALUE`] per tick and saturates at
    /// [`JUMP_FALLING_SPEED_LIMIT`].
    ///
    /// Preconditions: player in `Jumping` state with `y_speed = -16` and
    /// `sub_state = SUBSTATE_VALUE_TO_FALL` so gravity is already active; no
    /// surrounding solid cells so vertical movement always succeeds.
    ///
    /// Invariants asserted: after enough ticks `y_speed` reaches the cap and
    /// stays there.
    #[test]
    fn jumping_yspeed_accelerates_and_caps_at_fall_limit() {
        let grid = synthetic_grid(64, 64, CellKind::Air);
        let mut player = make_player(16, 16);
        player.set_state(PlayerStateKind::Jumping);
        player.sub_state = SUBSTATE_VALUE_TO_FALL;
        player.set_y_speed(-JUMP_INIT_SIZE);
        let runtime = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        let input = ActiveInput::new();

        for _ in 0..80 {
            player.update(&input, &runtime, &grid, &mut dispatcher);
        }

        assert_eq!(
            player.y_speed(),
            JUMP_FALLING_SPEED_LIMIT,
            "y_speed must saturate at JUMP_FALLING_SPEED_LIMIT"
        );
    }

    /// Unit under test: standing without a floor below transitions to
    /// `Jumping`.
    ///
    /// Preconditions: player at `(16, 0)` over an entirely passable grid; no
    /// input.
    ///
    /// Invariants asserted: after one tick the state is
    /// [`PlayerStateKind::Jumping`].
    #[test]
    fn stand_without_floor_transitions_to_jumping() {
        let grid = synthetic_grid(8, 8, CellKind::Air);
        let mut player = make_player(16, 0);
        let runtime = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        let input = ActiveInput::new();

        player.update(&input, &runtime, &grid, &mut dispatcher);

        assert_eq!(player.state(), PlayerStateKind::Jumping);
    }

    /// Unit under test: a falling player landing on a floor cell transitions
    /// back to `Stand`.
    ///
    /// Preconditions: player just above a solid row in the `Jumping` state
    /// with a positive `y_speed` that would land them onto the row; sub-state
    /// past the rising-animation gate.
    ///
    /// Invariants asserted: after one tick state is [`PlayerStateKind::Stand`].
    #[test]
    fn jumping_with_positive_yspeed_lands_on_floor() {
        let mut grid = synthetic_grid(8, 4, CellKind::Air);
        for cx in 0..8 {
            set_cell(&mut grid, cx, 1, CellKind::Solid);
        }
        let mut player = make_player(16, 0);
        player.set_state(PlayerStateKind::Jumping);
        player.sub_state = SUBSTATE_VALUE_TO_FALL;
        player.set_y_speed(8);
        let runtime = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        let input = ActiveInput::new();

        player.update(&input, &runtime, &grid, &mut dispatcher);

        assert_eq!(player.state(), PlayerStateKind::Stand);
    }

    /// Unit under test: `Up` input on a vine cell transitions `Stand` to
    /// `Climbing`.
    ///
    /// Preconditions: player on a vine cell (climbable, passable); input set
    /// carries [`InputCommand::Up`].
    ///
    /// Invariants asserted: state becomes [`PlayerStateKind::Climbing`].
    #[test]
    fn up_input_on_vine_transitions_to_climbing() {
        let mut grid = synthetic_grid(8, 8, CellKind::Air);
        set_cell(&mut grid, 1, 1, CellKind::Vine);
        let mut player = make_player(16, 16);
        let runtime = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        let mut input = ActiveInput::new();
        input.insert(InputCommand::Up);

        player.update(&input, &runtime, &grid, &mut dispatcher);

        assert_eq!(player.state(), PlayerStateKind::Climbing);
    }

    /// Unit under test: pressing Jump while in the `Climbing` state
    /// transitions to `Jumping` with `y_speed = -12`.
    ///
    /// Preconditions: player on a vine cell already in `Climbing`; input set
    /// carries [`InputCommand::Jump`].
    ///
    /// Invariants asserted: state becomes [`PlayerStateKind::Jumping`] and
    /// `y_speed = -JUMP_INIT_SIZE_FOR_CLIMB`.
    #[test]
    fn jump_from_climbing_uses_climb_jump_initial_yspeed() {
        let mut grid = synthetic_grid(8, 8, CellKind::Air);
        set_cell(&mut grid, 1, 1, CellKind::Vine);
        let mut player = make_player(16, 16);
        player.set_state(PlayerStateKind::Climbing);
        let runtime = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        let mut input = ActiveInput::new();
        input.insert(InputCommand::Jump);

        player.update(&input, &runtime, &grid, &mut dispatcher);

        assert_eq!(player.state(), PlayerStateKind::Jumping);
        assert_eq!(player.y_speed(), -JUMP_INIT_SIZE_FOR_CLIMB);
    }

    /// Unit under test: `on_kill` followed by enough ticks dispatches
    /// `DieRestartLevel`.
    ///
    /// Preconditions: a fresh player kill is triggered with
    /// [`DeathKind::Enemy`]; the dispatcher records every delivered message.
    ///
    /// Invariants asserted: state transitions to [`PlayerStateKind::Die`] on
    /// the next update; the die-burst dispatches exactly
    /// [`NB_COLORED_BULLET`] `CreateObject` messages; after
    /// [`STATECOUNT_MAX_TO_RESTART_GAME`] further ticks a `DieRestartLevel`
    /// message is delivered.
    #[test]
    fn die_dispatches_restart_after_max_statecount() {
        let grid = synthetic_grid(8, 8, CellKind::Air);
        let mut player = make_player(16, 16);
        let runtime = RuntimeState::new();

        let buffer: Arc<Mutex<Vec<(MessageType, MessagePayload)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::DieRestartLevel,
            Box::new(Recorder(Arc::clone(&buffer))),
        );
        dispatcher.subscribe(
            MessageType::CreateObject,
            Box::new(Recorder(Arc::clone(&buffer))),
        );

        player.on_kill(1, DeathKind::Enemy);
        // First tick promotes the pending die request: state flips and burst fires.
        player.update(&ActiveInput::new(), &runtime, &grid, &mut dispatcher);

        assert_eq!(player.state(), PlayerStateKind::Die);
        let create_count = buffer
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| matches!(t, MessageType::CreateObject))
            .count();
        assert_eq!(
            create_count as i32, NB_COLORED_BULLET,
            "die burst must dispatch exactly NB_COLORED_BULLET CreateObject messages"
        );

        // Subsequent ticks accumulate state_count; after STATECOUNT_MAX one
        // DieRestartLevel must arrive.
        for _ in 0..(STATECOUNT_MAX_TO_RESTART_GAME + 1) {
            player.update(&ActiveInput::new(), &runtime, &grid, &mut dispatcher);
        }

        let restart_count = buffer
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| matches!(t, MessageType::DieRestartLevel))
            .count();
        assert!(
            restart_count >= 1,
            "DieRestartLevel must be dispatched after STATECOUNT_MAX_TO_RESTART_GAME ticks; saw {restart_count}"
        );
    }

    /// Unit under test: `can_fire` returns `true` in the `Stand` state and
    /// `false` in the `Die` state.
    ///
    /// Preconditions: fresh player (starts in `Stand`); then forced into `Die`.
    ///
    /// Invariants asserted: `can_fire()` is `true` for `Stand` and `false`
    /// for `Die`.
    #[test]
    fn can_fire_true_in_stand_false_in_die() {
        let player = make_player(16, 16);
        assert!(
            player.can_fire(),
            "player in Stand state must be able to fire"
        );
        let mut player = make_player(16, 16);
        player.set_state(PlayerStateKind::Die);
        assert!(
            !player.can_fire(),
            "player in Die state must not be able to fire"
        );
    }

    /// Unit under test: `can_fire` returns `false` in the `Climbing` state.
    ///
    /// Invariants asserted: `can_fire()` is `false` for `Climbing`.
    #[test]
    fn can_fire_false_in_climbing() {
        let mut player = make_player(16, 16);
        player.set_state(PlayerStateKind::Climbing);
        assert!(
            !player.can_fire(),
            "player in Climbing state must not be able to fire"
        );
    }

    /// Unit under test: `bounding_box` reports the configured rectangle so
    /// the level screen's collision iteration sees a non-trivial player
    /// footprint.
    #[test]
    fn bounding_box_returns_configured_rect() {
        let player = make_player(48, 32);
        let bbox = player.bounding_box();
        assert_eq!(bbox, Rect::new(48, 32, BLOCK_SIZE_I, BLOCK_SIZE_I));
    }

    /// Unit under test: the save-snapshot round-trip
    /// (`JnObject -> PlayerEntity::new -> snapshot == JnObject`).
    ///
    /// Preconditions: a JN object record with distinct values in every field,
    /// including fields the player model does not track (`counter`, `flags`)
    /// and sub-`BLOCK_SIZE_I` dimensions that `new()` normalizes internally.
    ///
    /// Invariant asserted: `snapshot()` reproduces the source record exactly -
    /// the modeled fields are written back and the unmodeled authored fields
    /// (including the un-normalized dimensions) are preserved from the cloned
    /// origin.
    #[test]
    fn snapshot_round_trips_the_source_jn_object() {
        // Sub-block dimensions: `new()` clamps these to >= BLOCK_SIZE_I for
        // collision, but the snapshot must persist the authored values.
        let mut obj = JnObject::spawned(0, 112, 160, 10, 12);
        obj.set_speed(1, -3);
        obj.set_state(PlayerStateKind::Jumping.to_state_code());
        obj.set_sub_state(4);
        obj.set_state_count(7);
        obj.set_info1(-1);
        obj.set_zap_hold(3);
        // Fields the player does not model; must survive untouched.
        obj.set_counter(9);
        obj.set_flags(0x55);

        let player = PlayerEntity::new(&obj, &AssetCache::synthetic());
        let snapshot = player.snapshot().expect("player always snapshots");

        assert_eq!(snapshot, obj);
    }

    /// Unit under test: `arm_zaphold` seeds the touch cooldown and `update`
    /// decrements it once per tick.
    #[test]
    fn zaphold_decrements_each_update() {
        let grid = synthetic_grid(8, 8, CellKind::Air);
        let mut player = make_player(16, 16);
        player.arm_zaphold();
        assert_eq!(player.zaphold(), ZAPHOLD_AFTER_TOUCH as i32);

        let runtime = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        let input = ActiveInput::new();

        player.update(&input, &runtime, &grid, &mut dispatcher);
        assert_eq!(player.zaphold(), ZAPHOLD_AFTER_TOUCH as i32 - 1);
    }

    /// Unit under test: `draw` emits a Blit command in the player tileset.
    ///
    /// Preconditions: default player in `Stand` facing forward.
    ///
    /// Invariants asserted: the returned command is a `Blit` targeting
    /// `TILESET_INDEX` at the player's world position.
    #[test]
    fn draw_emits_blit_in_player_tileset() {
        let player = make_player(24, 32);
        let cmd = player.draw().expect("stand draw must yield a command");
        match cmd {
            RenderCommand::Blit {
                tileset,
                tile,
                x,
                y,
                ..
            } => {
                assert_eq!(tileset, TILESET_INDEX);
                assert_eq!(tile, TILE_STAND_MIDDLE);
                assert_eq!(x, 24);
                assert_eq!(y, 32);
            }
            other => panic!("expected Blit, got {other:?}"),
        }
    }
}
