use anyhow::Result;

use crate::audio::{AudioFormat};
use crate::audio::resampler::Resampler;

pub struct AudioNormalizer {
    resampler: Option<Resampler>,
}

impl AudioNormalizer {
    pub fn new() -> Self {
        Self { resampler: None }
    }

    pub fn process(&mut self, samples: &[f32], format: AudioFormat) -> Result<Vec<i16>> {
        let resampler = self.resampler.get_or_insert_with(|| {
            Resampler::new(format.sample_rate as f64)
                .expect("failed to create resampler")
        });

        let mono = Resampler::downmix_to_mono(samples, format.channels as usize);
        resampler.resample(&mono)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioFormat;

    #[test]
    fn process_returns_empty_for_empty_input() {
        let mut normalizer = AudioNormalizer::new();
        let format = AudioFormat {
            sample_rate: 16_000,
            channels: 1,
        };

        match normalizer.process(&[], format) {
            Ok(samples) => assert!(samples.is_empty()),
            Err(err) => panic!("empty input should not error: {err}"),
        }
    }

    #[test]
    fn process_resamples_mono_input_without_error() {
        let mut normalizer = AudioNormalizer::new();
        let format = AudioFormat {
            sample_rate: 16_000,
            channels: 1,
        };
        let samples = vec![0.0f32; 4_096];

        match normalizer.process(&samples, format) {
            Ok(samples) => assert!(!samples.is_empty()),
            Err(err) => panic!("mono input should be processed: {err}"),
        }
    }
}