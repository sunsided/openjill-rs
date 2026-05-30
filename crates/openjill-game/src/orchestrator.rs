//! Game orchestrator: owns the active screen handler and drives the tick and
//! transition loop.

use openjill_core::{
    ActiveInput, InventoryObject, MAP_LEVEL, MessageDispatcher, RenderCommand, RuntimeState,
    ScreenHandler, ScreenTransition,
};
use openjill_data::cfg::{CfgHighScore, CfgSaveSlot};
use openjill_data::episode::Episode;
use openjill_data::jn::{JnFile, JnReadError, JnSaveData};
use openjill_data::{DataDirectory, DataDirectoryError};
use thiserror::Error;

use crate::asset_cache::{AssetCache, AssetError};
use crate::saves::{SaveStore, SaveStoreError};
use crate::screens::intro_screens::{
    credits_screen, noisemaker_screen, ordering_info_screen, story_screen,
};
use crate::screens::level_screen::{EPISODE_1_SKY_COLOR, LevelScreen};
use crate::screens::start_menu::StartMenuScreen;

/// Constructs a fresh [`StartMenuScreen`] from the given asset cache.
fn make_start_menu(cache: &AssetCache) -> StartMenuScreen {
    StartMenuScreen::new(
        cache.intro_jn.clone(),
        cache.dma.clone(),
        cache.vcl.clone(),
        cache.cfg.clone(),
        &cache.sha,
    )
}

/// Loads the active episode's world-map JN file ([`Episode::map_jn`]) from
/// `data_dir` and constructs a [`LevelScreen`] with [`MAP_LEVEL`] as the level
/// number.
///
/// Using `LevelScreen` for the map matches the Java reference `MapLevelHandler`
/// which inherits `AbstractExecutingStdLevel` - the same entity update loop as
/// regular levels, giving the map full player physics and input.
fn load_map_screen(
    data_dir: &DataDirectory,
    episode: &Episode,
    cache: &AssetCache,
    dispatcher: &mut MessageDispatcher,
) -> Result<LevelScreen, LevelLoadError> {
    let map_file = episode.map_jn();
    let path = data_dir
        .resolve_path_case_insensitive(&map_file)
        .map_err(LevelLoadError::Resolve)?;
    let bytes = std::fs::read(&path).map_err(LevelLoadError::Read)?;
    LevelScreen::from_bytes(bytes, cache, MAP_LEVEL, dispatcher, EPISODE_1_SKY_COLOR)
        .map_err(LevelLoadError::Parse)
}

/// Loads a level JN file from `data_dir` and constructs a [`LevelScreen`].
///
/// Used by [`GameOrchestrator::apply_transition`] on
/// [`ScreenTransition::Level`] when no in-memory bytes are available for the
/// requested level file.  `cache` supplies the shared episode assets (DMA,
/// SHA) consumed by the object and background entity factories.
fn load_level_screen(
    data_dir: &DataDirectory,
    cache: &AssetCache,
    file: &str,
    level_number: i32,
    dispatcher: &mut MessageDispatcher,
) -> Result<LevelScreen, LevelLoadError> {
    let path = data_dir
        .resolve_path_case_insensitive(file)
        .map_err(LevelLoadError::Resolve)?;
    let bytes = std::fs::read(&path).map_err(LevelLoadError::Read)?;
    LevelScreen::from_bytes(bytes, cache, level_number, dispatcher, EPISODE_1_SKY_COLOR)
        .map_err(LevelLoadError::Parse)
}

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
    /// Active episode descriptor used to address per-episode JN filenames
    /// during screen transitions.
    episode: &'static Episode,
    /// Serialized world-map JN bytes preserved for save and restore operations.
    ///
    /// Populated from the outgoing handler's [`ScreenHandler::map_jn_bytes`]
    /// before each transition, mirroring `putCurrentLevelInFileMemory` from the
    /// Java reference implementation.
    map_jn_bytes: Option<Vec<u8>>,
    /// Serialized current-level JN bytes preserved for restart-level operations.
    ///
    /// Populated from the outgoing handler's
    /// [`ScreenHandler::level_jn_bytes`] whenever a level screen is replaced.
    /// Used to satisfy `ScreenTransition::RestartLevel` and to fall back to
    /// in-memory bytes for the previously visited level.
    level_jn_bytes: Option<Vec<u8>>,
    /// File name of the level currently held in [`level_jn_bytes`], when one
    /// is cached.  Used so [`apply_transition`] can decide whether the
    /// preserved bytes can be reused for an incoming
    /// [`ScreenTransition::Level`].
    ///
    /// [`apply_transition`]: GameOrchestrator::apply_transition
    /// [`level_jn_bytes`]: GameOrchestrator::level_jn_bytes
    level_jn_file: Option<String>,
    /// Level number associated with [`level_jn_bytes`], when one is cached.
    ///
    /// [`level_jn_bytes`]: GameOrchestrator::level_jn_bytes
    level_jn_number: Option<i32>,
    /// Shared message dispatcher used by level screens to receive
    /// `CheckpointChangeLevel`, `CheckpointChangeLevelPrevious`, and
    /// `DieRestartLevel` messages from gameplay subsystems.
    ///
    /// The orchestrator clears the dispatcher before constructing a new
    /// [`LevelScreen`] so stale handlers from a previous level do not survive
    /// the transition.
    dispatcher: MessageDispatcher,
    /// Render commands from the most recent game tick.
    ///
    /// Cached here for the event loop to re-present on vsync ticks that do not
    /// fire a game tick. Actual command execution via `execute_and_present` is
    /// wired in child issue 3.
    last_commands: Vec<RenderCommand>,
    /// Snapshot of [`RuntimeState`] taken the moment a new level is entered.
    ///
    /// Used by `RestartLevel` to restore health and inventory to their
    /// level-entry values while preserving accumulated score.  Mirrors the
    /// Java reference behavior where dying resets inventory to the pre-level
    /// state rather than the game-start state.
    level_entry_state: Option<RuntimeState>,
    /// Set to `true` when the active handler requests [`ScreenTransition::Quit`].
    quitting: bool,
    /// Writable runtime storage for save games, high scores, and the working
    /// config copy.  Seeded from the original (read-only) `JILL1.CFG` on first
    /// launch; the save/restore and high-score menu layer drives this.
    saves: SaveStore,
}

impl GameOrchestrator {
    /// Constructs the orchestrator, loads assets, and boots [`StartMenuScreen`]
    /// for `episode`.
    ///
    /// Returns [`OrchestratorError`] if any required asset file is missing or
    /// fails to parse.
    pub fn new(
        data_dir: DataDirectory,
        episode: &'static Episode,
    ) -> Result<Self, OrchestratorError> {
        let cache = AssetCache::load(&data_dir, episode)?;
        // Open the writable runtime store before moving `data_dir` into `Self`;
        // this seeds the per-user config copy from the original on first launch.
        let saves = SaveStore::open(&data_dir, episode)?;
        let handler: Box<dyn ScreenHandler> = Box::new(make_start_menu(&cache));
        Ok(Self {
            cache,
            state: RuntimeState::new(),
            handler,
            data_dir,
            episode,
            map_jn_bytes: None,
            level_jn_bytes: None,
            level_jn_file: None,
            level_jn_number: None,
            dispatcher: MessageDispatcher::new(),
            last_commands: Vec::new(),
            level_entry_state: None,
            quitting: false,
            saves,
        })
    }

    /// Returns a mutable reference to the orchestrator's shared message
    /// dispatcher.
    ///
    /// Gameplay subsystems publish level-transition messages
    /// (`CheckpointChangeLevel`, `CheckpointChangeLevelPrevious`,
    /// `DieRestartLevel`) here; an active [`LevelScreen`] is registered as a
    /// subscriber.  Tests use this accessor to dispatch synthetic messages.
    pub fn dispatcher_mut(&mut self) -> &mut MessageDispatcher {
        &mut self.dispatcher
    }

    /// Applies `transition` immediately, bypassing the message-dispatcher
    /// pipeline.
    ///
    /// Intended for developer tooling (e.g. the debug `L` key) that needs to
    /// drop the orchestrator into a specific screen without standing up a full
    /// gameplay loop first.
    pub fn force_transition(&mut self, transition: ScreenTransition) {
        self.apply_transition(transition);
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

    /// Returns the current high-score table from the runtime config.
    pub fn high_scores(&self) -> &[CfgHighScore] {
        self.saves.high_scores()
    }

    /// Returns the current save slots from the runtime config.
    pub fn save_slots(&self) -> &[CfgSaveSlot] {
        self.saves.save_slots()
    }

    /// Records a new high score (sorted, capped) and persists the config.
    pub fn record_high_score(&mut self, name: &str, score: i32) -> Result<(), SaveStoreError> {
        self.saves.record_high_score(name, score)
    }

    /// Snapshots the current game into `slot` under `name`.
    ///
    /// Captures a **live** JN snapshot of the active screen
    /// ([`ScreenHandler::snapshot_jn_bytes`]) - the current background, object
    /// entities, and runtime save-data - and pairs it with the orchestrator's
    /// cached bytes for the half the active screen does not own (the
    /// last-visited map when saving inside a level, or the last level when
    /// saving on the map). Returns [`SaveError::NothingToSave`] when no level
    /// has been entered yet (e.g. from the start menu).
    pub fn save_to_slot(&mut self, slot: usize, name: &str) -> Result<(), SaveError> {
        // A save requires an active game screen producing a live snapshot;
        // the start menu / intro screens return `None`.
        let Some(live) = self.handler.snapshot_jn_bytes(&self.state) else {
            return Err(SaveError::NothingToSave);
        };
        // The active screen owns exactly one half (map or level); the live
        // snapshot fills that half and the cached bytes fill the other. The
        // `is_world_map()` check is cheap (no buffer clone).
        let (map, level): (Option<&[u8]>, Option<&[u8]>) = if self.handler.is_world_map() {
            (Some(live.as_slice()), self.level_jn_bytes.as_deref())
        } else {
            (self.map_jn_bytes.as_deref(), Some(live.as_slice()))
        };
        let (Some(map), Some(level)) = (map, level) else {
            return Err(SaveError::NothingToSave);
        };
        self.saves.save_game(slot, name, map, level)?;
        Ok(())
    }

    /// Restores a saved game from `slot`, replacing the active screen and
    /// seeding [`RuntimeState`] from the saved snapshot's save-data block.
    ///
    /// Reads back the `(map, level)` JN snapshots, caches the map bytes for the
    /// next `Map` transition, and reconstructs the saved screen: the world map
    /// when the saved level number is [`MAP_LEVEL`], otherwise the level.
    /// Health / score / inventory are restored from the reconstructed screen's
    /// save-data block; `lives` is not in the DOS save block and is carried
    /// as-is.
    ///
    /// The restore is **transactional**: every fallible step (parsing the
    /// snapshots, constructing the replacement screen into a fresh dispatcher)
    /// runs into locals before any field of `self` is mutated, so a corrupt or
    /// incomplete save leaves the current game fully intact.
    pub fn restore_from_slot(&mut self, slot: usize) -> Result<(), SaveError> {
        let (map_bytes, level_bytes) = self.saves.load_game(slot)?;
        let level_jn = JnFile::from_bytes(level_bytes.clone()).map_err(SaveError::Parse)?;
        let level_number = i32::from(level_jn.save_data().level() as i16);

        // Build the replacement screen into a fresh dispatcher and recover the
        // runtime state, all in locals; nothing on `self` is touched yet.
        let mut new_dispatcher = MessageDispatcher::new();
        if level_number == MAP_LEVEL {
            // Saved on the world map: reconstruct the map; its (live) save-data
            // block carries the runtime state.
            let map_jn = JnFile::from_bytes(map_bytes.clone()).map_err(SaveError::Parse)?;
            let restored_state = self.runtime_state_from(map_jn.save_data());
            let screen = LevelScreen::from_bytes(
                map_bytes.clone(),
                &self.cache,
                MAP_LEVEL,
                &mut new_dispatcher,
                EPISODE_1_SKY_COLOR,
            )
            .map_err(SaveError::Parse)?;

            // Commit (infallible): swap in the new dispatcher, state, and screen.
            self.dispatcher = new_dispatcher;
            self.state = restored_state;
            self.map_jn_bytes = Some(map_bytes);
            // No level context for a map restore (the player re-enters a level
            // from the map, which loads it fresh).
            self.level_jn_bytes = None;
            self.level_jn_file = None;
            self.level_jn_number = None;
            self.level_entry_state = Some(self.state.clone());
            self.handler = Box::new(screen);
        } else {
            let restored_state = self.runtime_state_from(level_jn.save_data());
            let screen = LevelScreen::from_bytes(
                level_bytes,
                &self.cache,
                level_number,
                &mut new_dispatcher,
                EPISODE_1_SKY_COLOR,
            )
            .map_err(SaveError::Parse)?;
            let level_jn_bytes = screen.level_jn_bytes();

            // Commit (infallible).
            self.dispatcher = new_dispatcher;
            self.state = restored_state;
            self.map_jn_bytes = Some(map_bytes);
            self.level_jn_bytes = level_jn_bytes;
            self.level_jn_file = Some(self.episode.level_jn(level_number));
            self.level_jn_number = Some(level_number);
            self.level_entry_state = Some(self.state.clone());
            self.handler = Box::new(screen);
        }
        Ok(())
    }

    /// Builds a fresh [`RuntimeState`] from a saved JN save-data block,
    /// carrying the current `lives` (which the DOS save block omits).
    fn runtime_state_from(&self, save_data: &JnSaveData) -> RuntimeState {
        let mut state = self.state.clone();
        state.level = i32::from(save_data.level() as i16);
        state.health = i32::from(save_data.health());
        state.score = save_data.score() as i32;
        state.inventory = save_data
            .inventory()
            .iter()
            .filter_map(|&code| InventoryObject::from_index(code))
            .collect();
        state
    }

    /// Applies a screen transition returned by the active handler.
    ///
    /// When the outgoing handler owns map JN bytes, captures them into
    /// [`map_jn_bytes`] before swapping (mirroring `putCurrentLevelInFileMemory`
    /// from the Java reference). Handlers that do not own a JN file return
    /// `None`, and the stored bytes are left intact so map state survives
    /// trips through start-menu and intro/credits screens.
    ///
    /// [`map_jn_bytes`]: GameOrchestrator::map_jn_bytes
    fn apply_transition(&mut self, transition: ScreenTransition) {
        if let Some(bytes) = self.handler.map_jn_bytes() {
            self.map_jn_bytes = Some(bytes);
        }
        if let Some(bytes) = self.handler.level_jn_bytes() {
            self.level_jn_bytes = Some(bytes);
        }

        match transition {
            ScreenTransition::StartMenu => {
                self.dispatcher.clear();
                self.handler = Box::new(make_start_menu(&self.cache));
            }
            ScreenTransition::Story => {
                self.dispatcher.clear();
                self.handler = Box::new(story_screen(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                ));
            }
            ScreenTransition::Credits => {
                self.dispatcher.clear();
                self.handler = Box::new(credits_screen(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                ));
            }
            ScreenTransition::OrderingInfo => {
                self.dispatcher.clear();
                self.handler = Box::new(ordering_info_screen(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                ));
            }
            ScreenTransition::Noisemaker => {
                self.dispatcher.clear();
                self.handler = Box::new(noisemaker_screen(
                    self.cache.intro_jn.clone(),
                    self.cache.dma.clone(),
                ));
            }
            ScreenTransition::Quit => {
                self.quitting = true;
            }
            ScreenTransition::PerformSave { slot, name } => {
                // Snapshot the still-active screen into the slot; the handler is
                // intentionally not swapped so play resumes on the next tick.
                if let Err(err) = self.save_to_slot(slot, &name) {
                    eprintln!("openjill-game: save to slot {slot} failed: {err}");
                }
            }
            ScreenTransition::PerformLoad { slot } => {
                // `restore_from_slot` reconstructs the saved screen on success;
                // on failure the current screen is left untouched.
                if let Err(err) = self.restore_from_slot(slot) {
                    eprintln!("openjill-game: restore from slot {slot} failed: {err}");
                }
            }
            ScreenTransition::Map => {
                self.dispatcher.clear();
                let map_file = self.episode.map_jn();
                // Prefer the in-memory map JN bytes captured from a previous
                // visit; only reach to disk on the first transition to Map.
                // LevelScreen with MAP_LEVEL gives the map the full entity
                // update loop (player physics, input, viewport scrolling),
                // matching the Java reference MapLevelHandler.
                let map_result = match self.map_jn_bytes.clone() {
                    Some(bytes) => LevelScreen::from_bytes(
                        bytes,
                        &self.cache,
                        MAP_LEVEL,
                        &mut self.dispatcher,
                        EPISODE_1_SKY_COLOR,
                    )
                    .map_err(LevelLoadError::Parse),
                    None => load_map_screen(
                        &self.data_dir,
                        self.episode,
                        &self.cache,
                        &mut self.dispatcher,
                    ),
                };
                match map_result {
                    Ok(screen) => {
                        self.handler = Box::new(screen);
                    }
                    Err(err) => {
                        eprintln!(
                            "openjill-game: failed to load {map_file} ({err}); falling back to start menu"
                        );
                        self.handler = Box::new(make_start_menu(&self.cache));
                    }
                }
            }
            ScreenTransition::Level { file, number } => {
                self.dispatcher.clear();
                // Filenames are compared with ASCII case-insensitivity to
                // match `DataDirectory::resolve_path_case_insensitive`; a
                // transition payload using different casing than the cached
                // entry should still hit the in-memory bytes.
                let cached = self
                    .level_jn_bytes
                    .as_ref()
                    .zip(self.level_jn_file.as_ref())
                    .filter(|(_, cached_file)| cached_file.eq_ignore_ascii_case(file.as_str()))
                    .map(|(bytes, _)| bytes.clone());
                let level_result = match cached {
                    Some(bytes) => LevelScreen::from_bytes(
                        bytes,
                        &self.cache,
                        number,
                        &mut self.dispatcher,
                        EPISODE_1_SKY_COLOR,
                    )
                    .map_err(LevelLoadError::Parse),
                    None => load_level_screen(
                        &self.data_dir,
                        &self.cache,
                        &file,
                        number,
                        &mut self.dispatcher,
                    ),
                };
                match level_result {
                    Ok(screen) => {
                        self.level_jn_bytes = screen.level_jn_bytes();
                        self.level_jn_file = Some(file);
                        self.level_jn_number = Some(number);
                        self.level_entry_state = Some(self.state.clone());
                        self.handler = Box::new(screen);
                    }
                    Err(err) => {
                        eprintln!(
                            "openjill-game: failed to load level {file} ({err}); falling back to start menu"
                        );
                        self.handler = Box::new(make_start_menu(&self.cache));
                    }
                }
            }
            ScreenTransition::RestartLevel => {
                self.dispatcher.clear();
                let Some(bytes) = self.level_jn_bytes.clone() else {
                    eprintln!(
                        "openjill-game: RestartLevel requested without cached level bytes; falling back to start menu"
                    );
                    self.handler = Box::new(make_start_menu(&self.cache));
                    return;
                };
                let level_number = self.level_jn_number.unwrap_or(0);
                match LevelScreen::from_bytes(
                    bytes,
                    &self.cache,
                    level_number,
                    &mut self.dispatcher,
                    EPISODE_1_SKY_COLOR,
                ) {
                    Ok(screen) => {
                        if let Some(entry) = &self.level_entry_state {
                            self.state.lives = self.state.lives.saturating_sub(1);
                            self.state.health = entry.health;
                            self.state.inventory = entry.inventory.clone();
                        }
                        self.handler = Box::new(screen);
                    }
                    Err(err) => {
                        eprintln!(
                            "openjill-game: failed to restart level ({err}); falling back to start menu"
                        );
                        self.handler = Box::new(make_start_menu(&self.cache));
                    }
                }
            }
        }

        // Refresh the (possibly new) handler's save-slot labels: a transition
        // may have built a new screen, and a `PerformSave` changed the names.
        self.push_save_slot_names();
    }

    /// Pushes the current CFG save-slot names into the active handler so an
    /// in-screen save / load menu can label its slots.
    fn push_save_slot_names(&mut self) {
        let names: Vec<String> = self
            .saves
            .save_slots()
            .iter()
            .map(|slot| slot.name().to_string())
            .collect();
        self.handler.set_save_slot_names(names);
    }
}

/// Error returned when constructing [`GameOrchestrator`] fails.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    /// Asset loading from the data directory failed.
    #[error("failed to load game assets: {0}")]
    AssetLoad(#[from] AssetError),
    /// Opening the writable runtime store (config seed) failed.
    #[error("failed to open save store: {0}")]
    SaveStore(#[from] SaveStoreError),
}

/// Error returned when saving the current game to a slot fails.
#[derive(Debug, Error)]
pub enum SaveError {
    /// A save-store operation (slot bounds, I/O, config) failed.
    #[error(transparent)]
    Store(#[from] SaveStoreError),
    /// No level has been entered yet, so there is nothing to snapshot.
    #[error("no game in progress to save")]
    NothingToSave,
    /// A saved JN snapshot could not be parsed during a restore.
    #[error("failed to parse saved level: {0}")]
    Parse(#[source] JnReadError),
}

/// Error returned when loading a map or level JN file during a screen transition fails.
#[derive(Debug, Error)]
enum LevelLoadError {
    /// Case-insensitive lookup of the level file failed.
    #[error("failed to resolve level file: {0}")]
    Resolve(#[source] DataDirectoryError),
    /// Reading the resolved level bytes from disk failed.
    #[error("failed to read level file: {0}")]
    Read(#[source] std::io::Error),
    /// Parsing the level bytes into a `JnFile` failed.
    #[error("failed to parse level file: {0}")]
    Parse(#[source] JnReadError),
}

#[cfg(test)]
mod tests {
    use super::{GameOrchestrator, SaveError};
    use crate::asset_cache::AssetCache;
    use crate::saves::SaveStore;
    use openjill_core::runtime::RuntimeState;
    use openjill_core::{
        ActiveInput, InventoryObject, RenderCommand, ScreenHandler, ScreenTransition, TickResult,
    };
    use openjill_data::DataDirectory;
    use openjill_data::cfg::CfgFile;
    use openjill_data::dma::DmaFile;
    use openjill_data::episode;
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
        let cfg = CfgFile::from_bytes(vec![0u8; CFG_MIN_BYTES], episode::JILL1.jn_ext)
            .expect("zero CFG should parse");
        let intro_jn = JnFile::from_bytes(vec![0u8; JN_MIN_BYTES]).expect("zero JN should parse");
        AssetCache {
            dma,
            sha,
            vcl,
            cfg,
            intro_jn,
        }
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
            episode: &episode::JILL1,
            map_jn_bytes: None,
            level_jn_bytes: None,
            level_jn_file: None,
            level_jn_number: None,
            dispatcher: openjill_core::MessageDispatcher::new(),
            last_commands: Vec::new(),
            level_entry_state: None,
            quitting: false,
            saves: test_save_store(),
        }
    }

    /// Builds a [`SaveStore`] rooted at a unique temp directory with no original
    /// config (defaults), so orchestrator tests exercise save/high-score paths
    /// without touching the per-user data directory.
    fn test_save_store() -> SaveStore {
        use crate::saves::RuntimeDir;
        use std::sync::Once;
        use std::time::{SystemTime, UNIX_EPOCH};

        // All per-test runtime dirs live under one base that is wiped once at
        // the start of each test-process run, so temp state never accumulates
        // across runs while parallel tests still get isolated unique subdirs.
        static CLEAN_BASE: Once = Once::new();
        let base = std::env::temp_dir().join("openjill-orch-tests");
        CLEAN_BASE.call_once(|| {
            let _ = std::fs::remove_dir_all(&base);
        });
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!("{}-{nanos}", std::process::id()));
        SaveStore::open_with_runtime(
            RuntimeDir::with_root(root),
            &DataDirectory::new(std::env::temp_dir()),
            &episode::JILL1,
        )
        .expect("open save store with defaults")
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

    /// Unit under test: `apply_transition` on `ScreenTransition::Map` uses
    /// preserved bytes from [`GameOrchestrator::map_jn_bytes`] instead of
    /// reaching to disk, mirroring `putCurrentLevelInFileMemory` semantics.
    ///
    /// Preconditions: the orchestrator is seeded with synthetic zero MAP.JN1
    /// bytes in `self.map_jn_bytes`; its `data_dir` points at a temp directory
    /// that does **not** contain `MAP.JN1`.  A `OneShotTransitionHandler`
    /// returns `ScreenTransition::Map` on its first tick.
    ///
    /// Invariants asserted: after the tick, the new handler is a `MapScreen`
    /// (its `map_jn_bytes` returns the same bytes the orchestrator was seeded
    /// with).  Reaching to disk would fail because the temp dir has no
    /// `MAP.JN1`, so the only way this assertion passes is via the in-memory
    /// path.
    #[test]
    fn map_transition_reuses_preserved_bytes_without_disk_read() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::Map));
        let mut orchestrator = orchestrator_with_handler(handler);
        let synthetic_map = vec![0u8; JN_MIN_BYTES];
        orchestrator.map_jn_bytes = Some(synthetic_map.clone());

        let input: ActiveInput = Default::default();
        orchestrator.tick(&input);

        assert_eq!(
            orchestrator.handler.map_jn_bytes(),
            Some(synthetic_map),
            "MapScreen must be constructed from preserved bytes, not the (empty) disk dir"
        );
    }

    /// Unit under test: `apply_transition` does not overwrite preserved
    /// [`GameOrchestrator::map_jn_bytes`] with `None` when the outgoing
    /// handler does not own a JN file.
    ///
    /// Preconditions: the orchestrator is seeded with synthetic map bytes and
    /// runs a `OneShotTransitionHandler` (returns `None` from
    /// `map_jn_bytes`) that transitions to `StartMenu`.
    ///
    /// Invariants asserted: `self.map_jn_bytes` is still `Some(synthetic)`
    /// after the transition.
    #[test]
    fn transition_does_not_drop_preserved_bytes_for_jn_less_handlers() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::StartMenu));
        let mut orchestrator = orchestrator_with_handler(handler);
        let synthetic_map = vec![0u8; JN_MIN_BYTES];
        orchestrator.map_jn_bytes = Some(synthetic_map.clone());

        let input: ActiveInput = Default::default();
        orchestrator.tick(&input);

        assert_eq!(orchestrator.map_jn_bytes, Some(synthetic_map));
    }

    /// Test helper: seeds the orchestrator with an in-memory level file so a
    /// subsequent `ScreenTransition::Level` reuses the bytes instead of
    /// reaching to disk.
    fn seed_level_cache(orchestrator: &mut GameOrchestrator, file: &str, number: i32) {
        let bytes = vec![0u8; JN_MIN_BYTES];
        orchestrator.level_jn_bytes = Some(bytes);
        orchestrator.level_jn_file = Some(file.to_string());
        orchestrator.level_jn_number = Some(number);
    }

    /// Drives the orchestrator past the level message-box countdown so any
    /// pending transition fires.
    fn drive_message_box_countdown(orchestrator: &mut GameOrchestrator) {
        let input: ActiveInput = Default::default();
        for _ in 0..openjill_core::layout::LEVEL_MESSAGE_TICKS {
            orchestrator.tick(&input);
        }
    }

    /// Unit under test: `apply_transition` on `ScreenTransition::Level` uses
    /// preserved bytes from [`GameOrchestrator::level_jn_bytes`] and registers
    /// the resulting [`LevelScreen`] as the active handler.
    ///
    /// Preconditions: the orchestrator is seeded with synthetic level bytes
    /// for `"JN1L01.JN1"`; a `OneShotTransitionHandler` returns
    /// `ScreenTransition::Level { file: "JN1L01.JN1", number: 1 }` on its
    /// first tick.
    ///
    /// Invariants asserted: the transition payload uses lower-case
    /// `"jn1l01.jn1"` while the orchestrator cache holds the upper-case
    /// `"JN1L01.JN1"`; after the tick, the active handler still reports the
    /// seeded level bytes, confirming the cache match is ASCII
    /// case-insensitive (matching `resolve_path_case_insensitive`).
    #[test]
    fn level_transition_cache_match_is_case_insensitive() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::Level {
            file: String::from("jn1l01.jn1"),
            number: 1,
        }));
        let mut orchestrator = orchestrator_with_handler(handler);
        seed_level_cache(&mut orchestrator, "JN1L01.JN1", 1);
        let expected_bytes = orchestrator.level_jn_bytes.clone();

        let input: ActiveInput = Default::default();
        orchestrator.tick(&input);

        assert_eq!(
            orchestrator.handler.level_jn_bytes(),
            expected_bytes,
            "case-insensitive cache match must reuse seeded bytes without touching disk"
        );
    }

    /// Unit under test: [`GameOrchestrator::save_to_slot`] +
    /// [`GameOrchestrator::restore_from_slot`].
    ///
    /// Preconditions: the orchestrator enters a level (so the active handler is
    /// a real `LevelScreen`); the runtime state then carries a health, score,
    /// and inventory.
    ///
    /// Invariants asserted: after saving, clobbering the state, and restoring,
    /// the runtime state (health/score/inventory/level) is recovered from the
    /// saved level snapshot and the active level is reconstructed.
    #[test]
    fn save_then_restore_round_trips_runtime_state() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::Level {
            file: String::from("1.JN1"),
            number: 1,
        }));
        let mut orchestrator = orchestrator_with_handler(handler);
        seed_level_cache(&mut orchestrator, "1.JN1", 1);
        // A cached map snapshot stands in for the SAVEM half.
        orchestrator.map_jn_bytes = Some(vec![0u8; JN_MIN_BYTES]);

        // Enter the level so the active handler is a `LevelScreen`.
        orchestrator.tick(&ActiveInput::default());

        orchestrator.state.health = 4;
        orchestrator.state.score = 1234;
        orchestrator.state.inventory = vec![InventoryObject::Jill, InventoryObject::Gem];
        orchestrator.save_to_slot(0, "HERO").expect("save succeeds");

        // Clobber the live state, then restore it from the slot.
        orchestrator.state.health = 99;
        orchestrator.state.score = 0;
        orchestrator.state.inventory.clear();
        orchestrator.restore_from_slot(0).expect("restore succeeds");

        assert_eq!(orchestrator.state.health, 4);
        assert_eq!(orchestrator.state.score, 1234);
        assert_eq!(
            orchestrator.state.inventory,
            vec![InventoryObject::Jill, InventoryObject::Gem]
        );
        assert_eq!(orchestrator.state.level, 1);
        assert_eq!(orchestrator.level_jn_number, Some(1));
    }

    /// Unit under test: [`GameOrchestrator::restore_from_slot`] is transactional.
    ///
    /// Preconditions: a slot holds a save whose level bytes do not parse as a
    /// JN file; the orchestrator carries some runtime state.
    ///
    /// Invariant asserted: the restore returns [`SaveError::Parse`] and leaves
    /// the runtime state untouched (no partial mutation of `self`).
    #[test]
    fn restore_from_corrupt_slot_leaves_game_intact() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::StartMenu));
        let mut orchestrator = orchestrator_with_handler(handler);
        orchestrator.state.health = 7;
        orchestrator.state.score = 555;
        orchestrator
            .saves
            .save_game(1, "BAD", b"not a jn map", b"not a jn level")
            .expect("writing a (garbage) save succeeds");

        let result = orchestrator.restore_from_slot(1);

        assert!(matches!(result, Err(SaveError::Parse(_))));
        assert_eq!(orchestrator.state.health, 7, "state must be untouched");
        assert_eq!(orchestrator.state.score, 555, "state must be untouched");
    }

    /// Invariants asserted: after the tick, the active handler's
    /// `level_jn_bytes` matches the seeded bytes, confirming the in-memory
    /// path was taken without touching disk.
    #[test]
    fn level_transition_reuses_preserved_bytes_without_disk_read() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::Level {
            file: String::from("JN1L01.JN1"),
            number: 1,
        }));
        let mut orchestrator = orchestrator_with_handler(handler);
        seed_level_cache(&mut orchestrator, "JN1L01.JN1", 1);
        let expected_bytes = orchestrator.level_jn_bytes.clone();

        let input: ActiveInput = Default::default();
        orchestrator.tick(&input);

        assert_eq!(
            orchestrator.handler.level_jn_bytes(),
            expected_bytes,
            "LevelScreen must be constructed from preserved bytes, not the (empty) disk dir"
        );
    }

    /// Unit under test: `MessageDispatcher::send(CheckpointChangeLevel, …)`
    /// triggers a [`LevelScreen`] swap inside the orchestrator after exactly
    /// `LEVEL_MESSAGE_TICKS` further ticks.
    ///
    /// Preconditions: orchestrator runs a `LevelScreen` for `"JN1L01.JN1"`
    /// (level 1); a second synthetic level file `"JN1L02.JN1"` is preloaded
    /// into the level byte cache so the level transition can avoid disk.
    ///
    /// Invariants asserted: the active handler reports level number 2 after
    /// the countdown.
    #[test]
    fn dispatcher_change_level_transitions_after_message_ticks() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::Level {
            file: String::from("JN1L01.JN1"),
            number: 1,
        }));
        let mut orchestrator = orchestrator_with_handler(handler);
        seed_level_cache(&mut orchestrator, "JN1L01.JN1", 1);

        let input: ActiveInput = Default::default();
        orchestrator.tick(&input);

        // Replace cached bytes with a second synthetic level so the
        // dispatched transition can resolve in-memory.
        let level_two_bytes = vec![0u8; JN_MIN_BYTES];
        orchestrator.level_jn_bytes = Some(level_two_bytes);
        orchestrator.level_jn_file = Some(String::from("JN1L02.JN1"));
        orchestrator.level_jn_number = Some(2);

        orchestrator.dispatcher_mut().send(
            openjill_core::MessageType::CheckpointChangeLevel,
            openjill_core::MessagePayload::ChangeLevel(openjill_core::ChangeLevelPayload {
                level_file: String::from("JN1L02.JN1"),
                level_number: 2,
            }),
        );

        drive_message_box_countdown(&mut orchestrator);

        assert_eq!(orchestrator.level_jn_number, Some(2));
    }

    /// Unit under test: `MessageDispatcher::send(CheckpointChangeLevelPrevious, …)`
    /// swaps the active `LevelScreen` back to a `MapScreen` after the level
    /// message-box countdown.
    ///
    /// Preconditions: orchestrator runs a `LevelScreen` for `"JN1L01.JN1"`;
    /// the world-map byte cache is seeded with synthetic MAP.JN1 bytes.
    ///
    /// Invariants asserted: after the countdown, the active handler's
    /// `map_jn_bytes` reports the seeded MAP.JN1 bytes, confirming a swap to
    /// `MapScreen`.
    #[test]
    fn dispatcher_previous_returns_to_map_screen() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::Level {
            file: String::from("JN1L01.JN1"),
            number: 1,
        }));
        let mut orchestrator = orchestrator_with_handler(handler);
        seed_level_cache(&mut orchestrator, "JN1L01.JN1", 1);
        let synthetic_map = vec![0u8; JN_MIN_BYTES];
        orchestrator.map_jn_bytes = Some(synthetic_map.clone());

        let input: ActiveInput = Default::default();
        orchestrator.tick(&input);

        orchestrator.dispatcher_mut().send(
            openjill_core::MessageType::CheckpointChangeLevelPrevious,
            openjill_core::MessagePayload::None,
        );

        drive_message_box_countdown(&mut orchestrator);

        assert_eq!(orchestrator.handler.map_jn_bytes(), Some(synthetic_map));
    }

    /// Unit under test: `MessageDispatcher::send(DieRestartLevel, …)` builds
    /// a fresh `LevelScreen` from the in-memory level bytes, leaving the
    /// cached `level_jn_file` and `level_jn_number` unchanged.
    ///
    /// Preconditions: orchestrator runs a `LevelScreen` for `"JN1L01.JN1"`;
    /// the level byte cache holds the bytes used to construct it.
    ///
    /// Invariants asserted: after the countdown, the cached level number is
    /// unchanged (still `1`) and the active handler's `level_jn_bytes`
    /// matches the cached bytes, confirming a restart-from-memory path.
    #[test]
    fn dispatcher_die_restart_reloads_level_from_memory() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::Level {
            file: String::from("JN1L01.JN1"),
            number: 1,
        }));
        let mut orchestrator = orchestrator_with_handler(handler);
        seed_level_cache(&mut orchestrator, "JN1L01.JN1", 1);
        let cached_bytes = orchestrator.level_jn_bytes.clone();

        let input: ActiveInput = Default::default();
        orchestrator.tick(&input);

        orchestrator.dispatcher_mut().send(
            openjill_core::MessageType::DieRestartLevel,
            openjill_core::MessagePayload::None,
        );

        drive_message_box_countdown(&mut orchestrator);

        assert_eq!(orchestrator.level_jn_number, Some(1));
        assert_eq!(orchestrator.handler.level_jn_bytes(), cached_bytes);
    }

    /// Unit under test: [`GameOrchestrator::save_to_slot`] reports
    /// [`SaveError::NothingToSave`] before any level has been entered.
    ///
    /// Preconditions: a fresh orchestrator on the start menu, with no cached
    /// map or level JN bytes.
    #[test]
    fn save_to_slot_without_a_level_reports_nothing_to_save() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::StartMenu));
        let mut orchestrator = orchestrator_with_handler(handler);

        assert!(matches!(
            orchestrator.save_to_slot(0, "ME"),
            Err(SaveError::NothingToSave)
        ));
    }

    /// Unit under test: [`GameOrchestrator::save_to_slot`] requires an active
    /// game screen producing a live snapshot.
    ///
    /// Preconditions: the active handler is the start menu (no live snapshot),
    /// even though cached map and level bytes are present from prior play.
    ///
    /// Invariant asserted: the save is refused with
    /// [`SaveError::NothingToSave`] rather than persisting stale cached bytes -
    /// the player can only save from inside a level or the world map.
    #[test]
    fn save_to_slot_requires_an_active_game_screen() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::StartMenu));
        let mut orchestrator = orchestrator_with_handler(handler);
        orchestrator.map_jn_bytes = Some(vec![1u8; 8]);
        orchestrator.level_jn_bytes = Some(vec![2u8; 8]);

        assert!(matches!(
            orchestrator.save_to_slot(2, "HERO"),
            Err(SaveError::NothingToSave)
        ));
    }

    /// Unit under test: [`GameOrchestrator::record_high_score`] writes through
    /// to the runtime config and surfaces via [`GameOrchestrator::high_scores`].
    #[test]
    fn record_high_score_updates_table() {
        let handler = Box::new(OneShotTransitionHandler::new(ScreenTransition::StartMenu));
        let mut orchestrator = orchestrator_with_handler(handler);

        orchestrator
            .record_high_score("ACE", 4242)
            .expect("record succeeds");

        assert_eq!(orchestrator.high_scores()[0].name(), "ACE");
        assert_eq!(orchestrator.high_scores()[0].score(), 4242);
    }
}
