//! Static VGA status bar render commands and game-area offset helpers.

use openjill_core::RenderCommand;
use openjill_core::layout::{GAME_AREA_X, GAME_AREA_Y};
use std::sync::LazyLock;

/// Embedded JSON describing the static VGA status bar tile layout.
pub(crate) const STATUS_BAR_JSON: &str =
    include_str!("../../../OpenJill/src/main/resources/status_bar_vga.json");

/// Cached static VGA status bar commands parsed from [`STATUS_BAR_JSON`].
static STATUS_BAR_COMMANDS: LazyLock<Vec<RenderCommand>> = LazyLock::new(parse_status_bar_commands);

/// Returns the static VGA status bar as a sequence of [`RenderCommand::Blit`] commands.
///
/// Clones the cached commands parsed from the embedded `status_bar_vga.json`. Each `images`
/// array entry is converted into a `Blit` command with `opaque: false`.
pub fn status_bar_commands() -> Vec<RenderCommand> {
    STATUS_BAR_COMMANDS.clone()
}

/// Parses the embedded `status_bar_vga.json` into static status bar render commands.
///
/// Returns an empty `Vec` if the JSON is missing or structurally invalid; malformed individual
/// entries are silently skipped.
fn parse_status_bar_commands() -> Vec<RenderCommand> {
    let value: serde_json::Value = match serde_json::from_str(STATUS_BAR_JSON) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(images) = value.get("images").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    images
        .iter()
        .filter_map(|img| {
            let tileset = img.get("tileset")?.as_u64()? as u8;
            let tile = img.get("tile")?.as_u64()? as u16;
            let x = img.get("x")?.as_i64()? as i32;
            let y = img.get("y")?.as_i64()? as i32;
            Some(RenderCommand::Blit {
                tileset,
                tile,
                x,
                y,
                opaque: false,
            })
        })
        .collect()
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
    }
}

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
    /// in the JSON `images` array, and every returned command is a `Blit` variant.
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
        assert_eq!(
            commands.len(),
            expected,
            "all commands must be Blit entries"
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
                } if x == GAME_AREA_X + 10 && y == GAME_AREA_Y + 20
            ),
            "game_area_blit must offset x/y by game area origin"
        );
    }
}
