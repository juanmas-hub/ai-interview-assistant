use crate::ai::vector_store::SearchResult;

pub struct Prompt {
    pub system: String,
    pub user:   String,
}

pub fn build(context: &[SearchResult], question: &str) -> Prompt {
    Prompt {
        system: build_system_prompt(context),
        user:   question.to_string(),
    }
}

fn build_system_prompt(context: &[SearchResult]) -> String {
    let context_block = format_context(context);

    let context_section = if context_block.is_empty() {
        String::new()
    } else {
        format!("Candidate background:\n{context_block}\n\n")
    };

    format!(
        "You are an expert assistant helping a candidate during a backend technical interview.\n\
         Give 2-3 concise bullet points the candidate can use to answer the question.\n\
         If candidate background is provided, use it to personalize the answer.\n\
         Always complement with your own technical knowledge to give a complete, smart response.\n\
         Do NOT ask questions. Do NOT elaborate. Just key points to mention.\n\n\
         {context_section}"
    )
}

fn format_context(context: &[SearchResult]) -> String {
    context
        .iter()
        .enumerate()
        .map(|(i, r)| format!("[{}] {}", i + 1, r.payload))
        .collect::<Vec<_>>()
        .join("\n\n")
}