use anyhow::{Context, Result};
use std::sync::mpsc; // it might should be tokio, in main its tokio
use std::thread;
use std::error::Error;
use wasapi;
use crate::AudioEvent;


pub fn start_concurrent_capture(transmitter: mpsc::Sender<AudioEvent>) -> anyhow::Result<()>{

    let microphone_transmitter: mpsc::Sender<AudioEvent> = transmitter.clone();
    thread::spawn(move || {
        //if let Err(e) = capture_loop(){
        //}
    });

    let speaker_transmitter: mpsc::Sender<AudioEvent> = transmitter.clone();
    thread::spawn(move || {
        //if let Err(e) = capture_loop(){
        //}
    });


    Ok(())
}

fn capture_loop(
    device_direction: wasapi::Direction,
    is_user: bool,
    transmitter: mpsc::Sender<AudioEvent>
    ) -> anyhow::Result<()> {

        let device= wassapi::get_default_device(&device_direction)?;

        let mut audio_client = device.get_iaudioclient()?;

        let device_format = audio_client.get_mixformat()?;

        let actual_rate = device_format.get_samplespersec();

        let desired_format = wasapi::WaveFormat::new(
        32,
        32,
        &wasapi::SampleType::Float,
        actual_rate as usize,
        1,
        None,
    );

    let (default_period, min_period) = audio_client.get_periods()?;
        

    let stream_mode = wasapi::StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: min_period,
    };

    audio_client.
    Ok(())
}