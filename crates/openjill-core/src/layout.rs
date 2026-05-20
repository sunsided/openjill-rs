//! Screen layout constants derived from `status_bar_vga.json` and `jill_const.properties`.

/// Native game resolution width in pixels.
pub const SCREEN_WIDTH: u32 = 320;
/// Native game resolution height in pixels.
pub const SCREEN_HEIGHT: u32 = 200;

/// Game area left edge in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const GAME_AREA_X: i32 = 80;
/// Game area top edge in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const GAME_AREA_Y: i32 = 16;
/// Game area width in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const GAME_AREA_W: u32 = 232;
/// Game area height in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const GAME_AREA_H: u32 = 160;

/// Control area left edge in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const CONTROL_AREA_X: i32 = 8;
/// Control area top edge in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const CONTROL_AREA_Y: i32 = 16;
/// Control area width in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const CONTROL_AREA_W: u32 = 64;
/// Control area height in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const CONTROL_AREA_H: u32 = 85;

/// Inventory area left edge in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const INVENTORY_AREA_X: i32 = 8;
/// Inventory area top edge in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const INVENTORY_AREA_Y: i32 = 107;
/// Inventory area width in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const INVENTORY_AREA_W: u32 = 64;
/// Inventory area height in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const INVENTORY_AREA_H: u32 = 69;

/// Message bar top edge in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const MESSAGE_BAR_Y: i32 = 188;
/// Message bar height in framebuffer pixels (VGA layout, from `status_bar_vga.json`).
pub const MESSAGE_BAR_H: u32 = 12;

/// Tile and block size in pixels (`JillConst.BLOCK_SIZE = 16`).
pub const BLOCK_SIZE: u32 = 16;

/// Horizontal screen-update border in pixels; objects outside skip their update step.
///
/// Equals 6 blocks (`JillConst.xUpdateScreenBorder = 96`).
pub const X_UPDATE_BORDER: u32 = 96;

/// Vertical screen-update border in pixels; objects outside skip their update step.
///
/// Equals 3 blocks (`JillConst.yUpdateScreenBorder = 48`).
pub const Y_UPDATE_BORDER: u32 = 48;

/// Level-message display duration in ticks.
///
/// Derived from `open_jill.properties` `levelMessageTimeout = 4000 ms` divided by the
/// 55 ms tick delay, rounded down to 72 ticks.
pub const LEVEL_MESSAGE_TICKS: u32 = 72;

/// Zaphold counter placed on objects immediately after they touch the player.
///
/// Mirrors `JillConst.zapholdValueAfterTouchPlayer = 3`.
pub const ZAPHOLD_AFTER_TOUCH: u32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit under test: all VGA layout constants in the `layout` module.
    ///
    /// Preconditions: none; these are compile-time constants.
    ///
    /// Invariants asserted: every constant matches the exact value specified in
    /// `status_bar_vga.json` and `jill_const.properties`, providing a regression
    /// guard against accidental edits.
    #[test]
    fn layout_constants_match_source_values() {
        assert_eq!(SCREEN_WIDTH, 320);
        assert_eq!(SCREEN_HEIGHT, 200);

        assert_eq!(GAME_AREA_X, 80);
        assert_eq!(GAME_AREA_Y, 16);
        assert_eq!(GAME_AREA_W, 232);
        assert_eq!(GAME_AREA_H, 160);

        assert_eq!(CONTROL_AREA_X, 8);
        assert_eq!(CONTROL_AREA_Y, 16);
        assert_eq!(CONTROL_AREA_W, 64);
        assert_eq!(CONTROL_AREA_H, 85);

        assert_eq!(INVENTORY_AREA_X, 8);
        assert_eq!(INVENTORY_AREA_Y, 107);
        assert_eq!(INVENTORY_AREA_W, 64);
        assert_eq!(INVENTORY_AREA_H, 69);

        assert_eq!(MESSAGE_BAR_Y, 188);
        assert_eq!(MESSAGE_BAR_H, 12);

        assert_eq!(BLOCK_SIZE, 16);
        assert_eq!(X_UPDATE_BORDER, 96);
        assert_eq!(Y_UPDATE_BORDER, 48);
        assert_eq!(LEVEL_MESSAGE_TICKS, 72);
        assert_eq!(ZAPHOLD_AFTER_TOUCH, 3);
    }
}
