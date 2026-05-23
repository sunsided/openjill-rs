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
    ActiveInput, BackgroundGrid, DeathKind, InputCommand, MessageDispatcher, MessagePayload,
    MessageType, ObjectEntity, Rect, RenderCommand, RuntimeState,
};
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
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            state: PlayerStateKind::Stand,
            sub_state: 0,
            state_count: 0,
            x_speed: 0,
            y_speed: 0,
            info1: 0,
            zaphold: 0,
            death_kind: None,
            die_pending: false,
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
        _state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        dispatcher: &mut MessageDispatcher,
    ) {
        if self.zaphold > 0 {
            self.zaphold -= 1;
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
/// solid (`!is_passthrough`).
///
/// Mirrors `UtilityObjectEntity.checkIfFloorUnderObject` from the Java
/// reference: probe one pixel below the bottom edge and check every cell
/// covered by the bounding box footprint at that row.  `is_stair` cells also
/// count as floor; we coalesce this by treating stairs as `!is_passthrough`
/// implicitly through the `BackgroundEntity` flag layout.
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
            && (!cell.is_passthrough() || cell.is_stair())
        {
            return true;
        }
    }
    false
}

/// Returns `true` when the cell at the player's centre is climbable.
///
/// Mirrors `UtilityObjectEntity.isClimbing`: the reference samples the cell
/// directly under the centre of the object's bounding box and consults the
/// `isVine` flag (which we expose as `is_climbable`).
fn is_on_climbable(grid: &BackgroundGrid, x: i32, y: i32, w: i32, h: i32) -> bool {
    let cx = (x + w / 2).div_euclid(BLOCK_SIZE_I);
    let cy = (y + h / 2).div_euclid(BLOCK_SIZE_I);
    if cx < 0 || cy < 0 {
        return false;
    }
    grid.get(cx as usize, cy as usize)
        .map(openjill_core::BackgroundEntity::is_climbable)
        .unwrap_or(false)
}

/// Returns `true` when the rectangle `[x, x+w) x [y, y+h)` overlaps any cell
/// that is not passable.
///
/// `is_stair` cells count as solid for the bounding-box collision test so the
/// player cannot tunnel through a step diagonally, mirroring the Java
/// reference's `UtilityObjectEntity.moveObject*` helpers which treat stair
/// cells as blocking surfaces (`has_floor_below` applies the same rule).
fn collides_solid(grid: &BackgroundGrid, x: i32, y: i32, w: i32, h: i32) -> bool {
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
                && (!cell.is_passthrough() || cell.is_stair())
            {
                return true;
            }
        }
    }
    false
}

/// Attempts a single all-or-nothing vertical step.
///
/// Returns `true` when the player moved, `false` when the target rectangle
/// would overlap a solid cell.
fn try_move_vertical(grid: &BackgroundGrid, x: i32, y: &mut i32, w: i32, h: i32, dy: i32) -> bool {
    let new_y = *y + dy;
    if collides_solid(grid, x, new_y, w, h) {
        return false;
    }
    *y = new_y;
    true
}

/// Attempts a single all-or-nothing horizontal step.
///
/// Same semantics as [`try_move_vertical`] on the X axis.
fn try_move_vertical_horizontal(
    grid: &BackgroundGrid,
    x: &mut i32,
    y: i32,
    w: i32,
    h: i32,
    dx: i32,
) -> bool {
    let new_x = *x + dx;
    if collides_solid(grid, new_x, y, w, h) {
        return false;
    }
    *x = new_x;
    true
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

    /// Unit under test: `bounding_box` reports the configured rectangle so
    /// the level screen's collision iteration sees a non-trivial player
    /// footprint.
    #[test]
    fn bounding_box_returns_configured_rect() {
        let player = make_player(48, 32);
        let bbox = player.bounding_box();
        assert_eq!(bbox, Rect::new(48, 32, BLOCK_SIZE_I, BLOCK_SIZE_I));
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
