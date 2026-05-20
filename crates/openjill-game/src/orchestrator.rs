//! Game orchestrator: owns the active screen handler and drives the tick and
//! transition loop.

use openjill_core::{ActiveInput, RenderCommand, RuntimeState, ScreenHandler, ScreenTransition};
use openjill_data::DataDirectory;
use thiserror::Error;

use crate::asset_cache::{AssetCache, AssetError};
use crate::screens::intro_screens::{
    credits_screen, noisemaker_screen, ordering_info_screen, story_screen,
};
use crate::screens::start_menu::StartMenuScreen;

/// Owns the current screen handler and drives the game tick and transition loop.
///
/// Constructed once at application start from a [`DataDirectory`]. The winit
/// event loop calls [`GameOrchestrator::tick`] at approximately 18 Hz (every
/// 55 ms) and passes the returned render commands to the presenter.
pub struct GameOrchestrator {
    /// Pre-loaded game assets shared across all screens.
    cache: AssetCache,
    /// Persistent game state carried across screen transitions.
    state: RuntimeState,
    /// Currently active screen handler.
    handler: Box<dyn ScreenHandler>,
    /// Data directory used to open JN files on screen transitions.
    ///
    /// Used in child issues 4-6 when `apply_transition` constructs `MapScreen`
    /// and `LevelScreen`, which must load their respective JN files from disk.
    #[allow(dead_code)]
    data_dir: DataDirectory,
    /// Serialized world-map JN bytes preserved for save and restore operations.
    ///
    /// Populated from the outgoing handler's [`ScreenHandler::map_jn_bytes`]
    /// before each transition, mirroring `putCurrentLevelInFileMemory` from the
    /// Java reference implementation.
    map_jn_bytes: Option<Vec<u8>>,
    /// Serialized current-level JN bytes preserved for restart-level operations.
    ///
    /// Used in child issue 6 when `DIE_RESTART_LEVEL` handling needs to reload
    /// the level from its last saved-to-memory state.
    #[allow(dead_code)]
    level_jn_bytes: Option<Vec<u8>>,
    /// Render commands from the most recent game tick.
    ///
    /// Cached here for the event loop to re-present on vsync ticks that do not
    /// fire a game tick. Actual command execution via `execute_and_present` is
    /// wired in child issue 3.
    last_commands: Vec<RenderCommand>,
    /// Set to `true` when the active handler requests [`ScreenTransition::Quit`].
    quitting: bool,
}

impl GameOrchestrator {
    /// Constructs the orchestrator, loads assets, and boots [`StartMenuScreen`].
    ///
    /// Returns [`OrchestratorError`] if any required asset file is missing or
    /// fails to parse.
    pub fn new(data_dir: DataDirectory) -> Result<Self, OrchestratorError> {
        let cache = AssetCache::load(&data_dir)?;
        let handler: Box<dyn ScreenHandler> = Box::new(StartMenuScreen::new(
            cache.intro_jn.clone(),
            cache.dma.clone(),
            cache.vcl.clone(),
            cache.cfg.clone(),
        ));
        Ok(Self {
            cache,
            state: RuntimeState::new(),
            handler,
            data_dir,
            map_jn_bytes: None,
            level_jn_bytes: None,
            last_commands: Vec::new(),
            quitting: false,
        })
    }

    /// Advances the active screen handler by one fixed game tick.
    ///
    /// Applies any [`ScreenTransition`] returned by the handler, then prepends the static
    /// status bar commands before the handler's commands. The returned command list begins with
    /// the status bar so renderers always draw it first without each screen needing to emit it.
    pub fn tick(&mut self, input: &ActiveInput) -> Vec<RenderCommand> {
        let result = self.handler.tick(input, &mut self.state);
        if let Some(transition) = result.transition {
            self.apply_transition(transition);
        }
        let mut commands = crate::status_bar::status_bar_commands();
        commands.extend(result.commands);
        self.last_commands = commands.clone();
        commands
    }

    /// Returns `true` when the active handler has requested
    /// [`ScreenTransition::Quit`].
    pub fn is_quitting(&self) -> bool {
        self.quitting
    }

    /// Returns a reference to the pre-loaded asset cache.
    pub fn cache(&self) -> &AssetCache {
        &self.cache
    }

    /// Returns the render commands produced by the most recent game tick.
    pub fn last_commands(&self) -> &[RenderCommand] {
        &self.last_commands
    }

    /// Applies a screen transition returned by the active handler.
    ///
    /// Serializes the outgoing handler's JN bytes into [`map_jn_bytes`] before
    /// swapping, mirroring `putCurrentLevelInFileMemory` from the Java reference
    /// implementation. Then constructs and installs the next handler.
    ///
    /// [`map_jn_bytes`]: GameOrchestrator::map_jn_bytes
    fn apply_transition(&mut self, transition: ScreenTransition) {
        self.map_jn_bytes = self.handler.map_jn_bytes();

        match transition {
            ScreenTransition::StartMenu => {
                self.handler = Box::new(StartMenuScreen::new(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                    self.cache.vcl.clone(),
                    self.cache.cfg.clone(),
                ));
            }
            ScreenTransition::Story => {
                self.handler = Box::new(story_screen(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                ));
            }
            ScreenTransition::Credits => {
                self.handler = Box::new(credits_screen(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                ));
            }
            ScreenTransition::OrderingInfo => {
                self.handler = Box::new(ordering_info_screen(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                ));
            }
            ScreenTransition::Noisemaker => {
                self.handler = Box::new(noisemaker_screen(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                ));
            }
            ScreenTransition::Quit => {
                self.quitting = true;
            }
            // Map and Level transitions are implemented in later child issues.
            // Until then, fall back to StartMenuScreen so the loop stays valid.
            ScreenTransition::Map | ScreenTransition::Level { .. } | ScreenTransition::RestartLevel => {
                self.handler = Box::new(StartMenuScreen::new(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                    self.cache.vcl.clone(),
                    self.cache.cfg.clone(),
                ));
            }
        }
    }
}

/// Error returned when constructing [`GameOrchestrator`] fails.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    /// Asset loading from the data directory failed.
    #[error("failed to load game assets: {0}")]
    AssetLoad(#[from] AssetError),
}

#[cfg(test)]
mod tests {
    use super::GameOrchestrator;
    use crate::asset_cache::AssetCache;
    use openjill_core::runtime::RuntimeState;
    use openjill_core::{ActiveInput, RenderCommand, ScreenHandler, ScreenTransition, TickResult};
    use openjill_data::DataDirectory;
    use openjill_data::cfg::CfgFile;
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;
    use openjill_data::sha::ShaFile;
    use openjill_data::vcl::VclFile;

    /// Byte count for a minimal valid SHA header (128 u32 offsets + 128 u16 sizes).
    ///
    /// All zeros: every entry has offset=0 and size=0, so `is_valid()` returns
    /// false for all entries and no tilesets are parsed.
    const SHA_HEADER_BYTES: usize = 128 * 4 + 128 * 2;

    /// Byte count for a minimal valid VCL file.
    ///
    /// 400-byte sound-entry skip + 40 u32 text offsets + 40 u16 text lengths,
    /// all zeros: no text entries are parsed.
    const VCL_MIN_BYTES: usize = 400 + 40 * 4 + 40 * 2;

    /// Byte count for a minimal valid CFG file with all-zero fields.
    ///
    /// 10 high-score names (10 bytes each) + 20-byte hole + 10 i32 scores +
    /// 6 save names (12 bytes each) + setup/joystick/display/music/sound flags.
    const CFG_MIN_BYTES: usize = 10 * 10 + 20 + 10 * 4 + 6 * 12 + 2 + 2 + 6 * 2 + 2 + 2 + 2;

    /// Byte count for a minimal valid `JnFile` with all-zero background and save data.
    ///
    /// 128×64 background cells (u16 each) + 0-object count (u16) + save data block.
    const JN_MIN_BYTES: usize = 128 * 64 * 2 + 2 + 70;

    /// Creates a minimal valid [`AssetCache`] from synthetic zero-byte buffers.
    ///
    /// Used by orchestrator tests to avoid loading real game files.
    fn synthetic_cache() -> AssetCache {
        let dma = DmaFile::from_bytes(vec![]).expect("empty DMA should parse");
        let sha =
            ShaFile::from_bytes(vec![0u8; SHA_HEADER_BYTES]).expect("zero SHA header should parse");
        let vcl = VclFile::from_bytes(vec![0u8; VCL_MIN_BYTES]).expect("zero VCL should parse");
        let cfg =
            CfgFile::from_bytes(vec![0u8; CFG_MIN_BYTES], "JN1").expect("zero CFG should parse");
        let intro_jn =
            JnFile::from_bytes(vec![0u8; JN_MIN_BYTES]).expect("zero JN should parse");
        AssetCache { dma, sha, vcl, cfg, intro_jn }
    }

    /// Creates a [`GameOrchestrator`] from a synthetic cache and a custom handler.
    ///
    /// Bypasses `DataDirectory` asset loading so the orchestrator tick/transition
    /// logic can be tested without real game files.
    fn orchestrator_with_handler(handler: Box<dyn ScreenHandler>) -> GameOrchestrator {
        GameOrchestrator {
            cache: synthetic_cache(),
            state: RuntimeState::new(),
            handler,
            data_dir: DataDirectory::new(std::env::temp_dir()),
            map_jn_bytes: None,
            level_jn_bytes: None,
            last_commands: Vec::new(),
            quitting: false,
        }
    }

    /// Synthetic screen handler that returns a fixed [`ScreenTransition`] on the
    /// first tick and `None` on all subsequent ticks.
    struct OneShotTransitionHandler {
        /// Transition returned on the first tick; consumed via `take`.
        transition: Option<ScreenTransition>,
        /// Render command included in every tick result to distinguish this
        /// handler's output from the swapped-in handler's output.
        marker_command: RenderCommand,
    }

    impl OneShotTransitionHandler {
        /// Creates a handler that fires `transition` on first tick.
        fn new(transition: ScreenTransition) -> Self {
            Self {
                transition: Some(transition),
                marker_command: RenderCommand::Clear { color: 42 },
            }
        }
    }

    impl ScreenHandler for OneShotTransitionHandler {
        /// Returns a marker render command and fires the transition once.
        fn tick(&mut self, _input: &ActiveInput, _state: &mut RuntimeState) -> TickResult {
            TickResult {
                commands: vec![self.marker_command.clone()],
                transition: self.transition.take(),
                sound_events: Vec::new(),
            }
        }
    }

    /// Unit under test: `GameOrchestrator::tick` and `apply_transition`.
    ///
    /// Preconditions: a synthetic `OneShotTransitionHandler` that returns
    /// `ScreenTransition::StartMenu` on its first tick is installed in the
    /// orchestrator.
    ///
    /// Invariants asserted: the first tick includes the handler's marker render
    /// command alongside the prepended status bar commands; the second tick contains
    /// only the status bar commands (no marker), confirming that `apply_transition`
    /// replaced the handler with `StartMenuScreen`.
    #[test]
    fn tick_swaps_handler_on_screen_transition() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::StartMenu));
        let mut orchestrator = orchestrator_with_handler(handler);

        let input: ActiveInput = Default::default();

        let first = orchestrator.tick(&input);
        assert!(
            first.contains(&RenderCommand::Clear { color: 42 }),
            "marker command must appear in first tick alongside status bar commands"
        );
        assert!(!orchestrator.is_quitting());

        let second = orchestrator.tick(&input);
        assert!(
            !second.contains(&RenderCommand::Clear { color: 42 }),
            "marker command must not appear after handler swap to StartMenuScreen"
        );
        assert!(
            !second.is_empty(),
            "second tick must include status bar commands even after handler swap"
        );
    }

    /// Unit under test: `GameOrchestrator::tick` with `ScreenTransition::Quit`.
    ///
    /// Preconditions: a synthetic `OneShotTransitionHandler` that returns
    /// `ScreenTransition::Quit` on its first tick is installed.
    ///
    /// Invariants asserted: after the tick processes the quit transition,
    /// `is_quitting()` returns `true`.
    #[test]
    fn tick_sets_quitting_on_quit_transition() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::Quit));
        let mut orchestrator = orchestrator_with_handler(handler);

        let input: ActiveInput = Default::default();
        orchestrator.tick(&input);
        assert!(orchestrator.is_quitting());
    }
}
