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
    let grounding_rule  = grounding_rule(context);
    let context_section = format_context_section(context);

    format!(
        "You are a technical interview copilot. The interviewer just asked the \
         candidate a question in real time — you have seconds, not minutes, to help.\n\
         \n\
         Respond with 4-6 bullet points the candidate can glance at and say out loud. \
         Each bullet should be one full sentence — enough to convey the what AND the \
         why/how, not a bare keyword fragment. Don't pad artificially: if the question \
         genuinely only supports 3 solid points, give 3 rather than stretching to 6 — \
         but default to giving real, usable signal over a thin list.\n\
         \n\
         {grounding_rule}\n\
         \n\
         General rules:\n\
         - Lead with the most relevant point first.\n\
         - No preamble, no follow-up questions, no restating the question.\n\
         - Prefer concrete nouns (technology names, numbers, tradeoffs) over vague \
           adjectives.\n\
         {context_section}"
    )
}

fn grounding_rule(context: &[SearchResult]) -> String {
    if context.is_empty() {
        "No candidate background matched this question. Answer using general, \
         widely-known technical knowledge ONLY — speak conceptually or in the \
         infinitive (\"a common approach is...\", \"this typically involves...\"), \
         NOT as a first-person personal anecdote.\n\
         Do NOT invent a company name, project name, metric, or personal experience \
         under any circumstance. Never write a placeholder like \"[Company]\" or \
         \"[Project]\" — if you don't have a real detail to cite, don't reference one \
         at all. It is completely fine, and preferred, for an answer to contain zero \
         personal references when there's no real background to draw from."
            .to_string()
    } else {
        format!(
            "Ground your answer in the candidate's background below wherever it's \
             relevant — cite the specific project or experience by name, not a generic \
             claim. You may add general technical knowledge to complete the answer, but \
             never invent a specific company, project name, or metric that isn't in the \
             background below.\n\
             Draw from AS MANY DIFFERENT entries below as are genuinely relevant to this \
             question — up to {} are provided precisely so the answer can be varied. Do \
             NOT build every bullet around the same single project or technology if other \
             relevant entries exist; each bullet should ideally bring in a different \
             angle (a different project, technology, or lesson learned). Only repeat the \
             same background entry across bullets if the other entries are truly \
             irrelevant to this specific question.\n\
             If the background only partially covers the question, say what it does \
             cover concretely and fill the rest with general knowledge framed \
             impersonally — don't stretch the real background to sound like it covers \
             something it doesn't.",
            context.len(),
        )
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::vector_store::SearchResult;

    fn search_results(payloads: &[&str]) -> Vec<SearchResult> {
        payloads
            .iter()
            .enumerate()
            .map(|(index, payload)| SearchResult {
                payload: (*payload).to_string(),
                score: 1.0 - (index as f32 * 0.1),
            })
            .collect()
    }

    #[test]
    fn build_sets_user_question_and_system_prompt() {
        let prompt = build(&[], "Describe your experience with Rust");

        assert_eq!(prompt.user, "Describe your experience with Rust");
        assert!(prompt.system.contains("You are a technical interview copilot."));
        assert!(prompt.system.contains("General rules:"));
    }

    #[test]
    fn grounding_rule_without_context_uses_general_knowledge() {
        let rule = grounding_rule(&[]);

        assert!(rule.contains("general, widely-known technical knowledge ONLY"));
        assert!(rule.contains("Do NOT invent a company name"));
        assert!(rule.contains("NOT as a first-person personal anecdote"));
    }

    #[test]
    fn grounding_rule_with_context_mentions_background_and_entry_count() {
        let context = search_results(&["Built a Rust service", "Led an async migration"]);
        let rule = grounding_rule(&context);

        assert!(rule.contains("Ground your answer in the candidate's background below"));
        assert!(rule.contains("up to 2 are provided precisely"));
        assert!(rule.contains("Do NOT build every bullet around the same single project"));
    }

    #[test]
    fn format_context_section_returns_empty_string_for_empty_context() {
        assert!(format_context_section(&[]).is_empty());
    }

    #[test]
    fn format_context_section_formats_entries_in_order() {
        let context = search_results(&["Built a Rust service", "Led an async migration"]);
        let section = format_context_section(&context);

        assert!(section.starts_with("\nCandidate background (most relevant first):\n"));
        assert!(section.contains("[1] Built a Rust service"));
        assert!(section.contains("[2] Led an async migration"));
    }
}