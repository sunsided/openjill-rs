//! High-score name-entry screen shown on game over for a qualifying score.
//!
//! Installed by the orchestrator from `game_over` when the final score would
//! make the high-score table. The player types a name (via the keyboard
//! text-input channel on [`RuntimeState::text_input`]); confirming emits a
//! [`ScreenTransition::RecordHighScore`] which the orchestrator records before
//! returning to the start menu.

use openjill_core::{
    ActiveInput, FontSize, InputCommand, RenderCommand, RuntimeState, ScreenHandler,
    ScreenTransition, TickResult,
};

/// Maximum high-score name length in bytes (the CFG name field is 10 bytes).
const NAME_MAX: usize = 10;

/// Default name recorded when the player confirms an empty entry.
const DEFAULT_NAME: &str = "JILL";

/// Left edge of the entry box in framebuffer pixels.
const BOX_X: i32 = 80;
/// Top edge of the entry box in framebuffer pixels.
const BOX_Y: i32 = 72;
/// Width of the entry box in pixels.
const BOX_W: u32 = 160;
/// Height of the entry box in pixels.
const BOX_H: u32 = 56;
/// VGA palette index filling the entry box background.
const BOX_COLOR: u8 = 1;
/// EGA text color index for the entry box text.
const TEXT_COLOR: u8 = 4;

/// Prompts the player to type their name for a qualifying end-of-run score.
pub struct HighScoreEntryScreen {
    /// Final score being recorded.
    score: i32,
    /// Name typed so far.
    name: String,
    /// Whether a confirm / backspace / cancel key was held last tick, so one
    /// key press performs exactly one action.
    nav_was_active: bool,
}

impl HighScoreEntryScreen {
    /// Creates the entry screen for a qualifying `score`.
    pub fn new(score: i32) -> Self {
        Self {
            score,
            name: String::new(),
            // Treat keys as active so any key still held from the death frame
            // must be released before it confirms an empty name.
            nav_was_active: true,
        }
    }

    /// Builds the entry-box render commands.
    fn render(&self) -> Vec<RenderCommand> {
        let mut commands = vec![RenderCommand::FillRect {
            x: BOX_X,
            y: BOX_Y,
            width: BOX_W,
            height: BOX_H,
            color: BOX_COLOR,
        }];
        let lines = [
            (String::from("NEW HIGH SCORE!"), FontSize::Big),
            (format!("SCORE: {}", self.score), FontSize::Small),
            (String::from("ENTER NAME:"), FontSize::Small),
            (format!("{}_", self.name), FontSize::Small),
        ];
        for (index, (text, font)) in lines.into_iter().enumerate() {
            commands.push(RenderCommand::DrawText {
                text,
                x: BOX_X + 8,
                y: BOX_Y + 6 + index as i32 * 12,
                color_index: TEXT_COLOR,
                font,
            });
        }
        commands
    }
}

impl ScreenHandler for HighScoreEntryScreen {
    fn tick(&mut self, input: &ActiveInput, state: &mut RuntimeState) -> TickResult {
        // Append typed characters (single-tick channel). Only printable ASCII
        // is accepted - the CFG high-score name field stores printable ASCII,
        // so other characters would be stripped on persist (leaving an
        // apparently-non-empty name that saves blank). The field is byte-sized;
        // ASCII chars are one byte each, so `NAME_MAX` is the character cap too.
        for ch in &state.text_input {
            if (ch.is_ascii_graphic() || *ch == ' ') && self.name.len() < NAME_MAX {
                self.name.push(*ch);
            }
        }

        let confirm =
            input.contains(&InputCommand::ThrowItem) || input.contains(&InputCommand::Jump);
        let backspace = input.contains(&InputCommand::PrevInventory);
        let cancel = input.contains(&InputCommand::Pause);
        let active = confirm || backspace || cancel;

        let mut transition = None;
        if active && !self.nav_was_active {
            if confirm || cancel {
                // Confirm or Escape both finish; an empty name falls back to a
                // default so the score is always recorded once it qualified.
                let name = if self.name.trim().is_empty() {
                    DEFAULT_NAME.to_string()
                } else {
                    self.name.clone()
                };
                transition = Some(ScreenTransition::RecordHighScore {
                    name,
                    score: self.score,
                });
            } else if backspace {
                self.name.pop();
            }
        }
        self.nav_was_active = active;

        TickResult {
            commands: self.render(),
            transition,
            sound_events: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HighScoreEntryScreen;
    use openjill_core::{ActiveInput, InputCommand, RuntimeState, ScreenHandler, ScreenTransition};

    /// Unit under test: typing a name then confirming emits
    /// [`ScreenTransition::RecordHighScore`] with the typed name and score.
    #[test]
    fn typed_name_then_confirm_records_high_score() {
        let mut screen = HighScoreEntryScreen::new(4242);
        let mut state = RuntimeState::new();

        // Release any held key (reset debounce), then type "ACE".
        screen.tick(&ActiveInput::new(), &mut state);
        state.text_input = vec!['A', 'C', 'E'];
        screen.tick(&ActiveInput::new(), &mut state);
        state.text_input.clear();

        let mut confirm = ActiveInput::new();
        confirm.insert(InputCommand::ThrowItem);
        let result = screen.tick(&confirm, &mut state);
        match result.transition {
            Some(ScreenTransition::RecordHighScore { name, score }) => {
                assert_eq!(name, "ACE");
                assert_eq!(score, 4242);
            }
            other => panic!("expected RecordHighScore, got {other:?}"),
        }
    }

    /// Unit under test: an entry of only non-ASCII characters (which the CFG
    /// strips on persist) is treated as empty and records the default name.
    #[test]
    fn non_ascii_only_entry_records_default_name() {
        let mut screen = HighScoreEntryScreen::new(50);
        let mut state = RuntimeState::new();

        screen.tick(&ActiveInput::new(), &mut state); // reset debounce
        state.text_input = vec!['é', '€']; // non-ASCII: filtered out
        screen.tick(&ActiveInput::new(), &mut state);
        state.text_input.clear();

        let mut confirm = ActiveInput::new();
        confirm.insert(InputCommand::ThrowItem);
        match screen.tick(&confirm, &mut state).transition {
            Some(ScreenTransition::RecordHighScore { name, .. }) => assert_eq!(name, "JILL"),
            other => panic!("expected default-name RecordHighScore, got {other:?}"),
        }
    }

    /// Unit under test: confirming with no typed name records the default name.
    #[test]
    fn empty_name_confirm_records_default_name() {
        let mut screen = HighScoreEntryScreen::new(100);
        let mut state = RuntimeState::new();

        screen.tick(&ActiveInput::new(), &mut state); // reset debounce
        let mut confirm = ActiveInput::new();
        confirm.insert(InputCommand::ThrowItem);
        let result = screen.tick(&confirm, &mut state);
        match result.transition {
            Some(ScreenTransition::RecordHighScore { name, score }) => {
                assert_eq!(name, "JILL");
                assert_eq!(score, 100);
            }
            other => panic!("expected RecordHighScore, got {other:?}"),
        }
    }
}
