#![forbid(unsafe_code)]

#[derive(Default)]
pub struct AudioBackend;

impl AudioBackend {
    pub fn new() -> Self {
        Self
    }
}
