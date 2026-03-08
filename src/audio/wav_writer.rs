use hound::{WavSpec, WavWriter, SampleFormat};
use std::io::BufWriter;
use std::fs::File;

pub struct DualWavWriter {
    mic_writer: Option<WavWriter<BufWriter<File>>>,
    sys_writer: Option<WavWriter<BufWriter<File>>>,
}

impl DualWavWriter {
    pub fn new() -> Self {
        Self { mic_writer: None, sys_writer: None }
    }

    pub fn write_chunk(&mut self, is_user: bool, samples: &[f32], sample_rate: u32, channels: u16) {
        let writer = if is_user { &mut self.mic_writer } else { &mut self.sys_writer };

        if writer.is_none() {
            *writer = Self::create_writer(is_user, sample_rate, channels);
        }

        if let Some(w) = writer {
            for &sample in samples {
                if let Err(e) = w.write_sample(sample) {
                    eprintln!("[wav] Error while writing sample: {:?}", e);
                    return;
                }
            }
        }
    }

    fn create_writer(is_user: bool, sample_rate: u32, channels: u16) -> Option<WavWriter<BufWriter<File>>> {
        let filename = if is_user { "test_mic.wav" } else { "test_sys.wav" };
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };

        match WavWriter::create(filename, spec) {
            Ok(w) => {
                println!("[wav] '{}' created — {}Hz, {} channels", filename, sample_rate, channels);
                Some(w)
            }
            Err(e) => {
                eprintln!("[wav] Failed to create '{}': {:?}", filename, e);
                None
            }
        }
    }
}