use hound::{WavSpec, WavWriter, SampleFormat};
use std::io::BufWriter;
use std::fs::File;
use anyhow::Result;

use crate::config;
use super::{AudioFormat, Speaker};

pub struct CaptureRecorder {
    user_writer:   Option<WavWriter<BufWriter<File>>>,
    system_writer: Option<WavWriter<BufWriter<File>>>,
}

impl CaptureRecorder {
    pub fn new() -> Self {
        Self { user_writer: None, system_writer: None }
    }

    pub fn record_chunk(&mut self, speaker: Speaker, samples: &[f32], format: AudioFormat) {
        let writer = match speaker {
            Speaker::User   => &mut self.user_writer,
            Speaker::System => &mut self.system_writer,
        };

        if writer.is_none() {
            *writer = open_wav_writer(speaker, format, SampleFormat::Float, 32);
        }

        if let Some(w) = writer {
            for &sample in samples {
                if let Err(e) = w.write_sample(sample) {
                    eprintln!("[wav] Write error for {speaker}: {e:?}");
                    return;
                }
            }
        }
    }
}

pub fn save_turn_as_wav(speaker: Speaker, start_ms: u128, samples: &[i16]) -> Result<()> {
    let path = format!("turn_{}_{start_ms}.wav", speaker.label());

    let spec = WavSpec {
        channels:        1,
        sample_rate:     config::resampler::TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format:   SampleFormat::Int,
    };

    let mut writer = WavWriter::create(&path, spec)?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;

    println!("[wav] Turn saved → '{path}'");
    Ok(())
}


fn open_wav_writer(
    speaker:       Speaker,
    format:        AudioFormat,
    sample_format: SampleFormat,
    bits:          u16,
) -> Option<WavWriter<BufWriter<File>>> {
    let path = match speaker {
        Speaker::User   => "raw_mic.wav",
        Speaker::System => "raw_sys.wav",
    };

    let spec = WavSpec {
        channels:        format.channels,
        sample_rate:     format.sample_rate,
        bits_per_sample: bits,
        sample_format,
    };

    match WavWriter::create(path, spec) {
        Ok(w) => {
            println!("[wav] Recording {speaker} → '{path}' ({format})");
            Some(w)
        }
        Err(e) => {
            eprintln!("[wav] Failed to open '{path}': {e:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_chunk_initializes_writer_for_each_speaker() {
        let mut recorder = CaptureRecorder::new();
        let format = AudioFormat {
            sample_rate: 16_000,
            channels: 1,
        };

        recorder.record_chunk(Speaker::User, &[0.25f32, -0.25f32], format);
        recorder.record_chunk(Speaker::System, &[0.5f32], format);

        assert!(recorder.user_writer.is_some());
        assert!(recorder.system_writer.is_some());
    }
}