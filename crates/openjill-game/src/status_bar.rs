//! Static VGA status bar render commands and game-area offset helpers.

use openjill_core::layout::{
    CONTROL_AREA_W, CONTROL_AREA_X, CONTROL_AREA_Y, GAME_AREA_X, GAME_AREA_Y, INVENTORY_AREA_X,
    INVENTORY_AREA_Y,
};
use openjill_core::{FontSize, RenderCommand};
use std::sync::LazyLock;

/// Embedded JSON describing the static VGA status bar tile layout.
pub(crate) const STATUS_BAR_JSON: &str =
    include_str!("../../../OpenJill/src/main/resources/status_bar_vga.json");

/// Embedded JSON describing the control area text and key-binding labels.
pub(crate) const CONTROL_AREA_JSON: &str =
    include_str!("../../../OpenJill/src/main/resources/control_area.json");

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

/// Parses the embedded JSON resources into static status bar render commands.
///
/// Sources three JSON files:
/// - `status_bar_vga.json`: frame/border tiles (`images`), the Jill face portrait
///   (`imagesInvenroy`), and the "CONTROLS" / "INVENTORY" labels.
/// - `inventory_conf.json`: "health", "level", "map", "score" labels drawn in the
///   inventory area.
/// - `control_area.json`: the control-panel text content (movement hint, SHIFT/ALT/F1
///   labels, N/Q/S/R/T key bindings, and the horizontal separator line).
///
/// Returns an empty `Vec` only when `status_bar_vga.json` itself fails to parse.
/// Missing or structurally invalid entries in any of the three files are silently
/// skipped without discarding the other entries.
///
/// Emit order: all `Blit` commands first (frame tiles, then Jill face), then the
/// control-area separator `FillRect`, then all `DrawText` commands so text always
/// paints over tiles.
fn parse_status_bar_commands() -> Vec<RenderCommand> {
    let value: serde_json::Value = match serde_json::from_str(STATUS_BAR_JSON) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut commands = Vec::new();

    // Frame / border tiles (absolute framebuffer coordinates).
    if let Some(images) = value.get("images").and_then(|v| v.as_array()) {
        commands.extend(images.iter().filter_map(parse_blit_entry));
    }

    // Jill face portrait: 4×4 tile mosaic placed in the inventory area.
    // Coordinates in `imagesInvenroy` are relative to the inventory area origin.
    if let Some(images_inv) = value.get("imagesInvenroy").and_then(|v| v.as_array()) {
        commands.extend(
            images_inv
                .iter()
                .filter_map(|img| parse_blit_entry_offset(img, INVENTORY_AREA_X, INVENTORY_AREA_Y)),
        );
    }

    // Control-area horizontal separator line (separates controls from key bindings).
    // Rendered before the text commands so the line sits behind any overlapping glyphs.
    if let Ok(ctrl) = serde_json::from_str::<serde_json::Value>(CONTROL_AREA_JSON)
        && let Some(lines) = ctrl.get("lines").and_then(|v| v.as_array())
    {
        for line in lines {
            let Some(y_rel) = line
                .get("y")
                .and_then(|v| v.as_i64())
                .and_then(|v| i32::try_from(v).ok())
            else {
                continue;
            };
            let color = line
                .get("color")
                .and_then(|v| v.as_u64())
                .and_then(|v| u8::try_from(v).ok())
                .unwrap_or(0);
            commands.push(RenderCommand::FillRect {
                x: CONTROL_AREA_X,
                y: CONTROL_AREA_Y + y_rel,
                width: CONTROL_AREA_W,
                height: 1,
                color,
            });
        }
    }

    // status_bar_vga.json text labels ("CONTROLS", "INVENTORY") - absolute coords.
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

    // control_area.json labels - coordinates are relative to the control area origin.
    if let Ok(ctrl) = serde_json::from_str::<serde_json::Value>(CONTROL_AREA_JSON) {
        // SHIFT / ALT / F1 key labels.
        if let Some(special) = ctrl.get("specialKey").and_then(|v| v.as_array()) {
            commands.extend(special.iter().filter_map(|entry| {
                parse_text_entry_offset(entry, FontSize::Small, CONTROL_AREA_X, CONTROL_AREA_Y)
            }));
        }
        // Movement hint and key-binding description text (small font).
        if let Some(text) = ctrl.get("text").and_then(|v| v.as_array()) {
            commands.extend(text.iter().filter_map(|entry| {
                parse_text_entry_offset(entry, FontSize::Small, CONTROL_AREA_X, CONTROL_AREA_Y)
            }));
        }
        // N / Q / S / R / T single-character bindings (big font).
        if let Some(big) = ctrl.get("bigText").and_then(|v| v.as_array()) {
            commands.extend(big.iter().filter_map(|entry| {
                parse_text_entry_offset(entry, FontSize::Big, CONTROL_AREA_X, CONTROL_AREA_Y)
            }));
        }
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

/// Parses one blit entry with a framebuffer coordinate offset applied to `x` and `y`.
///
/// Used for arrays whose coordinates are relative to a sub-area origin (e.g.
/// `imagesInvenroy` entries are relative to the inventory area top-left).
fn parse_blit_entry_offset(img: &serde_json::Value, dx: i32, dy: i32) -> Option<RenderCommand> {
    let tileset = u8::try_from(img.get("tileset")?.as_u64()?).ok()?;
    let tile = u16::try_from(img.get("tile")?.as_u64()?).ok()?;
    let x = i32::try_from(img.get("x")?.as_i64()?).ok()? + dx;
    let y = i32::try_from(img.get("y")?.as_i64()?).ok()? + dy;
    Some(RenderCommand::Blit {
        tileset,
        tile,
        x,
        y,
        opaque: false,
        clip: None,
    })
}

/// Parses one text entry with a framebuffer coordinate offset applied to `x` and `y`.
///
/// Used for arrays whose coordinates are relative to a sub-area origin (e.g.
/// `inventory_conf.json` text labels are relative to the inventory area top-left,
/// and `control_area.json` labels are relative to the control area top-left).
fn parse_text_entry_offset(
    entry: &serde_json::Value,
    font: FontSize,
    dx: i32,
    dy: i32,
) -> Option<RenderCommand> {
    let text = entry.get("text")?.as_str()?.to_string();
    let color_index = u8::try_from(entry.get("color")?.as_u64()?).ok()?;
    let x = i32::try_from(entry.get("x")?.as_i64()?).ok()? + dx;
    let y = i32::try_from(entry.get("y")?.as_i64()?).ok()? + dy;
    Some(RenderCommand::DrawText {
        text,
        x,
        y,
        color_index,
        font,
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

    /// Unit under test: `status_bar_commands` Blit count against the JSON `images` and
    /// `imagesInvenroy` arrays.
    ///
    /// Preconditions: `STATUS_BAR_JSON` is the embedded `status_bar_vga.json` with valid
    /// top-level `images` and `imagesInvenroy` arrays.
    ///
    /// Invariants asserted: the number of `Blit` commands returned equals the number of entries
    /// in both arrays combined (`images` frame tiles + `imagesInvenroy` Jill portrait tiles).
    #[test]
    fn status_bar_commands_blit_count_matches_json_images() {
        let commands = status_bar_commands();
        let json: serde_json::Value =
            serde_json::from_str(STATUS_BAR_JSON).expect("STATUS_BAR_JSON must be valid JSON");
        let images_len = json["images"]
            .as_array()
            .expect("images must be an array")
            .len();
        let images_inv_len = json["imagesInvenroy"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        let expected = images_len + images_inv_len;
        let blit_count = commands
            .iter()
            .filter(|cmd| matches!(cmd, RenderCommand::Blit { .. }))
            .count();
        assert_eq!(
            blit_count, expected,
            "Blit count must match images + imagesInvenroy array lengths"
        );
    }

    /// Unit under test: `status_bar_commands` `DrawText` count includes `text` + `bigtext` at
    /// minimum, plus additional labels from `inventory_conf.json` and `control_area.json`.
    ///
    /// Preconditions: `STATUS_BAR_JSON` is the embedded `status_bar_vga.json` with `text` and
    /// `bigtext` arrays at the top level.
    ///
    /// Invariants asserted: the total `DrawText` count is at least `text_len + bigtext_len`
    /// (the status-bar labels from `status_bar_vga.json`). Extra commands from the inventory
    /// and control-area JSON files are allowed.
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
        assert!(
            drawtext_count >= text_len + bigtext_len,
            "DrawText count ({drawtext_count}) must be at least text + bigtext lengths ({min})",
            min = text_len + bigtext_len,
        );
    }

    /// Unit under test: `status_bar_commands` ordering — all blits before all `DrawText`
    /// commands, and the first `DrawText` commands are the `text` then `bigtext` entries from
    /// `status_bar_vga.json` in JSON order.
    ///
    /// Preconditions: `STATUS_BAR_JSON` has at least one entry in `images`, `text`, and
    /// `bigtext`.
    ///
    /// Invariants asserted:
    /// - Every `Blit` command appears before every `DrawText` command.
    /// - The first `text_len + bigtext_len` `DrawText` commands match the concatenation
    ///   `text ++ bigtext` from `status_bar_vga.json` in `(text, x, y, color)` order.
    ///   Additional `DrawText` commands from `inventory_conf.json` / `control_area.json`
    ///   are allowed to follow.
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

        assert!(
            actual.len() >= expected.len(),
            "fewer DrawText commands ({}) than expected status-bar labels ({})",
            actual.len(),
            expected.len(),
        );
        assert_eq!(
            &actual[..expected.len()],
            expected.as_slice(),
            "first DrawText commands must be JSON `text` followed by `bigtext`, in order"
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
