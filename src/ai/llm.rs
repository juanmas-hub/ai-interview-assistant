use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use super::prompt::Prompt;

#[async_trait]
pub trait Llm: Send + Sync {
    async fn complete(&self, prompt: Prompt) -> Result<String>;
}

const API_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const MODEL:   &str = "llama-3.1-8b-instant";

static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

pub struct GroqLlm {
    api_key: String,
}

impl GroqLlm {
    pub fn new() -> Result<Self> {
        Ok(Self { api_key: read_api_key()? })
    }
}

#[async_trait]
impl Llm for GroqLlm {
    async fn complete(&self, prompt: Prompt) -> Result<String> {
        let request: Request  = build_request(prompt);
        let response: Response = call_api(&self.api_key, &request).await?;
        extract_content(response)
    }
}

fn read_api_key() -> Result<String> {
    std::env::var("GROQ_API_KEY")
        .map_err(|_| anyhow::anyhow!("GROQ_API_KEY env var not set"))
}

fn build_request(prompt: Prompt) -> Request {
    let mut messages = Vec::with_capacity(2 + prompt.history.len() * 2);

    messages.push(Message { role: "system", content: prompt.system });

    for turn in prompt.history {
        messages.push(Message { role: "user",      content: turn.question });
        messages.push(Message { role: "assistant", content: turn.answer });
    }

    messages.push(Message { role: "user", content: prompt.question });

    Request { model: MODEL, messages }
}

async fn call_api(api_key: &str, request: &Request) -> Result<Response> {
    HTTP_CLIENT
        .post(API_URL)
        .bearer_auth(api_key)
        .json(request)
        .send()
        .await?
        .error_for_status()?
        .json::<Response>()
        .await
        .map_err(Into::into)
}

fn extract_content(response: Response) -> Result<String> {
    response
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| anyhow::anyhow!("Groq no devolvió ninguna respuesta"))
}

#[derive(Serialize)]
struct Request {
    model:    &'static str,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role:    &'static str,
    content: String,
}

#[derive(Deserialize)]
struct Response {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: String,
}