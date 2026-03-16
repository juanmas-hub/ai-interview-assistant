use anyhow::Result;
use serde::Deserialize;

use crate::audio::Speaker;
use crate::config;
use super::SttSender;
use crate::audio::vad::SpeechTurn;

#[derive(Deserialize)]
struct DgResponse {
    results: DgResults,
}

#[derive(Deserialize)]
struct DgResults {
    channels: Vec<DgChannel>,
}

#[derive(Deserialize)]
struct DgChannel {
    alternatives: Vec<DgAlternative>,
}

#[derive(Deserialize)]
struct DgAlternative {
    transcript: String,
}

pub struct DeepgramSender {
    api_key: String,
}

impl DeepgramSender {
    pub fn new(api_key: &str) -> Self {
        Self { api_key: api_key.to_string() }
    }
}

impl SttSender for DeepgramSender {
    fn send_turn(&self, turn: &SpeechTurn) {
        let api_key = self.api_key.clone();
        let audio   = turn.audio.clone();
        let speaker = turn.speaker;

        tokio::spawn(async move {
            match transcribe(&audio, &api_key).await {
                Ok(text) if !text.trim().is_empty() => {
                    let label = match speaker {
                        Speaker::User   => "[User]",
                        Speaker::System => "[Interviewer]",
                    };
                    println!("{label}: {}", text.trim());
                }
                Ok(_)    => {}
                Err(e)   => eprintln!("[deepgram] {speaker} transcription error: {e}"),
            }
        });
    }
}

async fn transcribe(samples: &[i16], api_key: &str) -> Result<String> {
    let chunks: Vec<&[i16]> = samples.chunks(config::deepgram::CHUNK_SAMPLES).collect();

    let client = reqwest::Client::new();
    let mut handles = Vec::new();

    for chunk in chunks {
        let bytes: Vec<u8> = chunk.iter().flat_map(|s| s.to_le_bytes()).collect();
        let client  = client.clone();
        let api_key = api_key.to_string();

        handles.push(tokio::spawn(async move {
            transcribe_chunk(&client, &bytes, &api_key).await
        }));
    }

    let mut transcript = String::new();
    for handle in handles {
        let text = handle.await??;
        if !transcript.is_empty() && !text.is_empty() {
            transcript.push(' ');
        }
        transcript.push_str(&text);
    }

    Ok(transcript)
}

async fn transcribe_chunk(client: &reqwest::Client, bytes: &[u8], api_key: &str) -> Result<String> {
    let response = client
        .post(config::deepgram::REST_URL)
        .header("Authorization", format!("Token {api_key}"))
        .header("Content-Type", "application/octet-stream")
        .body(bytes.to_vec())
        .send()
        .await?
        .json::<DgResponse>()
        .await?;

    let text = response.results.channels
        .into_iter()
        .next()
        .and_then(|c| c.alternatives.into_iter().next())
        .map(|a| a.transcript)
        .unwrap_or_default();

    Ok(text)
}