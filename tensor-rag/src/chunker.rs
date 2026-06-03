pub trait Chunker: Send + Sync {
    fn chunk(&self, text: &str) -> Vec<String>;
}

pub struct ParagraphChunker {
    pub max_chars: usize,
    pub overlap_chars: usize,
}

impl ParagraphChunker {
    pub fn new(max_chars: usize, overlap_chars: usize) -> Self {
        Self {
            max_chars,
            overlap_chars,
        }
    }
}

impl Default for ParagraphChunker {
    fn default() -> Self {
        Self::new(1000, 200)
    }
}

impl Chunker for ParagraphChunker {
    fn chunk(&self, text: &str) -> Vec<String> {
        let mut chunks = Vec::new();
        let paragraphs: Vec<&str> = text
            .split("\n\n")
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();

        let mut current_chunk = String::new();
        for p in paragraphs {
            if current_chunk.is_empty() {
                current_chunk.push_str(p);
            } else if current_chunk.len() + p.len() + 2 <= self.max_chars {
                current_chunk.push_str("\n\n");
                current_chunk.push_str(p);
            } else {
                chunks.push(current_chunk.clone());
                // For simplicity and semantic boundary preservation, we start the next chunk
                // with the current paragraph.
                current_chunk = p.to_string();
            }
        }
        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }
        chunks
    }
}
