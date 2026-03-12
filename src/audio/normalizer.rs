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