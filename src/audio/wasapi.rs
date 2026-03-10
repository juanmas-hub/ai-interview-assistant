use anyhow::Result;
use tokio::sync::mpsc;
use std::thread;
use wasapi;
use ringbuf::{HeapRb, traits::{Producer, Consumer, Split}};

use crate::config;
use super::{AudioEvent, AudioFormat, Speaker};
use super::resampler::Resampler;

enum AudioSource {
    Microphone,
    SystemLoopback,
}

impl AudioSource {
    fn wasapi_direction(&self) -> wasapi::Direction {
        match self {
            AudioSource::Microphone     => wasapi::Direction::Capture,
            AudioSource::SystemLoopback => wasapi::Direction::Render,
        }
    }

    fn speaker(&self) -> Speaker {
        match self {
            AudioSource::Microphone     => Speaker::User,
            AudioSource::SystemLoopback => Speaker::System,
        }
    }
}

struct OpenDevice {
    audio_client:   wasapi::AudioClient,
    capture_client: wasapi::AudioCaptureClient,
    event_handle:   wasapi::Handle,
    format:         AudioFormat,
}


pub fn start_concurrent_capture(tx: mpsc::Sender<AudioEvent>) -> Result<()> {
    spawn_capture_pipeline(AudioSource::Microphone,     tx.clone());
    spawn_capture_pipeline(AudioSource::SystemLoopback, tx);
    Ok(())
}


fn spawn_capture_pipeline(source: AudioSource, tx: mpsc::Sender<AudioEvent>) {
    let ring = HeapRb::<f32>::new(config::capture::RING_BUFFER_CAPACITY);
    let (mut producer, consumer) = ring.split();

    let (format_tx, format_rx) = std::sync::mpsc::channel::<AudioFormat>();

    let speaker   = source.speaker();
    let direction = source.wasapi_direction();

    thread::spawn(move || {
        let _ = wasapi::initialize_mta();
        if let Err(e) = fill_ring_buffer(direction, speaker, &mut producer, format_tx) {
            eprintln!("[wasapi] {speaker} capture error: {e:?}");
        }
    });

    thread::spawn(move || {
        match format_rx.recv() {
            Ok(format) => {
                if let Err(e) = forward_normalized_audio(consumer, speaker, format, tx) {
                    eprintln!("[wasapi] {speaker} forwarding error: {e:?}");
                }
            }
            Err(e) => eprintln!("[wasapi] {speaker} failed to receive format metadata: {e:?}"),
        }
    });
}


fn fill_ring_buffer(
    direction: wasapi::Direction,
    speaker:   Speaker,
    producer:  &mut impl Producer<Item = f32>,
    format_tx: std::sync::mpsc::Sender<AudioFormat>,
) -> Result<()> {
    let device = open_device(&direction)?;

    println!("[wasapi] {speaker} capture started — {}", device.format);

    format_tx.send(device.format)
        .map_err(|_| anyhow::anyhow!("format channel closed before metadata was sent"))?;

    loop {
        device.event_handle.wait_for_event(config::capture::EVENT_TIMEOUT_MS)?;
        drain_device_buffer(&device.capture_client, device.format.channels as usize, producer)?;
    }
}


fn forward_normalized_audio(
    mut consumer: impl Consumer<Item = f32>,
    speaker:      Speaker,
    format:       AudioFormat,
    tx:           mpsc::Sender<AudioEvent>,
) -> Result<()> {
    let mut resampler = Resampler::new(format.sample_rate as f64)?;
    let mut chunk     = vec![0f32; config::capture::CONSUMER_CHUNK_SIZE];

    loop {
        let samples_read = consumer.pop_slice(&mut chunk);

        if samples_read == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }

        let raw = &chunk[..samples_read];

        tx.blocking_send(AudioEvent::RawCapture {
            speaker,
            samples: raw.to_vec(),
            format,
        }).map_err(|_| anyhow::anyhow!("audio channel closed"))?;

        let mono      = Resampler::downmix_to_mono(raw, format.channels as usize);
        let resampled = resampler.resample(&mono)?;

        if !resampled.is_empty() {
            tx.blocking_send(AudioEvent::NormalizedCapture {
                speaker,
                samples: resampled,
            }).map_err(|_| anyhow::anyhow!("audio channel closed"))?;
        }
    }
}

fn open_device(direction: &wasapi::Direction) -> Result<OpenDevice> {
    let device = wasapi::DeviceEnumerator::new()?
        .get_default_device(direction)?;

    let mut audio_client = device.get_iaudioclient()?;

    let mix_format  = audio_client.get_mixformat()?;
    let sample_rate = mix_format.get_samplespersec();
    let channels    = mix_format.get_nchannels() as usize;

    let float32_pcm = wasapi::WaveFormat::new(
        32, 32,
        &wasapi::SampleType::Float,
        sample_rate as usize,
        channels,
        None,
    );

    let (default_period, _) = audio_client.get_device_period()?;

    let shared_event_mode = wasapi::StreamMode::EventsShared {
        autoconvert:          true,
        buffer_duration_hns:  default_period,
    };

    audio_client.initialize_client(
        &float32_pcm,
        &wasapi::Direction::Capture,
        &shared_event_mode,
    )?;

    let capture_client = audio_client.get_audiocaptureclient()?;
    let event_handle   = audio_client.set_get_eventhandle()?;
    audio_client.start_stream()?;

    Ok(OpenDevice {
        audio_client,
        capture_client,
        event_handle,
        format: AudioFormat {
            sample_rate: sample_rate as u32,
            channels:    channels as u16,
        },
    })
}

fn drain_device_buffer(
    capture_client: &wasapi::AudioCaptureClient,
    channels:        usize,
    producer:        &mut impl Producer<Item = f32>,
) -> Result<()> {
    loop {
        let packet_frames = capture_client.get_next_packet_size()?;
        if packet_frames.unwrap() == 0 { break; }

        let byte_count = packet_frames.unwrap() as usize * channels * std::mem::size_of::<f32>();
        let mut raw_bytes = vec![0u8; byte_count];
        capture_client.read_from_device(&mut raw_bytes)?;

        if raw_bytes.is_empty() { break; }

        let samples = le_bytes_to_f32_samples(&raw_bytes);
        let dropped = samples.len() - producer.push_slice(&samples);

        if dropped > 0 {
            eprintln!("[wasapi] Ring buffer overflow — {dropped} samples dropped");
        }
    }
    Ok(())
}

fn le_bytes_to_f32_samples(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}