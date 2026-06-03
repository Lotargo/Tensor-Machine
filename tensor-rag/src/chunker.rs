pub trait Chunker: Send + Sync {
    fn chunk(&self, text: &str) -> Vec<String>;
}

pub fn count_tokens(text: &str) -> usize {
    if text.trim().is_empty() {
        return 0;
    }

    let tokenizer_model = crate::config::get_tokenizer_model();
    chunkedrs::chunk(text)
        .max_tokens(usize::MAX)
        .model(&tokenizer_model)
        .split()
        .iter()
        .map(|chunk| chunk.token_count)
        .sum()
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

pub struct TokenChunker {
    pub max_tokens: usize,
    pub overlap: usize,
    pub model: String,
}

impl TokenChunker {
    pub fn new(max_tokens: usize, overlap: usize, model: &str) -> Self {
        Self {
            max_tokens,
            overlap,
            model: model.to_string(),
        }
    }
}

impl Default for TokenChunker {
    fn default() -> Self {
        Self::new(512, 64, "o200k_harmony")
    }
}

impl Chunker for TokenChunker {
    fn chunk(&self, text: &str) -> Vec<String> {
        let chunks = chunkedrs::chunk(text)
            .max_tokens(self.max_tokens)
            .overlap(self.overlap)
            .model(&self.model)
            .split();
        chunks.into_iter().map(|c| c.content).collect()
    }
}
