use anyhow::{Result};
use rubato::{FftFixedIn, Resampler as RubatoResampler};

const OUTPUT_SAMPLE_RATE: f64 = 16_000.0;

const INPUT_CHUNK_FRAMES: usize = 1024;

const SUB_CHUNKS: usize = 2;

pub struct Resampler{
    resampler: FftFixedIn<f32>,
    input_buffer: Vec<Vec<f32>>,
    output_buffer: Vec<Vec<f32>>,
}

impl Resampler{

    pub fn new(input_sample_rate: f64) -> Result<Self> {

        println!(
            "[resampler] Created: {}Hz -> {}Hz (rubato FftFixedIn)",
            input_sample_rate, OUTPUT_SAMPLE_RATE
        );

        let resampler = FftFixedIn::<f32>::new(
            input_sample_rate as usize,
            OUTPUT_SAMPLE_RATE as usize,
            INPUT_CHUNK_FRAMES,
            SUB_CHUNKS,
            1,
        )
        .map_err(|e| anyhow::anyhow!("[resampler] Failed to create FftFixedIn: {}", e))?;

        Ok(Self {
            resampler,
            input_buffer:  vec![Vec::new()],
            output_buffer: vec![Vec::new()],
        })
    }

    pub fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
        if channels == 1 {
            return samples.to_vec();
        }
        samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    }

    pub fn resample(&mut self, mono_input: &[f32]) -> Result<Vec<i16>> {
        if mono_input.is_empty() {
            return Ok(Vec::new());
        }

        self.input_buffer[0].extend_from_slice(mono_input);

        let mut output_samples: Vec<i16> = Vec::new();

        loop {
            let frames_needed = self.resampler.input_frames_next();
            if self.input_buffer[0].len() < frames_needed {
                break;
            }

            let chunk: Vec<f32> = self.input_buffer[0].drain(0..frames_needed).collect();
            let input_chunk = vec![chunk];

            let output_frames = self.resampler.output_frames_next();
            self.output_buffer[0].resize(output_frames, 0.0);

            match self.resampler.process_into_buffer(&input_chunk, &mut self.output_buffer, None) {
                Ok((_, out_len)) => {
                    for &sample in &self.output_buffer[0][..out_len] {
                        let scaled = (sample * 32_767.0).clamp(-32_768.0, 32_767.0);
                        output_samples.push(scaled as i16);
                    }
                }
                Err(e) => eprintln!("[resampler] Process error: {}", e),
            }
        }

        Ok(output_samples)
    }
}

