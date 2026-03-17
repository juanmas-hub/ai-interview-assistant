use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const API_URL: &str = "https://api.voyageai.com/v1/embeddings";
const MODEL:   &str = "voyage-3-lite";

static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

pub fn embed(text: &str) -> Result<Vec<f32>> {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(embed_async(text))
    })
}

async fn embed_async(text: &str) -> Result<Vec<f32>> {
    let api_key  = read_api_key()?;
    let request  = build_request(text);
    let response = call_api(&api_key, &request).await?;

    extract_vector(response)
}

fn read_api_key() -> Result<String> {
    std::env::var("VOYAGE_API_KEY")
        .map_err(|_| anyhow::anyhow!("VOYAGE_API_KEY env var not set"))
}

fn build_request(text: &str) -> Request {
    Request {
        model: MODEL,
        input: text.to_string(),
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

fn extract_vector(response: Response) -> Result<Vec<f32>> {
    response
        .data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .ok_or_else(|| anyhow::anyhow!("OpenAI no devolvió ningún embedding"))
}

#[derive(Serialize)]
struct Request {
    model: &'static str,
    input: String,
}

#[derive(Deserialize)]
struct Response {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}