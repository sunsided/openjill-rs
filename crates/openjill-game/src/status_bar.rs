//! Static VGA status bar render commands and game-area offset helpers.

use openjill_core::layout::{GAME_AREA_X, GAME_AREA_Y};
use openjill_core::{FontSize, RenderCommand};
use std::sync::LazyLock;

/// Embedded JSON describing the static VGA status bar tile layout.
pub(crate) const STATUS_BAR_JSON: &str =
    include_str!("../../../OpenJill/src/main/resources/status_bar_vga.json");

/// Cached static VGA status bar commands parsed from [`STATUS_BAR_JSON`].
static STATUS_BAR_COMMANDS: LazyLock<Vec<RenderCommand>> = LazyLock::new(parse_status_bar_commands);

/// Returns the static VGA status bar as a sequence of render commands.
///
/// Clones the cached commands parsed from the embedded `status_bar_vga.json`. The order
/// matches the Java reference `StatusBar` initialization: all `images` blits first, then
/// `text` labels, then `bigtext` labels, so text overlays any underlying frame tiles.
pub fn status_bar_commands() -> Vec<RenderCommand> {
    STATUS_BAR_COMMANDS.clone()
}

/// Parses the embedded `status_bar_vga.json` into static status bar render commands.
///
/// Returns an empty `Vec` only when the top-level JSON itself fails to parse. Each of the
/// three optional top-level arrays (`images`, `text`, `bigtext`) is consumed independently:
/// a missing or non-array value contributes nothing rather than discarding the other
/// arrays, and individual entries that omit required fields or carry out-of-range numeric
/// values are silently skipped. Emits commands in the order: `images` blits, then `text`
/// `DrawText`, then `bigtext` `DrawText`.
fn parse_status_bar_commands() -> Vec<RenderCommand> {
    let value: serde_json::Value = match serde_json::from_str(STATUS_BAR_JSON) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut commands = Vec::new();
    if let Some(images) = value.get("images").and_then(|v| v.as_array()) {
        commands.extend(images.iter().filter_map(parse_blit_entry));
    }
    if let Some(text) = value.get("text").and_then(|v| v.as_array()) {
        commands.extend(
            text.iter()
                .filter_map(|entry| parse_text_entry(entry, FontSize::Small)),
        );
    }
    if let Some(bigtext) = value.get("bigtext").and_then(|v| v.as_array()) {
        commands.extend(
            bigtext
                .iter()
                .filter_map(|entry| parse_text_entry(entry, FontSize::Big)),
        );
    }
    commands
}

/// Parses one `images` array entry into a [`RenderCommand::Blit`]. Returns `None` if any
/// required field is missing, has the wrong JSON type, or carries a numeric value that
/// does not fit the target width (treated as malformed rather than silently truncated).
fn parse_blit_entry(img: &serde_json::Value) -> Option<RenderCommand> {
    let tileset = u8::try_from(img.get("tileset")?.as_u64()?).ok()?;
    let tile = u16::try_from(img.get("tile")?.as_u64()?).ok()?;
    let x = i32::try_from(img.get("x")?.as_i64()?).ok()?;
    let y = i32::try_from(img.get("y")?.as_i64()?).ok()?;
    Some(RenderCommand::Blit {
        tileset,
        tile,
        x,
        y,
        opaque: false,
        clip: None,
    })
}

/// Parses one `text` or `bigtext` array entry into a [`RenderCommand::DrawText`]. Returns
/// `None` if any required field is missing, has the wrong JSON type, or carries a numeric
/// value that does not fit the target width.
///
/// `font` selects which SHA font tileset the rendered glyphs are drawn from:
/// callers pass [`FontSize::Small`] for `status_bar_vga.json`'s `text` array
/// (body labels rendered with the 6x6 font) and [`FontSize::Big`] for its
/// `bigtext` array (the 8x8 title font).
fn parse_text_entry(entry: &serde_json::Value, font: FontSize) -> Option<RenderCommand> {
    let text = entry.get("text")?.as_str()?.to_string();
    let color_index = u8::try_from(entry.get("color")?.as_u64()?).ok()?;
    let x = i32::try_from(entry.get("x")?.as_i64()?).ok()?;
    let y = i32::try_from(entry.get("y")?.as_i64()?).ok()?;
    Some(RenderCommand::DrawText {
        text,
        x,
        y,
        color_index,
        font,
    })
}

/// Returns a [`RenderCommand::Blit`] with coordinates offset into the game area.
///
/// `game_x` and `game_y` are in game-area space (origin at `(0, 0)` relative to the game area).
/// The returned command adds [`GAME_AREA_X`] and [`GAME_AREA_Y`] so the caller does not need to
/// know the absolute framebuffer layout.
pub fn game_area_blit(
    tileset: u8,
    tile: u16,
    game_x: i32,
    game_y: i32,
    opaque: bool,
) -> RenderCommand {
    RenderCommand::Blit {
        tileset,
        tile,
        x: GAME_AREA_X + game_x,
        y: GAME_AREA_Y + game_y,
        opaque,
        clip: Some(GAME_AREA_CLIP),
    }
}

/// Framebuffer clip rectangle that confines blit pixels to the game area.
///
/// Game-area background tiles emitted via [`game_area_blit`] carry this clip
/// rect so partial-overlap edge tiles do not bleed past the game-area border
/// into the surrounding status-bar frame.  The values mirror `GAME_AREA_*`
/// from `openjill_core::layout`.
pub const GAME_AREA_CLIP: openjill_core::ClipRect = openjill_core::ClipRect {
    x: GAME_AREA_X,
    y: GAME_AREA_Y,
    width: openjill_core::layout::GAME_AREA_W,
    height: openjill_core::layout::GAME_AREA_H,
};

#[cfg(test)]
mod tests {
    use super::{STATUS_BAR_JSON, game_area_blit, status_bar_commands};
    use openjill_core::RenderCommand;
    use openjill_core::layout::{GAME_AREA_X, GAME_AREA_Y};

    /// Unit under test: `status_bar_commands` Blit count against the JSON `images` array.
    ///
    /// Preconditions: `STATUS_BAR_JSON` is the embedded `status_bar_vga.json` with a valid
    /// top-level `images` array.
    ///
    /// Invariants asserted: the number of `Blit` commands returned equals the number of entries
    /// in the JSON `images` array.
    #[test]
    fn status_bar_commands_blit_count_matches_json_images() {
        let commands = status_bar_commands();
        let json: serde_json::Value =
            serde_json::from_str(STATUS_BAR_JSON).expect("STATUS_BAR_JSON must be valid JSON");
        let expected = json["images"]
            .as_array()
            .expect("images must be an array")
            .len();
        let blit_count = commands
            .iter()
            .filter(|cmd| matches!(cmd, RenderCommand::Blit { .. }))
            .count();
        assert_eq!(
            blit_count, expected,
            "Blit count must match images array length"
        );
    }

    /// Unit under test: `status_bar_commands` `DrawText` count against `text` + `bigtext`.
    ///
    /// Preconditions: `STATUS_BAR_JSON` is the embedded `status_bar_vga.json` with `text` and
    /// `bigtext` arrays at the top level.
    ///
    /// Invariants asserted: every entry in both arrays produces one `DrawText` command.
    #[test]
    fn status_bar_commands_drawtext_count_matches_text_arrays() {
        let commands = status_bar_commands();
        let json: serde_json::Value =
            serde_json::from_str(STATUS_BAR_JSON).expect("STATUS_BAR_JSON must be valid JSON");
        let text_len = json["text"]
            .as_array()
            .expect("text must be an array")
            .len();
        let bigtext_len = json["bigtext"]
            .as_array()
            .expect("bigtext must be an array")
            .len();
        let drawtext_count = commands
            .iter()
            .filter(|cmd| matches!(cmd, RenderCommand::DrawText { .. }))
            .count();
        assert_eq!(
            drawtext_count,
            text_len + bigtext_len,
            "DrawText count must equal text + bigtext lengths"
        );
    }

    /// Unit under test: `status_bar_commands` ordering and contents — blits first, then `text`
    /// entries, then `bigtext` entries, each preserving JSON array order with verbatim
    /// `(text, x, y, color)` values.
    ///
    /// Preconditions: `STATUS_BAR_JSON` has at least one entry in `images`, `text`, and
    /// `bigtext`.
    ///
    /// Invariants asserted: every `Blit` command appears before every `DrawText` command; the
    /// emitted `DrawText` sequence is exactly the concatenation `text ++ bigtext` from the
    /// JSON, both in length and in per-entry `(text, x, y, color)`. Deriving the expectation
    /// from the JSON keeps the test stable if `status_bar_vga.json` gains additional labels.
    #[test]
    fn status_bar_commands_emit_blits_then_text_then_bigtext() {
        let commands = status_bar_commands();
        let last_blit = commands
            .iter()
            .rposition(|cmd| matches!(cmd, RenderCommand::Blit { .. }))
            .expect("at least one Blit");
        let first_text = commands
            .iter()
            .position(|cmd| matches!(cmd, RenderCommand::DrawText { .. }))
            .expect("at least one DrawText");
        assert!(
            last_blit < first_text,
            "all Blit commands must precede DrawText commands"
        );

        let json: serde_json::Value =
            serde_json::from_str(STATUS_BAR_JSON).expect("STATUS_BAR_JSON must be valid JSON");
        let expected: Vec<(String, i32, i32, u8)> = json["text"]
            .as_array()
            .expect("text must be an array")
            .iter()
            .chain(
                json["bigtext"]
                    .as_array()
                    .expect("bigtext must be an array")
                    .iter(),
            )
            .map(|entry| {
                (
                    entry["text"]
                        .as_str()
                        .expect("text field must be a string")
                        .to_string(),
                    i32::try_from(entry["x"].as_i64().expect("x must be integer"))
                        .expect("x must fit i32"),
                    i32::try_from(entry["y"].as_i64().expect("y must be integer"))
                        .expect("y must fit i32"),
                    u8::try_from(entry["color"].as_u64().expect("color must be integer"))
                        .expect("color must fit u8"),
                )
            })
            .collect();

        let actual: Vec<(String, i32, i32, u8)> = commands
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::DrawText {
                    text,
                    x,
                    y,
                    color_index,
                    ..
                } => Some((text.clone(), *x, *y, *color_index)),
                _ => None,
            })
            .collect();

        assert_eq!(
            actual, expected,
            "DrawText sequence must equal JSON `text` followed by `bigtext`, in order"
        );
    }

    /// Unit under test: `game_area_blit` coordinate offset.
    ///
    /// Preconditions: the game area origin is `(GAME_AREA_X, GAME_AREA_Y)`.
    ///
    /// Invariants asserted: the returned `Blit` has framebuffer coordinates equal to
    /// `(GAME_AREA_X + game_x, GAME_AREA_Y + game_y)`, with tileset, tile, and opaque
    /// values preserved verbatim.
    #[test]
    fn game_area_blit_offsets_by_game_area_origin() {
        let cmd = game_area_blit(2, 5, 10, 20, true);
        assert!(
            matches!(
                cmd,
                RenderCommand::Blit {
                    tileset: 2,
                    tile: 5,
                    x,
                    y,
                    opaque: true,
                    clip: Some(_),
                } if x == GAME_AREA_X + 10 && y == GAME_AREA_Y + 20
            ),
            "game_area_blit must offset x/y by game area origin"
        );
    }
}
