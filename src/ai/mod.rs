pub mod rag;
pub mod llm;
pub mod embedder;
pub mod vector_store;
pub mod setup;

use anyhow::Result;

pub async fn run(question: &str) -> Result<String> {
    let vec = embedder::embed(question).await?;
    println!("[embedder] dims={} primer_valor={:.4}", vec.len(), vec[0]);
    llm::complete(question).await
}