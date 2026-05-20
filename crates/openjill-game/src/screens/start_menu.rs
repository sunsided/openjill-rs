//! Start menu screen handler stub.
//!
//! Full start menu logic, including tile rendering and high-score overlay, is
//! implemented in child issue 4. This stub satisfies the `ScreenHandler` trait
//! so `GameOrchestrator` can boot with a valid initial handler.

use openjill_core::runtime::RuntimeState;
use openjill_core::{ActiveInput, ScreenHandler, TickResult};

/// Start menu screen handler.
///
/// Stub for this issue; child issue 4 replaces this with the full
/// `StartMenuJill1Handler` logic backed by `INTRO.JN1`.
pub struct StartMenuScreen;

impl StartMenuScreen {
    /// Creates the start menu screen.
    pub fn new() -> Self {
        Self
    }
}

impl Default for StartMenuScreen {
    /// Creates the start menu screen.
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenHandler for StartMenuScreen {
    /// Returns an empty tick result; full logic is added in child issue 4.
    fn tick(&mut self, _input: &ActiveInput, _state: &mut RuntimeState) -> TickResult {
        TickResult::empty()
    }
}
