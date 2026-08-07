use std::fs::File;
use std::io::Write;

pub struct Transcript {
    file: File,
}

impl Transcript {
    pub fn open(path: &str) -> Self {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap_or_else(|e| panic!("failed to open {path}: {e}"));

        Self { file }
    }

    pub fn write_line(&mut self, line: &str) {
        print!("{line}");
        if let Err(e) = self.file.write_all(line.as_bytes()) {
            eprintln!("[transcript] write error: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_transcript_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("ai_interview_assistant_transcript_{unique}.txt"))
    }

    #[test]
    fn write_line_persists_content_to_disk() {
        let path = temp_transcript_path();
        let mut transcript = Transcript::open(path.to_str().unwrap());

        transcript.write_line("hello\n");

        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written, "hello\n");

        let _ = fs::remove_file(path);
    }
}