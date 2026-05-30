#![forbid(unsafe_code)]

//! Audio runtime for the Jill port.
//!
//! Plays the original `*.VCL` sound effects (8-bit signed PCM) through `rodio`.
//! [`AudioBackend`] is the only type that links `rodio`; the rest of the engine
//! produces rodio-free [`SoundEvent`]s and the host forwards them here.
//!
//! Background music (`*.DDT`, CMF/OPL2) is out of scope - see
//! `docs/port/08-audio-runtime.md`.

use openjill_core::SoundEvent;
use openjill_data::vcl::{VclFile, VclSound};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle};

/// A VCL sound decoded to `f32` PCM, ready to feed `rodio`.
struct DecodedSound {
    /// Sample rate in Hz (from the VCL entry).
    sample_rate: u32,
    /// PCM samples in `[-1.0, 1.0]`.
    samples: Vec<f32>,
}

/// Live `rodio` output. The stream is held so it stays open for the backend's
/// lifetime; dropping `AudioBackend` drops it and releases the device.
struct Output {
    /// Output stream kept alive alongside its handle.
    _stream: OutputStream,
    /// Handle used to play detached, overlapping sound sources.
    handle: OutputStreamHandle,
}

/// Plays semantic [`SoundEvent`]s through `rodio` using the original VCL sounds.
///
/// Construct once with the episode's [`VclFile`]; call [`AudioBackend::play`]
/// for each cue gameplay emits and [`AudioBackend::set_muted`] for the NOISE
/// toggle. When no audio device is available (headless / CI) the backend
/// constructs successfully and every `play` is a silent no-op.
pub struct AudioBackend {
    /// Decoded sounds indexed by VCL sound slot; `None` for empty slots.
    sounds: Vec<Option<DecodedSound>>,
    /// `rodio` output, or `None` when no device could be opened.
    output: Option<Output>,
    /// When `true`, [`AudioBackend::play`] is suppressed (NOISE off).
    muted: bool,
}

impl AudioBackend {
    /// Builds the backend from the episode's VCL file.
    ///
    /// Decodes every non-empty VCL sound to `f32` PCM and opens the default
    /// audio output. Never panics: if no output device is available the backend
    /// still constructs and every [`AudioBackend::play`] is a no-op.
    pub fn new(vcl: &VclFile) -> Self {
        let sounds = vcl
            .sounds()
            .iter()
            .map(|slot| slot.as_ref().map(decode))
            .collect();

        let output = match OutputStream::try_default() {
            Ok((stream, handle)) => Some(Output {
                _stream: stream,
                handle,
            }),
            Err(error) => {
                eprintln!("openjill-audio: no audio output device ({error}); sound disabled");
                None
            }
        };

        Self {
            sounds,
            output,
            muted: false,
        }
    }

    /// Plays the sound mapped to `event`.
    ///
    /// A no-op when muted, when no audio device is available, or when the event
    /// has no mapped / decoded sound. Sounds are fire-and-forget and may overlap.
    pub fn play(&self, event: SoundEvent) {
        if self.muted {
            return;
        }
        let Some(output) = &self.output else {
            return;
        };
        let Some(index) = vcl_index(event) else {
            return;
        };
        let Some(Some(sound)) = self.sounds.get(index) else {
            return;
        };

        let source = SamplesBuffer::new(1, sound.sample_rate, sound.samples.clone());
        if let Err(error) = output.handle.play_raw(source) {
            eprintln!("openjill-audio: failed to play sound {index}: {error}");
        }
    }

    /// Mutes or unmutes playback (driven by the NOISE toggle).
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }
}

/// Converts an 8-bit signed PCM VCL sound to `f32` samples in `[-1.0, 1.0]`.
fn decode(sound: &VclSound) -> DecodedSound {
    DecodedSound {
        sample_rate: u32::from(sound.frequency()),
        samples: sound.pcm().iter().map(|&s| f32::from(s) / 128.0).collect(),
    }
}

/// Maps a [`SoundEvent`] to its VCL sound slot index.
///
/// The four player cues are confirmed by ear against `JILL1.VCL`. Future cues
/// (pickups, enemies, doors, menu) need a slot each, but the original
/// event->sound mapping is not recorded by either port and could not be
/// identified by ear - it must be reverse-engineered from `JILL.EXE`. See the
/// "Sound index -> event mapping" section of `docs/port/08-audio-runtime.md`.
///
/// Returns `None` for events with no assigned sound (including future
/// `#[non_exhaustive]` variants).
fn vcl_index(event: SoundEvent) -> Option<usize> {
    let index = match event {
        SoundEvent::PlayerJump => 1,
        SoundEvent::PlayerFire => 2,
        SoundEvent::PlayerHurt => 3,
        SoundEvent::PlayerDie => 4,
        _ => return None,
    };
    Some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal in-memory VCL whose sound slot `index` carries `pcm` at
    /// `frequency`, located just past the table region.
    fn vcl_with_sound(index: usize, pcm: &[i8], frequency: u16) -> VclFile {
        // Sound tables (400) + text tables (240) = 640-byte table region.
        const TABLES_END: usize = 400 + (40 * 4) + (40 * 2);
        let payload_offset = TABLES_END;
        let mut bytes = vec![0u8; payload_offset + pcm.len()];

        // Sound slot: offset (u32 @ index*4), length (u16 @ 200 + index*2),
        // frequency (u16 @ 300 + index*2).
        bytes[index * 4..index * 4 + 4].copy_from_slice(&(payload_offset as u32).to_le_bytes());
        let length_pos = 200 + index * 2;
        bytes[length_pos..length_pos + 2].copy_from_slice(&(pcm.len() as u16).to_le_bytes());
        let freq_pos = 300 + index * 2;
        bytes[freq_pos..freq_pos + 2].copy_from_slice(&frequency.to_le_bytes());

        for (i, &sample) in pcm.iter().enumerate() {
            bytes[payload_offset + i] = sample as u8;
        }

        VclFile::from_bytes(bytes).expect("synthetic VCL should parse")
    }

    /// Unit under test: `AudioBackend::new` decodes VCL 8-bit signed PCM to
    /// `f32` in `[-1.0, 1.0]` at the entry's sample rate.
    #[test]
    fn decodes_vcl_pcm_to_f32() {
        // Slot 1 is the PlayerJump mapping; bytes span the signed extremes.
        let vcl = vcl_with_sound(1, &[0, 64, -128, 127], 6000);
        let backend = AudioBackend::new(&vcl);

        let sound = backend.sounds[1]
            .as_ref()
            .expect("slot 1 must decode to a sound");
        assert_eq!(sound.sample_rate, 6000);
        assert_eq!(sound.samples, vec![0.0, 0.5, -1.0, 127.0 / 128.0]);
    }

    /// Unit under test: the provisional event mapping resolves the wired cues to
    /// distinct, valid VCL slots.
    #[test]
    fn maps_player_events_to_distinct_slots() {
        let slots = [
            vcl_index(SoundEvent::PlayerJump),
            vcl_index(SoundEvent::PlayerFire),
            vcl_index(SoundEvent::PlayerHurt),
            vcl_index(SoundEvent::PlayerDie),
        ];
        assert_eq!(slots, [Some(1), Some(2), Some(3), Some(4)]);
    }

    /// Unit under test: constructing the backend and playing through it never
    /// panics, even with no audio device (headless / CI) - `play` is a silent
    /// no-op, and muting is honored.
    #[test]
    fn play_is_a_safe_no_op_without_a_device() {
        let vcl = vcl_with_sound(1, &[0, 1, 2, 3], 6000);
        let mut backend = AudioBackend::new(&vcl);

        // Whether or not a device exists on the test machine, these must not panic.
        backend.play(SoundEvent::PlayerJump);
        backend.set_muted(true);
        backend.play(SoundEvent::PlayerFire);
        backend.set_muted(false);
    }
}
