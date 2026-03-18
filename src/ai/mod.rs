pub mod embedder;
mod llm;
mod prompt;
mod rag;
pub mod vector_store;

use anyhow::Result;
use std::sync::Arc;
use vector_store::VectorStore;

pub async fn run(question: &str, store: &Arc<VectorStore>) -> Result<String> {
    rag::answer(question, store).await
}
 