use anyhow::Result;
use std::thread;
use tokio::sync::mpsc;
use wasapi;
use ringbuf::{HeapRb, traits::{Producer, Consumer, Split}};

use crate::config;
use super::{AudioEvent, AudioFormat, Speaker};

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
    capture_client: wasapi::AudioCaptureClient,
    event_handle:   wasapi::Handle,
    format:         AudioFormat,
}

struct SendableDevice(OpenDevice);
unsafe impl Send for SendableDevice {}
unsafe impl Sync for SendableDevice {}

pub fn start_concurrent_capture(tx: mpsc::Sender<AudioEvent>) -> Result<()> {
    spawn_capture_pipeline(AudioSource::Microphone,     tx.clone());
    spawn_capture_pipeline(AudioSource::SystemLoopback, tx);
    Ok(())
}

fn spawn_capture_pipeline(source: AudioSource, tx: mpsc::Sender<AudioEvent>) {
    let speaker        = source.speaker();
    let (device, format) = open_source(&source);
    let (producer, consumer) = HeapRb::<f32>::new(config::capture::RING_BUFFER_CAPACITY).split();

    spawn_fill_thread(speaker, device, producer);
    spawn_forward_thread(speaker, format, consumer, tx);
}

fn open_source(source: &AudioSource) -> (SendableDevice, AudioFormat) {
    let _ = wasapi::initialize_mta();
    let device = open_device(&source.wasapi_direction()).expect("failed to open audio device");
    let format = device.format;
    (SendableDevice(device), format)
}

fn spawn_fill_thread(
    speaker:  Speaker,
    device:   SendableDevice,
    mut producer: impl Producer<Item = f32> + Send + 'static,
) {
    thread::spawn(move || {
        let _ = wasapi::initialize_mta();
        if let Err(e) = fill_ring_buffer(device, speaker, &mut producer) {
            eprintln!("[wasapi] {speaker} capture error: {e:?}");
        }
    });
}

fn spawn_forward_thread(
    speaker:  Speaker,
    format:   AudioFormat,
    consumer: impl Consumer<Item = f32> + Send + 'static,
    tx:       mpsc::Sender<AudioEvent>,
) {
    thread::spawn(move || {
        let _ = wasapi::initialize_mta();
        if let Err(e) = forward_raw_audio(consumer, speaker, format, tx) {
            eprintln!("[wasapi] {speaker} forwarding error: {e:?}");
        }
    });
}

fn fill_ring_buffer(
    device:   SendableDevice,
    speaker:  Speaker,
    producer: &mut impl Producer<Item = f32>,
) -> Result<()> {
    println!("[wasapi] {speaker} capture started — {}", device.0.format);

    loop {
        device.0.event_handle.wait_for_event(config::capture::EVENT_TIMEOUT_MS)?;
        drain_device_buffer(&device.0.capture_client, device.0.format.channels as usize, producer)?;
    }
}

fn forward_raw_audio(
    mut consumer: impl Consumer<Item = f32>,
    speaker:      Speaker,
    format:       AudioFormat,
    tx:           mpsc::Sender<AudioEvent>,
) -> Result<()> {
    let mut chunk = vec![0f32; config::capture::CONSUMER_CHUNK_SIZE];

    loop {
        let n = consumer.pop_slice(&mut chunk);

        if n == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }

        send_audio_chunk(&tx, speaker, format, &chunk[..n])?;
    }
}

fn open_device(direction: &wasapi::Direction) -> Result<OpenDevice> {
    let mut audio_client: wasapi::AudioClient       = acquire_audio_client(direction)?;
    let audio_format: AudioFormat                   = init_capture_stream(&mut audio_client)?;
    let capture_client: wasapi::AudioCaptureClient  = audio_client.get_audiocaptureclient()?;
    let event_handle: wasapi::Handle                = audio_client.set_get_eventhandle()?;
    audio_client.start_stream()?;

    Ok(OpenDevice { capture_client, event_handle, format: audio_format })
}

fn acquire_audio_client(direction: &wasapi::Direction) -> Result<wasapi::AudioClient> {
    let device = wasapi::DeviceEnumerator::new()?
        .get_default_device(direction)?;
    device.get_iaudioclient().map_err(Into::into)
}

fn init_capture_stream(audio_client: &mut wasapi::AudioClient) -> Result<AudioFormat> {
    let mix_format: wasapi::WaveFormat  = audio_client.get_mixformat()?;
    let sample_rate: u32                = mix_format.get_samplespersec();
    let channels: usize                 = mix_format.get_nchannels() as usize;

    let float32_format = wasapi::WaveFormat::new(
        32, 32,
        &wasapi::SampleType::Float,
        sample_rate as usize,
        channels,
        None,
    );

    let (default_period, _) = audio_client.get_device_period()?;
    let stream_mode = wasapi::StreamMode::EventsShared {
        autoconvert:         true,
        buffer_duration_hns: default_period,
    };

    audio_client.initialize_client(
        &float32_format,
        &wasapi::Direction::Capture,
        &stream_mode,
    )?;

    Ok(AudioFormat {
        sample_rate: sample_rate as u32,
        channels:    channels as u16,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn drain_device_buffer(
    capture_client: &wasapi::AudioCaptureClient,
    channels:       usize,
    producer:       &mut impl Producer<Item = f32>,
) -> Result<()> {
    while let Some(samples) = read_next_packet(capture_client, channels)? {
        push_to_ring_buffer(producer, &samples);
    }
    Ok(())
}

fn read_next_packet(
    capture_client: &wasapi::AudioCaptureClient,
    channels:       usize,
) -> Result<Option<Vec<f32>>> {
    let packet_frames = match capture_client.get_next_packet_size()? {
        Some(0) | None => return Ok(None),
        Some(n)        => n as usize,
    };

    let byte_count = packet_frames * channels * std::mem::size_of::<f32>();
    let mut raw_bytes = vec![0u8; byte_count];
    capture_client.read_from_device(&mut raw_bytes)?;

    if raw_bytes.is_empty() { return Ok(None); }

    Ok(Some(le_bytes_to_f32_samples(&raw_bytes)))
}

fn push_to_ring_buffer(producer: &mut impl Producer<Item = f32>, samples: &[f32]) {
    let dropped = samples.len() - producer.push_slice(samples);
    if dropped > 0 {
        eprintln!("[wasapi] ring buffer overflow — {dropped} samples dropped");
    }
}

fn send_audio_chunk(
    tx:      &mpsc::Sender<AudioEvent>,
    speaker: Speaker,
    format:  AudioFormat,
    samples: &[f32],
) -> Result<()> {
    tx.blocking_send(AudioEvent::RawCapture {
        speaker,
        samples: samples.to_vec(),
        format,
    })
    .map_err(|_| anyhow::anyhow!("audio channel closed"))
}

fn le_bytes_to_f32_samples(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}