#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use openjill_core::{InputCommand, Palette};
use openjill_data::DataDirectory;
use openjill_data::episode::Episode;
use openjill_data::sha::ShaFile;
use openjill_render::{Presenter, PresenterError, SurfaceError};
use thiserror::Error;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

pub mod asset_cache;
pub mod entities;
pub mod level_config;
pub mod orchestrator;
pub mod saves;
pub mod screens;
pub mod status_bar;

use orchestrator::GameOrchestrator;

/// Default keyboard mapping from physical keys to logical input commands.
///
/// Matches the original DOS Jill of the Jungle bindings and the
/// `keysControlText` entries in `control_area.json`: SHIFT jumps, ALT
/// throws the held inventory item. SPACE is kept as a secondary jump key
/// for Java-port parity (`SimpleGameKeyHandler` maps `VK_SHIFT` to jump),
/// and CTRL is kept as a secondary throw-item key for menu confirmation
/// in [`crate::screens::start_menu::StartMenuScreen`].
static INPUT_COMMAND_KEY_MAP: &[(KeyCode, InputCommand)] = &[
    (KeyCode::ArrowLeft, InputCommand::MoveLeft),
    (KeyCode::ArrowRight, InputCommand::MoveRight),
    (KeyCode::ArrowUp, InputCommand::Up),
    (KeyCode::Space, InputCommand::Jump),
    (KeyCode::ShiftLeft, InputCommand::Jump),
    (KeyCode::ShiftRight, InputCommand::Jump),
    (KeyCode::AltLeft, InputCommand::ThrowItem),
    (KeyCode::AltRight, InputCommand::ThrowItem),
    (KeyCode::ArrowDown, InputCommand::Duck),
    (KeyCode::ControlLeft, InputCommand::ThrowItem),
    (KeyCode::ControlRight, InputCommand::ThrowItem),
    (KeyCode::Tab, InputCommand::NextInventory),
    (KeyCode::Backspace, InputCommand::PrevInventory),
    (KeyCode::Escape, InputCommand::Pause),
    (KeyCode::KeyQ, InputCommand::Quit),
    (KeyCode::KeyN, InputCommand::ToggleNoise),
    (KeyCode::KeyT, InputCommand::ToggleTurtle),
    (KeyCode::KeyS, InputCommand::Save),
    (KeyCode::KeyR, InputCommand::Restore),
];

/// Runs the game event loop for the configured data directory and episode.
pub fn run(data_dir: PathBuf, episode: &'static Episode) -> Result<(), GameError> {
    let event_loop = EventLoop::new()?;
    let mut app = GameApp::new(data_dir, episode);
    event_loop.run_app(&mut app)?;
    app.error.take().map_or(Ok(()), Err)
}

/// Game tick interval in milliseconds, matching the Java `open_jill.properties`
/// value of 55 ms.
const TICK_INTERVAL: Duration = Duration::from_millis(55);

/// Event-loop application state that owns the game window and presenter.
pub struct GameApp {
    /// Data directory selected by the CLI.
    data_dir: PathBuf,
    /// Active episode descriptor resolved by the CLI.
    episode: &'static Episode,
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
    /// Game orchestrator that owns the active screen handler.
    ///
    /// `None` when the required asset files are unavailable at startup, in
    /// which case the event loop continues with no game logic.
    orchestrator: Option<GameOrchestrator>,
    /// Timestamp of the last fired game tick, used to enforce the 55 ms tick
    /// interval regardless of the vsync rate.
    last_tick: Option<Instant>,
}

impl GameApp {
    /// Creates a new game app shell before the event loop enters `resumed`.
    ///
    /// Loads the startup palette from the first non-empty SHA color map found in
    /// the active episode's SHA file ([`Episode::sha`]), or falls back to a
    /// greyscale ramp when no color map is available. Also constructs the
    /// [`GameOrchestrator`] if the required asset files are present; logs a
    /// warning to stderr and continues without game logic if they are not.
    pub fn new(data_dir: PathBuf, episode: &'static Episode) -> Self {
        let startup_assets = load_startup_assets_from_data_dir(&data_dir, episode);
        let orchestrator = match GameOrchestrator::new(
            DataDirectory::new(data_dir.clone()),
            episode,
        ) {
            Ok(orch) => {
                println!("openjill-game: game orchestrator initialized");
                Some(orch)
            }
            Err(err) => {
                eprintln!(
                    "openjill-game: orchestrator init failed ({err}); running without game logic"
                );
                None
            }
        };
        Self {
            data_dir,
            episode,
            window: None,
            presenter: None,
            palette: startup_assets.palette,
            error: None,
            pressed_keys: BTreeSet::new(),
            orchestrator,
            last_tick: None,
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

    /// Dispatches debug-only level-transition hotkeys (debug builds only).
    ///
    /// Wired so a developer can exercise the level-transition pipeline
    /// without gameplay (epic 6) having attached real checkpoint objects to
    /// the world map yet. Single-fire on key down: edge-triggered by the
    /// caller via `event.state == Pressed && !event.repeat`.
    ///
    /// | Key | Action |
    /// |-----|--------|
    /// | `L` | Force-transition into level 1 of the active episode |
    /// | `K` | Send `CheckpointChangeLevelPrevious` (return to map) |
    /// | `J` | Send `DieRestartLevel` |
    ///
    /// Level files are named `<level_number>.<jn_ext>` on disk where
    /// `jn_ext` is the active episode's JN extension (for example
    /// `1.JN1`, `2.JN1`, ..., `50.JN1` in episode 1). `L` calls
    /// [`GameOrchestrator::force_transition`] directly: the dispatcher route
    /// requires a subscriber, and [`crate::screens::map_screen::MapScreen`]
    /// does not subscribe to `CheckpointChangeLevel`, so a queued message
    /// would never fire a transition. `K` and `J` use the dispatcher because
    /// they target [`crate::screens::level_screen::LevelScreen`], which
    /// subscribes for both. (`R` is a gameplay key - control-panel RESTORE -
    /// so the debug restart moved to `J`.)
    #[cfg(debug_assertions)]
    fn handle_debug_hotkey(&mut self, key_code: KeyCode) {
        let Some(orch) = self.orchestrator.as_mut() else {
            return;
        };
        match key_code {
            KeyCode::KeyL => {
                let level_file = self.episode.level_jn(1);
                orch.force_transition(openjill_core::ScreenTransition::Level {
                    file: level_file.clone(),
                    number: 1,
                });
                eprintln!("openjill-game: debug: force-transition -> LevelScreen {level_file}");
            }
            KeyCode::KeyK => {
                orch.dispatcher_mut().send(
                    openjill_core::MessageType::CheckpointChangeLevelPrevious,
                    openjill_core::MessagePayload::None,
                );
                eprintln!("openjill-game: debug: dispatched CheckpointChangeLevelPrevious");
            }
            KeyCode::KeyJ => {
                orch.dispatcher_mut().send(
                    openjill_core::MessageType::DieRestartLevel,
                    openjill_core::MessagePayload::None,
                );
                eprintln!("openjill-game: debug: dispatched DieRestartLevel");
            }
            _ => {}
        }
    }
}

/// Startup rendering assets loaded from the active episode's SHA file.
struct StartupAssets {
    /// Palette used to present indexed frames.
    palette: Palette,
}

/// Loads startup rendering assets from the active episode's SHA file
/// ([`Episode::sha`]).
///
/// The VGA palette is always the hardcoded `Palette::jill_vga()` - it does not come from
/// the SHA file. SHA is opened here only for diagnostic logging (font tileset index).
fn load_startup_assets_from_data_dir(
    data_dir: &std::path::Path,
    episode: &Episode,
) -> StartupAssets {
    let directory = DataDirectory::new(data_dir.to_path_buf());
    let sha_name = episode.sha;
    match directory.open_reader(sha_name) {
        Ok(mut reader) => match ShaFile::parse(&mut reader) {
            Ok(sha) => {
                if let Some(font_tileset) = sha.tilesets().iter().find(|ts| ts.is_font()) {
                    println!(
                        "openjill-game: first SHA font tileset index: {}",
                        font_tileset.entry_index()
                    );
                } else {
                    eprintln!("openjill-game: no SHA tileset with is_font=true was found");
                }
            }
            Err(error) => {
                eprintln!("openjill-game: failed to parse {sha_name} ({error})");
            }
        },
        Err(error) => {
            eprintln!("openjill-game: {sha_name} unavailable ({error})");
        }
    }
    StartupAssets {
        palette: Palette::jill_vga(),
    }
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
                    #[cfg(debug_assertions)]
                    if event.state == ElementState::Pressed && !event.repeat {
                        self.handle_debug_hotkey(key_code);
                    }
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

    /// Advances the game tick when the 55 ms interval has elapsed, then presents one frame.
    ///
    /// Game ticks fire at approximately 18 Hz (every 55 ms). When an orchestrator is active,
    /// the render commands it produces are executed via [`Presenter::execute_and_present`],
    /// which dispatches each command against the indexed framebuffer and then uploads the
    /// result to the GPU. When no orchestrator is available (assets missing at startup),
    /// a black frame is presented instead.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let should_tick = self
            .last_tick
            .map(|last| now.duration_since(last) >= TICK_INTERVAL)
            .unwrap_or(true);

        if should_tick {
            if let Some(orch) = self.orchestrator.as_mut() {
                let active = Self::active_commands(&self.pressed_keys);
                orch.tick(&active);
                if orch.is_quitting() {
                    event_loop.exit();
                    return;
                }
            }
            self.last_tick = Some(now);
        }

        if let Some(presenter) = self.presenter.as_mut() {
            let result = match &self.orchestrator {
                Some(orch) => presenter.execute_and_present(
                    orch.last_commands(),
                    &orch.cache().sha,
                    &self.palette,
                ),
                None => {
                    presenter.clear(0);
                    presenter.present(&self.palette)
                }
            };
            match result {
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
            Some(InputCommand::Up)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::Space),
            Some(InputCommand::Jump)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::ShiftLeft),
            Some(InputCommand::Jump)
        );
        assert_eq!(
            GameApp::map_key_to_input_command(KeyCode::AltLeft),
            Some(InputCommand::ThrowItem)
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
        GameApp::update_pressed_keys(&mut pressed, KeyCode::Space, ElementState::Pressed);
        GameApp::update_pressed_keys(&mut pressed, KeyCode::ShiftLeft, ElementState::Pressed);
        GameApp::update_pressed_keys(&mut pressed, KeyCode::ShiftRight, ElementState::Pressed);
        assert_eq!(
            GameApp::active_commands(&pressed),
            BTreeSet::from([InputCommand::Jump])
        );

        GameApp::update_pressed_keys(&mut pressed, KeyCode::Space, ElementState::Released);
        assert_eq!(
            GameApp::active_commands(&pressed),
            BTreeSet::from([InputCommand::Jump])
        );

        GameApp::update_pressed_keys(&mut pressed, KeyCode::ShiftLeft, ElementState::Released);
        assert_eq!(
            GameApp::active_commands(&pressed),
            BTreeSet::from([InputCommand::Jump])
        );

        GameApp::update_pressed_keys(&mut pressed, KeyCode::ShiftRight, ElementState::Released);
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
