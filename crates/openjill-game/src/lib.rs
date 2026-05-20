#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use openjill_core::{InputCommand, Palette};
use openjill_data::DataDirectory;
use openjill_data::sha::ShaFile;
use openjill_render::{Presenter, PresenterError, SurfaceError};
use thiserror::Error;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

/// Default keyboard mapping from physical keys to logical input commands.
static INPUT_COMMAND_KEY_MAP: &[(KeyCode, InputCommand)] = &[
    (KeyCode::ArrowLeft, InputCommand::MoveLeft),
    (KeyCode::ArrowRight, InputCommand::MoveRight),
    (KeyCode::ArrowUp, InputCommand::Jump),
    (KeyCode::Space, InputCommand::Jump),
    (KeyCode::AltLeft, InputCommand::Jump),
    (KeyCode::AltRight, InputCommand::Jump),
    (KeyCode::ArrowDown, InputCommand::Duck),
    (KeyCode::ControlLeft, InputCommand::ThrowItem),
    (KeyCode::ControlRight, InputCommand::ThrowItem),
    (KeyCode::Tab, InputCommand::NextInventory),
    (KeyCode::Backspace, InputCommand::PrevInventory),
    (KeyCode::Escape, InputCommand::Pause),
    (KeyCode::KeyQ, InputCommand::Quit),
];

/// Runs the game event loop for the configured data directory.
pub fn run(data_dir: PathBuf) -> Result<(), GameError> {
    let event_loop = EventLoop::new()?;
    let mut app = GameApp::new(data_dir);
    event_loop.run_app(&mut app)?;
    app.error.take().map_or(Ok(()), Err)
}

/// Event-loop application state that owns the game window and presenter.
pub struct GameApp {
    /// Data directory selected by the CLI.
    data_dir: PathBuf,
    /// Active native window when initialization succeeds.
    window: Option<Arc<Window>>,
    /// Rendering presenter initialized from the native window.
    presenter: Option<Presenter>,
    /// Active palette used to expand indexed frame data during presentation.
    palette: Palette,
    /// Deferred startup/runtime error propagated after event-loop shutdown.
    error: Option<GameError>,
    /// Physical keys currently held down; source of truth for active commands.
    ///
    /// Tracking physical keys (rather than commands directly) keeps multi-key
    /// bindings correct when one of several keys mapped to the same command is
    /// released while another remains held.
    pressed_keys: BTreeSet<KeyCode>,
}

impl GameApp {
    /// Creates a new game app shell before the event loop enters `resumed`.
    ///
    /// Loads the startup palette from the first non-empty SHA color map found in
    /// `JILL1.SHA`, or falls back to a greyscale ramp when no color map is available.
    pub fn new(data_dir: PathBuf) -> Self {
        let palette = load_palette_from_data_dir(&data_dir);
        Self {
            data_dir,
            window: None,
            presenter: None,
            palette,
            error: None,
            pressed_keys: BTreeSet::new(),
        }
    }

    /// Translates one physical key code into its mapped logical command.
    ///
    /// Returns `None` when no default binding exists for the key.
    fn map_key_to_input_command(key_code: KeyCode) -> Option<InputCommand> {
        INPUT_COMMAND_KEY_MAP
            .iter()
            .find_map(|(mapped_key, command)| (*mapped_key == key_code).then_some(*command))
    }

    /// Applies one key press or release to the pressed-keys set.
    ///
    /// Unmapped keys are silently ignored and leave the set unchanged.
    fn update_pressed_keys(
        pressed_keys: &mut BTreeSet<KeyCode>,
        key_code: KeyCode,
        state: ElementState,
    ) {
        if Self::map_key_to_input_command(key_code).is_none() {
            return;
        }
        match state {
            ElementState::Pressed => {
                pressed_keys.insert(key_code);
            }
            ElementState::Released => {
                pressed_keys.remove(&key_code);
            }
        }
    }

    /// Derives the set of logical commands currently active from held keys.
    ///
    /// Multiple held keys mapped to the same command collapse to a single entry,
    /// and a command remains active as long as at least one bound key is held.
    fn active_commands(pressed_keys: &BTreeSet<KeyCode>) -> BTreeSet<InputCommand> {
        pressed_keys
            .iter()
            .filter_map(|key| Self::map_key_to_input_command(*key))
            .collect()
    }

    /// Runs one game tick with the currently active logical input commands.
    ///
    /// This per-frame hook is where orchestration code advances game state from inputs.
    fn tick(active_commands: &BTreeSet<InputCommand>) {
        let _ = active_commands;
    }
}

/// Loads the startup palette from the first non-empty SHA color map in `JILL1.SHA`.
///
/// Falls back to [`Palette::greyscale_fallback`] when the file is missing, unreadable, or
/// contains no non-empty color map, and logs which source was used to `stderr`.
fn load_palette_from_data_dir(data_dir: &std::path::Path) -> Palette {
    let directory = DataDirectory::new(data_dir.to_path_buf());
    let mut reader = match directory.open_reader("JILL1.SHA") {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!(
                "openjill-game: JILL1.SHA unavailable ({error}); using greyscale palette fallback"
            );
            return Palette::greyscale_fallback();
        }
    };
    let sha = match ShaFile::parse(&mut reader) {
        Ok(sha) => sha,
        Err(error) => {
            eprintln!(
                "openjill-game: failed to parse JILL1.SHA color map ({error}); using greyscale palette fallback"
            );
            return Palette::greyscale_fallback();
        }
    };

    for tileset in sha.tilesets() {
        if let Some(color_map) = tileset.color_map().filter(|entries| !entries.is_empty()) {
            eprintln!(
                "openjill-game: palette loaded from JILL1.SHA tileset entry {}",
                tileset.entry_index()
            );
            return Palette::from_sha_color_map(color_map);
        }
    }

    eprintln!(
        "openjill-game: no non-empty color map in JILL1.SHA; using greyscale palette fallback"
    );
    Palette::greyscale_fallback()
}

impl ApplicationHandler for GameApp {
    /// Initializes the native window and renderer when the app resumes.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let data_label = self
            .data_dir
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("data");
        let title = format!("OpenJill - {data_label}");
        let attributes = WindowAttributes::default().with_title(title);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.error = Some(GameError::WindowCreation(error));
                event_loop.exit();
                return;
            }
        };
        let mut presenter = match pollster::block_on(Presenter::new(window.clone())) {
            Ok(presenter) => presenter,
            Err(error) => {
                self.error = Some(GameError::Presenter(error));
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        presenter.resize(size.width, size.height);
        self.window = Some(window);
        self.presenter = Some(presenter);
    }

    /// Handles window events required for the initial render/input integration.
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .map(|window| window.id())
            .is_some_and(|active_id| active_id != window_id)
        {
            return;
        }

        match event {
            WindowEvent::Resized(size) => {
                if let Some(presenter) = self.presenter.as_mut() {
                    presenter.resize(size.width, size.height);
                }
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key_code) = event.physical_key {
                    Self::update_pressed_keys(&mut self.pressed_keys, key_code, event.state);
                }
            }
            WindowEvent::Focused(false) => {
                // Release events are not delivered when focus is lost (e.g. alt-tab),
                // so clear held keys to avoid sticky inputs after focus returns.
                self.pressed_keys.clear();
            }
            _ => {}
        }
    }

    /// Clears and presents one frame while the loop is idle, then requests another redraw.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        Self::tick(&Self::active_commands(&self.pressed_keys));
        if let Some(presenter) = self.presenter.as_mut() {
            presenter.clear(0);
            match presenter.present(&self.palette) {
                Ok(()) => {}
                Err(PresenterError::SurfaceError(SurfaceError::Lost))
                | Err(PresenterError::SurfaceError(SurfaceError::Outdated)) => {
                    presenter.reconfigure();
                }
                Err(PresenterError::SurfaceError(SurfaceError::Timeout))
                | Err(PresenterError::SurfaceError(SurfaceError::Occluded)) => {}
                Err(error) => {
                    self.error = Some(GameError::Presenter(error));
                    event_loop.exit();
                    return;
                }
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

/// Errors returned while running the game event loop.
#[derive(Debug, Error)]
pub enum GameError {
    /// Winit event-loop setup failed.
    #[error("failed to create or run window event loop: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    /// Native window creation failed during `resumed`.
    #[error("failed to create native game window: {0}")]
    WindowCreation(#[source] winit::error::OsError),
    /// Renderer setup or frame presentation failed.
    #[error(transparent)]
    Presenter(#[from] PresenterError),
}

#[cfg(test)]
mod tests {
    use super::GameApp;
    use openjill_core::InputCommand;
    use std::collections::BTreeSet;
    use winit::event::ElementState;
    use winit::keyboard::KeyCode;

    /// Unit under test: `GameApp::map_key_to_input_command`.
    ///
    /// Preconditions: representative keys from the default control mapping are provided,
    /// including keys that intentionally share the same command (jump and throw item).
    ///
    /// Invariants asserted: each mapped key resolves to the expected `InputCommand`, and
    /// shared bindings map to the same command consistently.
    #[test]
    fn map_key_to_input_command_resolves_default_bindings() {
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::ArrowLeft),
            Some(InputCommand::MoveLeft)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::ArrowRight),
            Some(InputCommand::MoveRight)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::ArrowUp),
            Some(InputCommand::Jump)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::Space),
            Some(InputCommand::Jump)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::AltLeft),
            Some(InputCommand::Jump)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::ArrowDown),
            Some(InputCommand::Duck)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::ControlLeft),
            Some(InputCommand::ThrowItem)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::Tab),
            Some(InputCommand::NextInventory)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::Backspace),
            Some(InputCommand::PrevInventory)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::Escape),
            Some(InputCommand::Pause)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::KeyQ),
            Some(InputCommand::Quit)
        );
    }

    /// Unit under test: `GameApp::map_key_to_input_command`.
    ///
    /// Preconditions: an unmapped key code is provided.
    ///
    /// Invariants asserted: unmapped keys are ignored and return `None`.
    #[test]
    fn map_key_to_input_command_ignores_unmapped_keys() {
        assert_eq!(GameApp::map_key_to_input_command(KeyCode::KeyZ), None);
    }

    /// Unit under test: `GameApp::update_pressed_keys` and `GameApp::active_commands`.
    ///
    /// Preconditions: an empty pressed-keys set receives mapped and unmapped key events,
    /// with both pressed and released states.
    ///
    /// Invariants asserted: mapped presses are tracked, mapped releases are removed,
    /// the derived active-command set reflects held keys, and unmapped keys leave both
    /// the pressed-key set and the derived command set unchanged.
    #[test]
    fn update_pressed_keys_tracks_press_release_and_ignores_unmapped_keys() {
        let mut pressed = BTreeSet::new();
        GameApp::update_pressed_keys(&mut pressed, KeyCode::ArrowLeft, ElementState::Pressed);
        GameApp::update_pressed_keys(&mut pressed, KeyCode::Space, ElementState::Pressed);
        assert_eq!(
            GameApp::active_commands(&pressed),
            BTreeSet::from([InputCommand::MoveLeft, InputCommand::Jump])
        );

        GameApp::update_pressed_keys(&mut pressed, KeyCode::ArrowLeft, ElementState::Released);
        assert_eq!(
            GameApp::active_commands(&pressed),
            BTreeSet::from([InputCommand::Jump])
        );

        GameApp::update_pressed_keys(&mut pressed, KeyCode::KeyZ, ElementState::Pressed);
        assert_eq!(
            GameApp::active_commands(&pressed),
            BTreeSet::from([InputCommand::Jump])
        );
    }

    /// Unit under test: `GameApp::active_commands`.
    ///
    /// Preconditions: multiple physical keys mapped to the same logical command are held,
    /// then released one at a time.
    ///
    /// Invariants asserted: a command stays active while any of its bound keys is held,
    /// and clears only after the last bound key is released.
    #[test]
    fn active_commands_persist_until_last_bound_key_released() {
        let mut pressed = BTreeSet::new();
        GameApp::update_pressed_keys(&mut pressed, KeyCode::ArrowUp, ElementState::Pressed);
        GameApp::update_pressed_keys(&mut pressed, KeyCode::Space, ElementState::Pressed);
        GameApp::update_pressed_keys(&mut pressed, KeyCode::AltLeft, ElementState::Pressed);
        assert_eq!(
            GameApp::active_commands(&pressed),
            BTreeSet::from([InputCommand::Jump])
        );

        GameApp::update_pressed_keys(&mut pressed, KeyCode::ArrowUp, ElementState::Released);
        assert_eq!(
            GameApp::active_commands(&pressed),
            BTreeSet::from([InputCommand::Jump])
        );

        GameApp::update_pressed_keys(&mut pressed, KeyCode::Space, ElementState::Released);
        assert_eq!(
            GameApp::active_commands(&pressed),
            BTreeSet::from([InputCommand::Jump])
        );

        GameApp::update_pressed_keys(&mut pressed, KeyCode::AltLeft, ElementState::Released);
        assert!(GameApp::active_commands(&pressed).is_empty());
    }

    /// Unit under test: focus-loss handling clears the pressed-keys set.
    ///
    /// Preconditions: keys are held when focus is lost; release events for them never arrive.
    ///
    /// Invariants asserted: clearing `pressed_keys` (as the focus-loss handler does)
    /// drops all derived active commands so inputs do not stick across alt-tab.
    #[test]
    fn clearing_pressed_keys_drops_all_active_commands() {
        let mut pressed = BTreeSet::new();
        GameApp::update_pressed_keys(&mut pressed, KeyCode::ArrowLeft, ElementState::Pressed);
        GameApp::update_pressed_keys(&mut pressed, KeyCode::Space, ElementState::Pressed);
        assert!(!GameApp::active_commands(&pressed).is_empty());

        pressed.clear();
        assert!(GameApp::active_commands(&pressed).is_empty());
    }
}
