use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}

const API_URL: &str = "https://api.voyageai.com/v1/embeddings";
const MODEL:   &str = "voyage-3-lite";

static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

pub struct VoyageEmbedder {
    api_key: String,
}

impl VoyageEmbedder {
    pub fn new() -> Result<Self> {
        Ok(Self { api_key: read_api_key()? })
    }
}

#[async_trait]
impl Embedder for VoyageEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.embed_batch(&[text])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Voyage no devolvió ningún embedding"))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let request  = build_request(texts);
        let response = call_api(&self.api_key, &request).await?;
        extract_vectors(response)
    }
}

fn read_api_key() -> Result<String> {
    std::env::var("VOYAGE_API_KEY")
        .map_err(|_| anyhow::anyhow!("VOYAGE_API_KEY env var not set"))
}

fn build_request(texts: &[&str]) -> Request {
    Request {
        model: MODEL,
        input: texts.iter().map(|t| t.to_string()).collect(),
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

fn extract_vectors(response: Response) -> Result<Vec<Vec<f32>>> {
    if response.data.is_empty() {
        return Err(anyhow::anyhow!("Voyage no devolvió ningún embedding"));
    }
    Ok(response.data.into_iter().map(|d| d.embedding).collect())
}

#[derive(Serialize)]
struct Request {
    model: &'static str,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct Response {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}