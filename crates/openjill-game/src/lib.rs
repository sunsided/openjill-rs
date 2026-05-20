#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;

use openjill_render::{Palette, Presenter, PresenterError, SurfaceError};
use thiserror::Error;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

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
}

impl GameApp {
    /// Creates a new game app shell before the event loop enters `resumed`.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            window: None,
            presenter: None,
            palette: Palette::default(),
            error: None,
        }
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
            _ => {}
        }
    }

    /// Clears and presents one frame while the loop is idle, then requests another redraw.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
