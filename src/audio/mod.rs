pub mod resampler;
pub mod hotkey;
pub mod vad;
pub mod wasapi;
pub mod wav_writer;

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    User,
    System,
}

impl Speaker {
    pub fn label(&self) -> &'static str {
        match self {
            Speaker::User   => "user",
            Speaker::System => "system",
        }
    }
}

impl fmt::Display for Speaker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Speaker::User   => write!(f, "USER  "),
            Speaker::System => write!(f, "SYSTEM"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels:    u16,
}

impl fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}Hz {}ch", self.sample_rate, self.channels)
    }
}

pub enum AudioEvent {
    RawCapture {
        speaker: Speaker,
        samples: Vec<f32>,
        format:  AudioFormat,
    },
    NormalizedCapture {
        speaker: Speaker,
        samples: Vec<i16>,
    },
    CaptureError {
        speaker: Speaker,
        error:   String,
    },
}