//! Object-layer renderer for `INTRO.JN1`-backed screens.
//!
//! Mirrors `AbstractObjectJillLevel`'s per-frame `updateObject` /
//! `drawObject` pair from the Java reference: iterate the JN object
//! layer, intersect each record's world-space bounding box with the
//! viewport-translated game area, dispatch on the JN `object_type` id,
//! and emit one [`RenderCommand`] per visible object.
//!
//! The dispatch table is limited to the static, non-interactive object
//! types `INTRO.JN1` uses for its title and start-menu screens:
//!
//! - type 0 - static-pose Jill sprite (rendered with the
//!   forward-facing stand tile from `JILL1.SHA` tileset 8).
//!   Used by the start menu's right-half Jill at world (1960, 944).
//! - type 20 - small-font `TextTileManager` text.
//!   Used by the start menu's "THIS GAME IS / SHAREWARE" notice.
//! - type 21 - big-font `TextTileManager` text.
//!   Used elsewhere in `INTRO.JN1` (title / credit screens).
//!
//! Other object types intersecting the viewport are silently skipped;
//! enemy AI, projectiles, checkpoints, doors, lifts, and other
//! interactive object types belong to the per-screen object entity
//! loop rather than to this static draw helper.

use openjill_core::layout::{GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
use openjill_core::{FontSize, RenderCommand};
use openjill_data::jn::{JnFile, JnObject};

use crate::status_bar::GAME_AREA_CLIP;

/// JN object type for the player / static-pose Jill sprite.
///
/// Matches `objects_manager_mapping.json` entry `type: 0`
/// (`PlayerManager`).  In `INTRO.JN1` the start-menu viewport carries
/// one such record with `x_speed = y_speed = 0` and `state = 0`,
/// which the Java reference draws as the forward-facing stand pose
/// from `JILL1.SHA` tileset 8 tile 16.
const OBJ_TYPE_PLAYER: u8 = 0;

/// JN object type for a small-font text record (`TextTileManager` /
/// `SMALL_TEXT = 20`).  The string-stack entry pointed at by the
/// object record carries the visible payload; `x_speed` is the
/// foreground palette index and `y_speed` is the background index
/// (treated here as transparent because the indexed `DrawText`
/// renderer does not support an opaque background fill).
const OBJ_TYPE_SMALL_TEXT: u8 = 20;

/// JN object type for a big-font text record (`TextTileManager` /
/// `BIG_TEXT = 21`).  Same conventions as [`OBJ_TYPE_SMALL_TEXT`]
/// but rendered with [`FontSize::Big`].
const OBJ_TYPE_BIG_TEXT: u8 = 21;

/// SHA tileset that carries every player sprite frame.
///
/// Matches `PlayerStandConst.TILESET_INDEX = 8` in the Java reference;
/// this constant is duplicated here rather than imported from the
/// player entity module to keep the start-menu render path independent
/// of the gameplay player implementation.
const PLAYER_TILESET: u8 = 8;

/// Forward-facing stand tile inside [`PLAYER_TILESET`].
///
/// Matches `PlayerStandConst.TILE_MIDDLE_INDEX = 16` in the Java
/// reference.  `INTRO.JN1` does not animate the start-menu Jill, so
/// this single tile is enough for the static stand pose.
const PLAYER_TILE_STAND_MIDDLE: u16 = 16;

/// Maximum logical palette index the [`RenderCommand::DrawText`]
/// path accepts before the bright-EGA shift.
///
/// `TextTileManager` reads `x_speed` (a signed 16-bit field) as the
/// foreground color; valid EGA indices are `0..=7`.  Any value
/// outside that range is clamped before being handed to the
/// renderer, matching the saturating behaviour the render crate
/// already applies after the `TEXT_COLOR_BRIGHT_SHIFT`.
const MAX_TEXT_COLOR: i16 = 7;

/// Renders the visible portion of the JN object layer for an
/// `INTRO.JN1`-backed screen.
///
/// `offset_x` and `offset_y` follow the same sign convention as
/// [`crate::screens::intro_background::render_intro_background`]: a
/// negative value shifts the camera left/up, revealing world content
/// at `(-offset_x, -offset_y)`.  Object records whose bounding box
/// intersects the game-area viewport are translated into framebuffer
/// space and dispatched to per-type emitters; off-screen and
/// unsupported records produce no commands.
pub fn render_intro_objects(jn: &JnFile, offset_x: i32, offset_y: i32) -> Vec<RenderCommand> {
    let view_x0 = -offset_x;
    let view_y0 = -offset_y;
    let view_x1 = view_x0 + GAME_AREA_W as i32;
    let view_y1 = view_y0 + GAME_AREA_H as i32;

    let mut commands = Vec::new();
    for obj in jn.objects() {
        let ox = i32::from(obj.x());
        let oy = i32::from(obj.y());
        let ow = i32::from(obj.width());
        let oh = i32::from(obj.height());

        if ox + ow <= view_x0 || ox >= view_x1 || oy + oh <= view_y0 || oy >= view_y1 {
            continue;
        }

        let fb_x = GAME_AREA_X + (ox - view_x0);
        let fb_y = GAME_AREA_Y + (oy - view_y0);

        match obj.object_type() {
            OBJ_TYPE_PLAYER => commands.push(render_player_stand(fb_x, fb_y)),
            OBJ_TYPE_SMALL_TEXT => {
                if let Some(cmd) = render_text(jn, obj, fb_x, fb_y, FontSize::Small) {
                    commands.push(cmd);
                }
            }
            OBJ_TYPE_BIG_TEXT => {
                if let Some(cmd) = render_text(jn, obj, fb_x, fb_y, FontSize::Big) {
                    commands.push(cmd);
                }
            }
            _ => {}
        }
    }
    commands
}

/// Emits the static stand-pose Jill sprite at `(x, y)`.
///
/// `INTRO.JN1`'s player record sits at world (1960, 944) with
/// `x_speed = y_speed = 0` and `state = 0`, which the Java reference
/// draws with the forward-facing stand tile.  Animation, facing, and
/// running frames are gameplay-only; this helper is intentionally
/// fixed to a single tile so the start menu does not depend on the
/// player state machine.
///
/// The returned `Blit` carries [`GAME_AREA_CLIP`] so a stand sprite
/// straddling the game-area edge cannot bleed past it into the
/// surrounding status-bar frame, matching the per-command clip
/// `LevelScreen::translate_object_command` already applies on its
/// gameplay draw path.
fn render_player_stand(x: i32, y: i32) -> RenderCommand {
    RenderCommand::Blit {
        tileset: PLAYER_TILESET,
        tile: PLAYER_TILE_STAND_MIDDLE,
        x,
        y,
        opaque: false,
        clip: Some(GAME_AREA_CLIP),
    }
}

/// Resolves the string-stack payload for a text-tile object and emits
/// a [`RenderCommand::DrawText`].
///
/// Returns `None` when the object has no associated string entry or
/// its string index points past the end of the parsed string stack;
/// both cases are treated as malformed records and skipped silently.
fn render_text(
    jn: &JnFile,
    obj: &JnObject,
    x: i32,
    y: i32,
    font: FontSize,
) -> Option<RenderCommand> {
    let string_index = obj.string_index()?;
    let entry = jn.strings().get(string_index)?;
    let text = entry.value().to_string();
    if text.is_empty() {
        return None;
    }
    let color_index = obj.x_speed().clamp(0, MAX_TEXT_COLOR) as u8;
    Some(RenderCommand::DrawText {
        text,
        x,
        y,
        color_index,
        font,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_TEXT_COLOR, OBJ_TYPE_BIG_TEXT, OBJ_TYPE_PLAYER, OBJ_TYPE_SMALL_TEXT,
        PLAYER_TILE_STAND_MIDDLE, PLAYER_TILESET, render_intro_objects,
    };
    use openjill_core::layout::{GAME_AREA_X, GAME_AREA_Y};
    use openjill_core::{FontSize, RenderCommand};
    use openjill_data::jn::JnFile;

    /// Per-object record size in the JN object layer (mirrors
    /// `parse_object`'s field widths).
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Background block count (`128 * 64 * 2` bytes).
    const BACKGROUND_BYTES: usize = 128 * 64 * 2;

    /// Fixed save-block size that follows the object layer.
    const SAVE_BLOCK_BYTES: usize = 70;

    /// Parameter bundle for [`synthetic_jn`] keeping the constructor
    /// argument count below the Clippy threshold.
    struct SyntheticObject<'a> {
        object_type: u8,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        x_speed: i16,
        y_speed: i16,
        payload_text: Option<&'a str>,
    }

    /// Builds a synthetic [`JnFile`] with one object whose fields match
    /// the supplied parameters and, when `text` is supplied, exactly
    /// one string-stack entry the object's pointer marker references.
    ///
    /// `payload_text` is appended as a length-prefixed string-stack
    /// record after the save block when `Some`, matching `parse_string`'s
    /// `[u16 length][bytes][u8 terminator]` layout.
    fn synthetic_jn(spec: SyntheticObject<'_>) -> JnFile {
        let SyntheticObject {
            object_type,
            x,
            y,
            width,
            height,
            x_speed,
            y_speed,
            payload_text,
        } = spec;
        let mut bytes = vec![0u8; BACKGROUND_BYTES + 2 + OBJECT_RECORD_BYTES + SAVE_BLOCK_BYTES];

        // Object count = 1.
        let count_off = BACKGROUND_BYTES;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());

        let record_off = count_off + 2;
        bytes[record_off] = object_type;
        bytes[record_off + 1..record_off + 3].copy_from_slice(&x.to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&y.to_le_bytes());
        bytes[record_off + 5..record_off + 7].copy_from_slice(&x_speed.to_le_bytes());
        bytes[record_off + 7..record_off + 9].copy_from_slice(&y_speed.to_le_bytes());
        bytes[record_off + 9..record_off + 11].copy_from_slice(&width.to_le_bytes());
        bytes[record_off + 11..record_off + 13].copy_from_slice(&height.to_le_bytes());

        if let Some(text) = payload_text {
            // Mark a nonzero pointer so the JN parser ties this object
            // to the upcoming string entry.  Field offsets follow
            // `parse_object`: `object_type` u8 + 11 u16 fields = 23
            // bytes precede the `pointer` u32.
            let pointer_off = record_off + 23;
            bytes[pointer_off..pointer_off + 4].copy_from_slice(&1u32.to_le_bytes());

            // Append `[u16 length][text bytes][u8 terminator]` after the
            // save block to seed the string stack with one entry.
            let text_bytes = text.as_bytes();
            let length =
                u16::try_from(text_bytes.len()).expect("test payload must fit in u16 length");
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(text_bytes);
            bytes.push(0);
        }

        JnFile::from_bytes(bytes).expect("synthetic JN must parse")
    }

    /// Unit under test: `render_intro_objects` for a single
    /// in-viewport type-0 player record.
    ///
    /// Preconditions: synthetic JN with one type-0 object at world
    /// (1960, 944), 16x32, and the start-menu viewport offset
    /// `(-1808, -864)`.
    ///
    /// Invariants asserted: exactly one [`RenderCommand::Blit`] is
    /// emitted; tileset / tile match the forward-facing stand pose;
    /// framebuffer position is `(GAME_AREA_X + 152, GAME_AREA_Y + 80)`.
    #[test]
    fn player_in_viewport_emits_stand_blit() {
        let jn = synthetic_jn(SyntheticObject {
            object_type: OBJ_TYPE_PLAYER,
            x: 1960,
            y: 944,
            width: 16,
            height: 32,
            x_speed: 0,
            y_speed: 0,
            payload_text: None,
        });
        let commands = render_intro_objects(&jn, -1808, -864);
        assert_eq!(commands.len(), 1, "exactly one Blit expected");
        match &commands[0] {
            RenderCommand::Blit {
                tileset,
                tile,
                x,
                y,
                ..
            } => {
                assert_eq!(*tileset, PLAYER_TILESET);
                assert_eq!(*tile, PLAYER_TILE_STAND_MIDDLE);
                assert_eq!(*x, GAME_AREA_X + 152);
                assert_eq!(*y, GAME_AREA_Y + 80);
            }
            other => panic!("expected Blit, got {other:?}"),
        }
    }

    /// Unit under test: `render_intro_objects` for an in-viewport
    /// type-20 (small) text record.
    ///
    /// Preconditions: synthetic JN with one type-20 object whose
    /// `pointer` marker selects a single string-stack entry
    /// `"SHAREWARE"` at world (1952, 992) with `x_speed = 3`.
    ///
    /// Invariants asserted: exactly one [`RenderCommand::DrawText`] is
    /// emitted with `FontSize::Small`, `color_index = 3`, payload
    /// `"SHAREWARE"`, and framebuffer position
    /// `(GAME_AREA_X + 144, GAME_AREA_Y + 128)`.
    #[test]
    fn small_text_object_emits_draw_text() {
        let jn = synthetic_jn(SyntheticObject {
            object_type: OBJ_TYPE_SMALL_TEXT,
            x: 1952,
            y: 992,
            width: 72,
            height: 7,
            x_speed: 3,
            y_speed: -1,
            payload_text: Some("SHAREWARE"),
        });
        let commands = render_intro_objects(&jn, -1808, -864);
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            RenderCommand::DrawText {
                text,
                x,
                y,
                color_index,
                font,
            } => {
                assert_eq!(text, "SHAREWARE");
                assert_eq!(*color_index, 3);
                assert_eq!(*font, FontSize::Small);
                assert_eq!(*x, GAME_AREA_X + 144);
                assert_eq!(*y, GAME_AREA_Y + 128);
            }
            other => panic!("expected DrawText, got {other:?}"),
        }
    }

    /// Unit under test: `render_intro_objects` for an in-viewport
    /// type-21 (big) text record.
    ///
    /// Preconditions: synthetic JN with one type-21 object resolving
    /// the string `"TITLE"`.
    ///
    /// Invariants asserted: emitted command is `DrawText` with
    /// `FontSize::Big`.
    #[test]
    fn big_text_object_emits_big_font() {
        let jn = synthetic_jn(SyntheticObject {
            object_type: OBJ_TYPE_BIG_TEXT,
            x: 1808,
            y: 864,
            width: 64,
            height: 8,
            x_speed: 5,
            y_speed: -1,
            payload_text: Some("TITLE"),
        });
        let commands = render_intro_objects(&jn, -1808, -864);
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            RenderCommand::DrawText { font, .. } => assert_eq!(*font, FontSize::Big),
            other => panic!("expected DrawText, got {other:?}"),
        }
    }

    /// Unit under test: out-of-viewport objects produce no commands.
    ///
    /// Preconditions: synthetic JN with one type-0 record placed far
    /// outside the start-menu viewport.
    ///
    /// Invariants asserted: returned command list is empty.
    #[test]
    fn object_outside_viewport_is_skipped() {
        let jn = synthetic_jn(SyntheticObject {
            object_type: OBJ_TYPE_PLAYER,
            x: 100,
            y: 100,
            width: 16,
            height: 32,
            x_speed: 0,
            y_speed: 0,
            payload_text: None,
        });
        let commands = render_intro_objects(&jn, -1808, -864);
        assert!(commands.is_empty());
    }

    /// Unit under test: unsupported object types are silently dropped.
    ///
    /// Preconditions: synthetic JN with one in-viewport type-12
    /// (checkpoint) object.
    ///
    /// Invariants asserted: returned command list is empty; no panic
    /// or warning surfaces from the dispatch.
    #[test]
    fn unsupported_type_is_skipped() {
        let jn = synthetic_jn(SyntheticObject {
            object_type: 12,
            x: 1960,
            y: 944,
            width: 16,
            height: 16,
            x_speed: 0,
            y_speed: 0,
            payload_text: None,
        });
        let commands = render_intro_objects(&jn, -1808, -864);
        assert!(commands.is_empty());
    }

    /// Unit under test: a text object whose string index points past
    /// the end of the parsed string stack is skipped instead of
    /// panicking.
    ///
    /// Preconditions: synthetic JN with a type-20 object that has a
    /// nonzero `pointer` marker but no string-stack entry.
    ///
    /// Invariants asserted: returned command list is empty.
    #[test]
    fn text_object_without_string_payload_is_skipped() {
        let jn = synthetic_jn(SyntheticObject {
            object_type: OBJ_TYPE_SMALL_TEXT,
            x: 1952,
            y: 992,
            width: 72,
            height: 7,
            x_speed: 3,
            y_speed: -1,
            payload_text: None,
        });
        let commands = render_intro_objects(&jn, -1808, -864);
        assert!(commands.is_empty());
    }

    /// Unit under test: `MAX_TEXT_COLOR` clamps EGA indices to the
    /// `0..=7` range the renderer's bright-shift expects.
    ///
    /// Preconditions: synthetic JN whose text object has
    /// `x_speed = 99`.
    ///
    /// Invariants asserted: emitted `DrawText` carries
    /// `color_index = MAX_TEXT_COLOR as u8`.
    #[test]
    fn text_color_is_clamped_to_seven() {
        let jn = synthetic_jn(SyntheticObject {
            object_type: OBJ_TYPE_SMALL_TEXT,
            x: 1952,
            y: 992,
            width: 72,
            height: 7,
            x_speed: 99,
            y_speed: -1,
            payload_text: Some("HELLO"),
        });
        let commands = render_intro_objects(&jn, -1808, -864);
        match &commands[0] {
            RenderCommand::DrawText { color_index, .. } => {
                assert_eq!(*color_index, MAX_TEXT_COLOR as u8);
            }
            other => panic!("expected DrawText, got {other:?}"),
        }
    }
}
