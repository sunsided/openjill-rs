#![forbid(unsafe_code)]

use openjill_audio::AudioBackend;
use openjill_core::CoreState;
use openjill_render::Renderer;

pub struct GameApp {
    core: CoreState,
    renderer: Renderer,
    audio: AudioBackend,
}

impl GameApp {
    pub fn new(core: CoreState, renderer: Renderer, audio: AudioBackend) -> Self {
        Self {
            core,
            renderer,
            audio,
        }
    }

    pub fn core(&self) -> &CoreState {
        &self.core
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub fn audio(&self) -> &AudioBackend {
        &self.audio
    }
}
