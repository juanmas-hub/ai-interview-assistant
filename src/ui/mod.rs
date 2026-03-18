pub mod overlay;
pub mod renderer;

use std::io::{self, BufRead, Write};

pub fn prompt_user_context() -> String {
    print_header();
    read_until_double_enter()
}


fn print_header() {
    println!();
    println!("╔══════════════════════════════════════════════╗");
    println!("║         AI Interview Copilot — Setup         ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();
    println!("Tell us about yourself before starting the interview.");
    println!("Write one idea per line. Examples:");
    println!("  I work at Caelum as a Full Stack Engineer using Go and React");
    println!("  I built a microservices-based ticketing app called Nexus using Go and TypeScript");
    println!("  I have experience with hexagonal architecture and DDD");
    println!();
    println!("Press Enter twice when you're done.");
    println!();
    print!("> ");
    io::stdout().flush().unwrap();
}

fn read_until_double_enter() -> String {
    let stdin = io::stdin();
    let mut lines: Vec<String> = Vec::new();
    let mut last_was_empty = false;

    for line in stdin.lock().lines() {
        let line = line.expect("failed to read line");

        if line.trim().is_empty() {
            if last_was_empty { break; }
            last_was_empty = true;
        } else {
            last_was_empty = false;
            lines.push(line);
        }
    }

    lines.join("\n")
}