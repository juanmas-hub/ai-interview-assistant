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