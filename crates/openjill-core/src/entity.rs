//! Entity traits, geometry, and grid types shared between core and gameplay.
//!
//! Mirrors `ObjectEntity` and `BackgroundEntity` from `openjill-core-api` plus
//! the death classification enum and the `BackgroundGrid` container that
//! level screens own for collision lookups and per-tick iteration.

use crate::ActiveInput;
use crate::message::MessageDispatcher;
use crate::render::RenderCommand;
use crate::runtime::RuntimeState;

/// Background grid width in cells.
///
/// Matches `BackgroundLayer.MAP_WIDTH = 128` in the Java reference
/// (`jn-file-api`) and the `BACKGROUND_WIDTH` constant in the JN parser.
pub const BACKGROUND_GRID_WIDTH: usize = 128;

/// Background grid height in cells.
///
/// Matches `BackgroundLayer.MAP_HEIGHT = 64` in the Java reference and the
/// `BACKGROUND_HEIGHT` constant in the JN parser.
pub const BACKGROUND_GRID_HEIGHT: usize = 64;

/// Axis-aligned rectangle in pixel coordinates.
///
/// Used for object collision tests and viewport culling.  The rectangle covers
/// pixels in the half-open range `[x, x + w) x [y, y + h)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left edge of the rectangle in pixels.
    pub x: i32,
    /// Top edge of the rectangle in pixels.
    pub y: i32,
    /// Width of the rectangle in pixels.
    pub w: i32,
    /// Height of the rectangle in pixels.
    pub h: i32,
}

impl Rect {
    /// Builds a rectangle from its top-left corner and dimensions.
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// Returns `true` when this rectangle overlaps `other`.
    ///
    /// Touching-only-on-the-edge rectangles do not count as overlapping
    /// because the half-open `[x, x + w)` interval excludes the right and
    /// bottom edges.
    pub fn intersects(&self, other: &Rect) -> bool {
        if self.w <= 0 || self.h <= 0 || other.w <= 0 || other.h <= 0 {
            return false;
        }
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    /// Returns `true` when `(px, py)` lies inside this rectangle.
    pub fn contains_point(&self, px: i32, py: i32) -> bool {
        if self.w <= 0 || self.h <= 0 {
            return false;
        }
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Classification of player death causes.
///
/// Selects the die sub-state used by `PlayerEntity` and is passed into
/// `ObjectEntity::on_kill` when a hazard or enemy resolves to a player
/// death.  Mirrors the integer death codes used by the Java reference
/// `PlayerDieXConst` classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathKind {
    /// Killed by an enemy or projectile.
    Enemy,
    /// Killed by drowning in a water background cell.
    Water,
    /// Killed by another hazardous background cell (lava, spikes, etc.).
    OtherBackground,
}

/// One active game object.
///
/// Implemented per object type in `openjill-game`.  Trait object compatible
/// so `LevelScreen` can hold `Vec<Box<dyn ObjectEntity>>` and dispatch
/// polymorphically.  The `Send` bound mirrors the Java reference's
/// expectation that object managers can be moved between threads even though
/// the current Rust port runs single-threaded.
pub trait ObjectEntity: Send {
    /// Advances state by one tick.
    ///
    /// May push messages to `dispatcher` to signal pickups, removal,
    /// or level transitions.  `backgrounds` lets the implementation probe
    /// the surrounding cells for floor/wall collision.
    fn update(
        &mut self,
        input: &ActiveInput,
        state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        dispatcher: &mut MessageDispatcher,
    );

    /// Returns the render command for this object this tick, if visible.
    fn draw(&self) -> Option<RenderCommand>;

    /// Called when the player's bounding box overlaps this object.
    ///
    /// `state` is the shared [`RuntimeState`] so handlers can query the
    /// inventory (for example, the locked-door entity tests whether the
    /// player carries the matching key before opening).
    fn on_touch(&mut self, state: &RuntimeState, dispatcher: &mut MessageDispatcher);

    /// Called when a weapon or hazard hits this object.
    fn on_kill(&mut self, damage: i32, death_kind: DeathKind);

    /// Returns the object's bounding box for collision and culling.
    fn bounding_box(&self) -> Rect;

    /// Returns `true` when this object should tick and draw even outside the
    /// viewport update border.
    fn always_active(&self) -> bool {
        false
    }

    /// Returns `true` when this object is the checkpoint marker driving
    /// level transitions.
    fn is_checkpoint(&self) -> bool {
        false
    }

    /// Returns `true` when this is the player object.
    fn is_player(&self) -> bool {
        false
    }

    /// Returns `true` when this object has signalled that it should be removed
    /// from the active object list.
    ///
    /// Pickup entities flip this from `false` to `true` inside their
    /// [`ObjectEntity::on_touch`] handler so the level loop can purge them at
    /// the end of the tick.  Mirrors the `ObjectListMessage(this, false)`
    /// pattern in the Java reference's `AbstractKeyManager` and friends.
    fn should_remove(&self) -> bool {
        false
    }
}

/// One background cell handler.
///
/// Implemented per DMA name in `openjill-game`.  Coordinates passed to
/// [`BackgroundEntity::draw`] are framebuffer pixel coordinates already
/// offset by the viewport and the game-area origin so individual
/// implementations only need to emit a `RenderCommand::Blit` at the
/// supplied position.
pub trait BackgroundEntity: Send {
    /// Returns the render command for this cell at the supplied framebuffer
    /// pixel coordinates, or `None` when the cell is transparent.
    fn draw(&self, screen_x: i32, screen_y: i32) -> Option<RenderCommand>;

    /// Advances per-tick state for cells with dynamic behavior.
    ///
    /// Cell indices `cell_x` and `cell_y` identify this cell in the
    /// surrounding [`BackgroundGrid`] so implementations can write back into
    /// neighbouring cells via messages if needed.
    fn update(&mut self, cell_x: i32, cell_y: i32, dispatcher: &mut MessageDispatcher);

    /// Called when the player's bounding box overlaps this cell.
    fn on_player_touch(
        &mut self,
        player: &mut dyn ObjectEntity,
        dispatcher: &mut MessageDispatcher,
    );

    /// Returns `true` when the player passes through this cell vertically.
    fn is_passthrough(&self) -> bool;

    /// Returns `true` when the player can climb this cell (vine/tree).
    fn is_climbable(&self) -> bool;

    /// Returns `true` when this cell behaves as a stair or slope.
    fn is_stair(&self) -> bool;

    /// Returns `true` when this cell wants per-tick `update` calls.
    fn needs_update(&self) -> bool {
        false
    }
}

/// Full background layer for the current level.
///
/// Indexed as `cells[y][x]` where `x` ranges over `[0, width)` and `y`
/// ranges over `[0, height)`.  Constructed once at level load time from the
/// JN background layer.
pub struct BackgroundGrid {
    /// Row-major background cells; `cells[y][x]` is the cell at `(x, y)`.
    pub cells: Vec<Vec<Box<dyn BackgroundEntity>>>,
    /// Width of the grid in cells.
    pub width: usize,
    /// Height of the grid in cells.
    pub height: usize,
}

impl BackgroundGrid {
    /// Builds a grid from a row-major vector of cells.
    ///
    /// `width` is taken from the first row; `height` is the row count.
    /// All rows must have identical lengths; this is debug-asserted.
    pub fn new(cells: Vec<Vec<Box<dyn BackgroundEntity>>>) -> Self {
        let height = cells.len();
        let width = cells.first().map(|row| row.len()).unwrap_or(0);
        debug_assert!(
            cells.iter().all(|row| row.len() == width),
            "all background rows must share the same width"
        );
        Self {
            cells,
            width,
            height,
        }
    }

    /// Returns a reference to the cell at `(x, y)`, or `None` when out of bounds.
    pub fn get(&self, x: usize, y: usize) -> Option<&dyn BackgroundEntity> {
        self.cells
            .get(y)
            .and_then(|row| row.get(x))
            .map(std::convert::AsRef::as_ref)
    }

    /// Returns a mutable reference to the cell at `(x, y)`, or `None`.
    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut Box<dyn BackgroundEntity>> {
        self.cells.get_mut(y).and_then(|row| row.get_mut(x))
    }
}

#[cfg(test)]
mod tests {
    use super::Rect;

    /// Unit under test: `Rect::intersects`.
    ///
    /// Preconditions: two rectangles are constructed with known overlap,
    /// non-overlap, and edge-touching configurations.
    ///
    /// Invariants asserted: overlapping rects report `true`, non-overlapping
    /// rects report `false`, and rects touching only on the right or bottom
    /// edge do not count as overlapping (half-open interval semantics).
    #[test]
    fn rect_intersects_overlapping_returns_true() {
        let a = Rect::new(0, 0, 16, 16);
        let b = Rect::new(8, 8, 16, 16);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
    }

    /// Unit under test: `Rect::intersects` with disjoint rects.
    #[test]
    fn rect_intersects_non_overlapping_returns_false() {
        let a = Rect::new(0, 0, 16, 16);
        let b = Rect::new(32, 32, 16, 16);
        assert!(!a.intersects(&b));
        assert!(!b.intersects(&a));
    }

    /// Unit under test: `Rect::intersects` with rects that touch only on an
    /// edge.
    ///
    /// Preconditions: two 16x16 rects share an edge at x=16.
    ///
    /// Invariants asserted: edge-only contact reports `false` because the
    /// rectangle covers pixels in the half-open range `[x, x + w)`.
    #[test]
    fn rect_intersects_edge_touching_returns_false() {
        let a = Rect::new(0, 0, 16, 16);
        let b = Rect::new(16, 0, 16, 16);
        assert!(!a.intersects(&b));
    }

    /// Unit under test: `Rect::contains_point`.
    #[test]
    fn rect_contains_point_respects_half_open_bounds() {
        let r = Rect::new(0, 0, 16, 16);
        assert!(r.contains_point(0, 0));
        assert!(r.contains_point(15, 15));
        assert!(!r.contains_point(16, 0));
        assert!(!r.contains_point(0, 16));
        assert!(!r.contains_point(-1, 0));
    }

    /// Unit under test: `Rect::intersects` with empty rectangles.
    ///
    /// Preconditions: one rectangle has zero width.
    ///
    /// Invariants asserted: an empty rectangle never intersects anything.
    #[test]
    fn rect_intersects_empty_returns_false() {
        let empty = Rect::new(0, 0, 0, 16);
        let r = Rect::new(0, 0, 16, 16);
        assert!(!empty.intersects(&r));
        assert!(!r.intersects(&empty));
    }
}
