//! INTRO.JN1-backed static screen handlers.
//!
//! All handlers in this module share the same loaded `JnFile` (INTRO.JN1) and
//! differ only in the background viewport offset. Each screen auto-advances to
//! the start menu after [`AUTO_ADVANCE_TICKS`] ticks or when any key is pressed.

use crate::screens::intro_background::render_intro_background;
use openjill_core::runtime::RuntimeState;
use openjill_core::{ActiveInput, ScreenHandler, ScreenTransition, TickResult};
use openjill_data::dma::DmaFile;
use openjill_data::jn::JnFile;

/// Number of ticks (at 18 Hz) before a static intro screen auto-returns to the
/// start menu.  Mirrors the Java `LEVEL_MESSAGE_TICKS` = 72 constant.
const AUTO_ADVANCE_TICKS: u32 = 72;

/// A single-background, auto-advancing INTRO.JN1 screen.
///
/// Used for the story, credits, ordering-info, and noisemaker screens. All four
/// render a fixed background viewport from `INTRO.JN1` and return to the start
/// menu either after [`AUTO_ADVANCE_TICKS`] game ticks or when the player
/// presses any key.
pub struct IntroStaticScreen {
    /// Parsed `INTRO.JN1` data used for background rendering.
    intro: JnFile,
    /// Parsed `JILL.DMA` lookup used to resolve map codes to tileset tiles.
    dma: DmaFile,
    /// Horizontal background offset in pixels (OpenJill sign convention:
    /// negative value means "scroll right by `|offset|`").
    offset_x: i32,
    /// Vertical background offset in pixels.
    offset_y: i32,
    /// Number of game ticks elapsed on this screen.
    ticks: u32,
}

impl IntroStaticScreen {
    /// Creates a new static INTRO.JN1 screen with the given viewport offset.
    pub fn new(intro: JnFile, dma: DmaFile, offset_x: i32, offset_y: i32) -> Self {
        Self {
            intro,
            dma,
            offset_x,
            offset_y,
            ticks: 0,
        }
    }
}

impl ScreenHandler for IntroStaticScreen {
    /// Renders the viewport and transitions to the start menu on key press or
    /// after [`AUTO_ADVANCE_TICKS`] ticks.
    fn tick(&mut self, input: &ActiveInput, _state: &mut RuntimeState) -> TickResult {
        self.ticks = self.ticks.saturating_add(1);
        let commands = render_intro_background(&self.intro, &self.dma, self.offset_x, self.offset_y);
        let transition = if !input.is_empty() || self.ticks >= AUTO_ADVANCE_TICKS {
            Some(ScreenTransition::StartMenu)
        } else {
            None
        };
        TickResult {
            commands,
            transition,
            sound_events: Vec::new(),
        }
    }
}

/// Creates the story screen.
///
/// Viewport offset `(-36 * 16, -2 * 16)` = `(-576, -32)` from
/// `StoryScreenJill1Handler::centerScreen` in the Java reference.
pub fn story_screen(intro: JnFile, dma: DmaFile) -> IntroStaticScreen {
    IntroStaticScreen::new(intro, dma, -576, -32)
}

/// Creates the credits screen.
///
/// Viewport offset `(-15 * 16, 0)` = `(-240, 0)` from
/// `CreditScreenJill1Handler::centerScreen` in the Java reference.
pub fn credits_screen(intro: JnFile, dma: DmaFile) -> IntroStaticScreen {
    IntroStaticScreen::new(intro, dma, -240, 0)
}

/// Creates the ordering-info screen.
///
/// Viewport offset `(-14 * 16, -15 * 16)` = `(-224, -240)` from
/// `OrderingInfoScreenJill1Handler::newScreen(14, 15)` in the Java reference.
pub fn ordering_info_screen(intro: JnFile, dma: DmaFile) -> IntroStaticScreen {
    IntroStaticScreen::new(intro, dma, -224, -240)
}

/// Creates the noisemaker screen.
///
/// Viewport offset `(-62 * 16, 0)` = `(-992, 0)` from
/// `NoisemakerScreenJill1Handler::centerScreen` in the Java reference.
pub fn noisemaker_screen(intro: JnFile, dma: DmaFile) -> IntroStaticScreen {
    IntroStaticScreen::new(intro, dma, -992, 0)
}

#[cfg(test)]
mod tests {
    use super::{IntroStaticScreen, AUTO_ADVANCE_TICKS};
    use openjill_core::runtime::RuntimeState;
    use openjill_core::{ActiveInput, InputCommand, ScreenHandler, ScreenTransition};
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;

    /// Minimal `JnFile` byte count: 128×64 background (u16 each) + u16 object
    /// count (0) + 70-byte save-data block.
    const JN_MIN_BYTES: usize = 128 * 64 * 2 + 2 + 70;

    /// Builds a minimal all-zero `JnFile` for use in unit tests.
    fn zero_jn() -> JnFile {
        JnFile::from_bytes(vec![0u8; JN_MIN_BYTES]).expect("zero JN should parse")
    }

    /// Builds a minimal empty `DmaFile` for use in unit tests.
    fn empty_dma() -> DmaFile {
        DmaFile::from_bytes(vec![]).expect("empty DMA should parse")
    }

    /// Unit under test: `IntroStaticScreen::tick` transitions to start menu on
    /// any key press regardless of elapsed ticks.
    #[test]
    fn any_key_press_transitions_to_start_menu() {
        let mut screen = IntroStaticScreen::new(zero_jn(), empty_dma(), 0, 0);
        let mut input = ActiveInput::new();
        input.insert(InputCommand::Jump);
        let result = screen.tick(&input, &mut RuntimeState::new());
        assert_eq!(result.transition, Some(ScreenTransition::StartMenu));
    }

    /// Unit under test: `IntroStaticScreen::tick` does not transition before
    /// the auto-advance threshold when no key is pressed.
    #[test]
    fn no_transition_before_auto_advance_threshold() {
        let mut screen = IntroStaticScreen::new(zero_jn(), empty_dma(), 0, 0);
        let input = ActiveInput::new();
        // Tick up to one before the threshold; no transition expected yet.
        for _ in 0..(AUTO_ADVANCE_TICKS - 1) {
            let result = screen.tick(&input, &mut RuntimeState::new());
            assert_eq!(result.transition, None, "must not transition before threshold");
        }
    }

    /// Unit under test: `IntroStaticScreen::tick` auto-transitions at the
    /// auto-advance threshold when no key is pressed.
    #[test]
    fn auto_advances_to_start_menu_at_threshold() {
        let mut screen = IntroStaticScreen::new(zero_jn(), empty_dma(), 0, 0);
        let input = ActiveInput::new();
        // Exhaust the ticks until the threshold fires.
        for _ in 0..AUTO_ADVANCE_TICKS {
            screen.tick(&input, &mut RuntimeState::new());
        }
        let result = screen.tick(&input, &mut RuntimeState::new());
        assert_eq!(result.transition, Some(ScreenTransition::StartMenu));
    }
}
