use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const API_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const MODEL:   &str = "llama-3.1-8b-instant";

const SYSTEM_PROMPT: &str =
    "Sos un asistente experto ayudando a un candidato en una entrevista técnica. \
     Respondé de forma clara, concisa y en español.";

static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

pub async fn complete(question: &str) -> Result<String> {
    let api_key  = read_api_key()?;
    let request  = build_request(question);
    let response = call_api(&api_key, &request).await?;

    extract_content(response)
}

fn read_api_key() -> Result<String> {
    std::env::var("GROQ_API_KEY")
        .map_err(|_| anyhow::anyhow!("GROQ_API_KEY env var not set"))
}

fn build_request(question: &str) -> Request {
    Request {
        model:    MODEL,
        messages: vec![
            Message { role: "system", content: SYSTEM_PROMPT.to_string() },
            Message { role: "user",   content: question.to_string() },
        ],
    }
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
        .map(|choice| choice.message.content)
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