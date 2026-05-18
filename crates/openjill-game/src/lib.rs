#![forbid(unsafe_code)]

use openjill_audio::AudioBackend;
use openjill_core::CoreState;
use openjill_render::Renderer;

pub struct GameApp {
    pub core: CoreState,
    pub renderer: Renderer,
    pub audio: AudioBackend,
}

impl GameApp {
    pub fn new(core: CoreState, renderer: Renderer, audio: AudioBackend) -> Self {
        Self {
            core,
            renderer,
            audio,
        }
    }
}
