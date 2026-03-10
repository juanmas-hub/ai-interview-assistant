use anyhow::Result;
use rubato::{FftFixedIn, Resampler as RubatoResampler};

use crate::config;

pub struct Resampler {
    inner:         FftFixedIn<f32>,
    input_buffer:  Vec<Vec<f32>>,
    output_buffer: Vec<Vec<f32>>,
}

impl Resampler {
    pub fn new(input_sample_rate: f64) -> Result<Self> {
        let target = config::resampler::TARGET_SAMPLE_RATE as f64;

        println!("[resampler] {input_sample_rate}Hz → {target}Hz");

        let inner = FftFixedIn::<f32>::new(
            input_sample_rate as usize,
            target as usize,
            config::resampler::INPUT_CHUNK_FRAMES,
            config::resampler::SUB_CHUNKS,
            1,
        )
        .map_err(|e| anyhow::anyhow!("failed to create resampler: {e}"))?;

        Ok(Self {
            inner,
            input_buffer:  vec![Vec::new()],
            output_buffer: vec![Vec::new()],
        })
    }

    pub fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
        match channels {
            1 => samples.to_vec(),
            n => samples
                .chunks_exact(n)
                .map(|frame| frame.iter().sum::<f32>() / n as f32)
                .collect(),
        }
    }

    pub fn resample(&mut self, mono_input: &[f32]) -> Result<Vec<i16>> {
        if mono_input.is_empty() {
            return Ok(Vec::new());
        }

        self.input_buffer[0].extend_from_slice(mono_input);
        let mut output: Vec<i16> = Vec::new();

        while self.input_buffer[0].len() >= self.inner.input_frames_next() {
            let frames_needed = self.inner.input_frames_next();
            let chunk: Vec<f32> = self.input_buffer[0].drain(..frames_needed).collect();

            let out_frames = self.inner.output_frames_next();
            self.output_buffer[0].resize(out_frames, 0.0);

            match self.inner.process_into_buffer(&[chunk], &mut self.output_buffer, None) {
                Ok((_, out_len)) => {
                    output.extend(
                        self.output_buffer[0][..out_len]
                            .iter()
                            .map(|&s| (s * 32_767.0).clamp(-32_768.0, 32_767.0) as i16),
                    );
                }
                Err(e) => eprintln!("[resampler] processing error: {e}"),
            }
        }

        Ok(output)
    }
}

