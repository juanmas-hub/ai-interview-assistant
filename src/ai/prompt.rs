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
    let context_section = format_context_section(context);

    format!(
        "You are a technical interview copilot. The interviewer just asked the \
         candidate a question in real time — you have seconds, not minutes, to help.\n\
         \n\
         Respond with 2-4 short bullet points the candidate can glance at and say out loud.\n\
         \n\
         Rules:\n\
         - Lead with the most relevant point first.\n\
         - If the candidate's background below contains something directly relevant, \
           anchor the answer in it — cite the specific project or experience by name, \
           not a generic claim.\n\
         - Fill any gaps with accurate general technical knowledge — never invent \
           specifics about the candidate's background that aren't in the context below.\n\
         - No preamble, no follow-up questions, no restating the question.\n\
         - Prefer concrete nouns (technology names, numbers, tradeoffs) over vague \
           adjectives.\n\
         {context_section}"
    )
}

fn format_context_section(context: &[SearchResult]) -> String {
    if context.is_empty() {
        return String::new();
    }

    let entries = context
        .iter()
        .enumerate()
        .map(|(i, r)| format!("[{}] {}", i + 1, r.payload))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!("\nCandidate background (most relevant first):\n{entries}\n")
}