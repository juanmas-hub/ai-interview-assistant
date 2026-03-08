use anyhow::Result;
use tokio::sync::mpsc;
use std::thread;
use wasapi;
use crate::AudioEvent;

pub fn start_concurrent_capture(transmitter: mpsc::Sender<AudioEvent>) -> Result<()> {
    spawn_capture_thread(wasapi::Direction::Capture, true,  transmitter.clone());
    spawn_capture_thread(wasapi::Direction::Render,  false, transmitter);
    Ok(())
}

fn spawn_capture_thread(direction: wasapi::Direction, is_user: bool, transmitter: mpsc::Sender<AudioEvent>) {
    thread::spawn(move || {
        let _ = wasapi::initialize_mta();
        let label = if is_user { "microphone" } else { "system" };

        if let Err(e) = capture_loop(direction, is_user, transmitter) {
            eprintln!("[wasapi] Error in {} capture: {:?}", label, e);
        }
    });
}

fn capture_loop(
    direction: wasapi::Direction,
    is_user: bool,
    transmitter: mpsc::Sender<AudioEvent>,
) -> Result<()> {
    let label = if is_user { "microphone" } else { "system" };

    let (_audio_client, capture_client, event_handle, sample_rate, channels) = 
        setup_audio_client(&direction)?;

    println!("[wasapi] Init {} capture — {}Hz, {} channels", label, sample_rate, channels);

    let timeout_ms = 1000;
    loop {
        event_handle.wait_for_event(timeout_ms)?;
        drain_capture_buffer(&capture_client, channels, &transmitter, is_user, sample_rate)?;
    }
}

fn setup_audio_client(
    direction: &wasapi::Direction,
) -> Result<(wasapi::AudioClient, wasapi::AudioCaptureClient, wasapi::Handle, u32, usize)> {
    let device = wasapi::DeviceEnumerator::new()?
        .get_default_device(direction)?;

    let mut audio_client = device.get_iaudioclient()?;

    let mix_format   = audio_client.get_mixformat()?;
    let sample_rate  = mix_format.get_samplespersec();
    let channels     = mix_format.get_nchannels() as usize;

    let desired_format = wasapi::WaveFormat::new(
        32, 32,
        &wasapi::SampleType::Float,
        sample_rate as usize,
        channels,
        None,
    );

    let (default_period, _) = audio_client.get_device_period()?;

    let stream_mode = wasapi::StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: default_period,
    };

    audio_client.initialize_client(&desired_format, &wasapi::Direction::Capture, &stream_mode)?;

    let capture_client = audio_client.get_audiocaptureclient()?;
    let event_handle   = audio_client.set_get_eventhandle()?;
    audio_client.start_stream()?;

    Ok((audio_client, capture_client, event_handle, sample_rate as u32, channels))
}

fn drain_capture_buffer(
    capture_client: &wasapi::AudioCaptureClient,
    channels: usize,
    transmitter: &mpsc::Sender<AudioEvent>,
    is_user: bool,
    sample_rate: u32,
) -> Result<()> {
    loop {
        let packet_size = capture_client.get_next_packet_size()?;
        if packet_size.unwrap() == 0 { break; }

        let expected_bytes = packet_size.unwrap() as usize * channels * 4;
        let mut raw = vec![0u8; expected_bytes];
        capture_client.read_from_device(&mut raw)?;

        if raw.is_empty() { break; }

        let samples = bytes_to_f32_samples(&raw);

        transmitter.blocking_send(AudioEvent::Chunk {
            is_user,
            data: samples,
            sample_rate,
            channels: channels as u16,
        }).map_err(|_| anyhow::anyhow!("[wasapi] Canal de audio cerrado"))?;
    }
    Ok(())
}

fn bytes_to_f32_samples(raw: &[u8]) -> Vec<f32> {
    raw.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}