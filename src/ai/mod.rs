pub mod rag;
pub mod llm;

use anyhow::Result;

pub async fn run(question: &str) -> Result<String> {
    llm::complete(question).await
}