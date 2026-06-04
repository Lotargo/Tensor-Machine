use regex::Regex;
use once_cell::sync::Lazy;

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SmartChunkItem {
    pub text: String,
    pub chapter_id: String,
    pub chunk_index: i32,
}

pub struct SmartChunker {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
}

impl SmartChunker {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap,
        }
    }

    pub fn chunk(&self, text: &str) -> Vec<SmartChunkItem> {
        let cleaned = clean_markdown(text);

        static HEADER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(#+)\s+(.*)").unwrap());
        static CHAPTER_EXTRACT_REGEX: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"(?i)(?:Chapter|Part|Глава|Часть)\s+((?:\d+|\b[IVXLCDM]+\b|[a-zA-Z\p{Cyrillic}]+))").unwrap()
        });

        #[derive(Clone)]
        struct TextSpan {
            text: String,
            is_header: bool,
            len: usize,
        }

        let mut current_buffer: Vec<TextSpan> = Vec::new();
        let mut current_len = 0;
        let mut current_chapter_id = "1".to_string();
        let mut chunks = Vec::new();
        let mut chunk_index_counter = 0;

        let mut emit_chunk = |buffer: &[TextSpan], chap_id: &str, counter: &mut i32| {
            if buffer.is_empty() {
                return;
            }
            let mut sb = String::new();
            for (i, span) in buffer.iter().enumerate() {
                sb.push_str(&span.text);
                if i < buffer.len() - 1 {
                    if span.is_header {
                        sb.push_str("\n\n");
                    } else {
                        sb.push_str(" ");
                    }
                }
            }
            let chunk_text = sb;
            if !is_meaningless(&chunk_text) {
                chunks.push(SmartChunkItem {
                    text: chunk_text,
                    chapter_id: chap_id.to_string(),
                    chunk_index: *counter,
                });
                *counter += 1;
            }
        };

        // Split text by lines
        for line in cleaned.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Split into paragraphs by double newline (if present in the line) or just handle as single line
            let paragraphs = line.split("\n\n");
            for p in paragraphs {
                let p = p.trim();
                if p.is_empty() {
                    continue;
                }

                let is_header = HEADER_REGEX.is_match(p);
                let spans = if is_header {
                    vec![TextSpan {
                        text: p.to_string(),
                        is_header: true,
                        len: p.chars().count(),
                    }]
                } else {
                    segment_sentences(p)
                        .into_iter()
                        .map(|s| TextSpan {
                            len: s.chars().count(),
                            text: s,
                            is_header: false,
                        })
                        .collect()
                };

                for span in spans {
                    let mut next_chapter_id = String::new();
                    if span.is_header {
                        if let Some(caps) = CHAPTER_EXTRACT_REGEX.captures(&span.text) {
                            if let Some(m) = caps.get(1) {
                                let raw = m.as_str();
                                if let Some(num) = parse_text_number(raw) {
                                    next_chapter_id = num;
                                } else {
                                    next_chapter_id = normalize_chapter_id(raw);
                                }
                            }
                        }
                    }

                    if span.is_header && !current_buffer.is_empty() {
                        if current_len > (self.chunk_size as f64 * 0.4) as usize || !next_chapter_id.is_empty() {
                            emit_chunk(&current_buffer, &current_chapter_id, &mut chunk_index_counter);
                            
                            let mut overlap_len = 0;
                            let mut overlap_start_idx = current_buffer.len();
                            for (j, s) in current_buffer.iter().enumerate().rev() {
                                if overlap_len + s.len > self.chunk_overlap {
                                    break;
                                }
                                overlap_len += s.len;
                                overlap_start_idx = j;
                            }
                            
                            current_buffer = current_buffer[overlap_start_idx..].to_vec();
                            current_len = overlap_len;
                        }
                    }

                    if !next_chapter_id.is_empty() {
                        current_chapter_id = next_chapter_id;
                    }

                    if current_len + span.len > self.chunk_size && !current_buffer.is_empty() {
                        emit_chunk(&current_buffer, &current_chapter_id, &mut chunk_index_counter);
                        
                        let mut overlap_len = 0;
                        let mut overlap_start_idx = current_buffer.len();
                        for (j, s) in current_buffer.iter().enumerate().rev() {
                            if overlap_len + s.len > self.chunk_overlap {
                                break;
                            }
                            overlap_len += s.len;
                            overlap_start_idx = j;
                        }
                        
                        current_buffer = current_buffer[overlap_start_idx..].to_vec();
                        current_len = overlap_len;
                    }

                    current_len += span.len;
                    current_buffer.push(span);
                }
            }
        }

        if !current_buffer.is_empty() {
            emit_chunk(&current_buffer, &current_chapter_id, &mut chunk_index_counter);
        }

        chunks
    }
}

pub fn clean_markdown(raw: &str) -> String {
    static ESCAPED_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\\([^\n])").unwrap());
    static STYLE_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"[*_]{1,3}([^*_\n]+)[*_]{1,3}").unwrap());
    static SPACES_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]+").unwrap());

    let text = ESCAPED_REGEX.replace_all(raw, "$1");
    let text = STYLE_REGEX.replace_all(&text, "$1");
    
    let mut clean_lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let cleaned = SPACES_REGEX.replace_all(trimmed, " ");
        clean_lines.push(cleaned.into_owned());
    }
    clean_lines.join("\n")
}

pub fn segment_sentences(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"([?!.]\s+)|(\n+)").unwrap());
    
    let mut sentences = Vec::new();
    let mut last_idx = 0;
    
    for mat in RE.find_iter(text) {
        let start = mat.start();
        let end = mat.end();
        let match_str = &text[start..end];
        let mut is_split = false;
        
        if match_str.contains('\n') {
            is_split = true;
        } else if end < text.len() {
            if let Some(next_char) = text[end..].chars().next() {
                if next_char.is_uppercase() || next_char.is_numeric() {
                    is_split = true;
                }
            }
        } else {
            is_split = true;
        }
        
        if is_split {
            let segment = text[last_idx..end].trim();
            if !segment.is_empty() {
                sentences.push(segment.to_string());
            }
            last_idx = end;
        }
    }
    
    if last_idx < text.len() {
        let tail = text[last_idx..].trim();
        if !tail.is_empty() {
            sentences.push(tail.to_string());
        }
    }
    
    sentences
}

pub fn parse_text_number(raw: &str) -> Option<String> {
    let raw = raw.trim().to_lowercase();
    match raw.as_str() {
        "one" | "first" | "один" | "первая" | "первый" => Some("1".to_string()),
        "two" | "second" | "два" | "вторая" | "второй" => Some("2".to_string()),
        "three" | "third" | "три" | "третья" | "третий" => Some("3".to_string()),
        "four" | "fourth" | "четыре" | "четвертая" | "четвертый" => Some("4".to_string()),
        "five" | "fifth" | "пять" | "пятая" | "пятый" => Some("5".to_string()),
        "six" | "sixth" | "шесть" | "шестая" | "шестой" => Some("6".to_string()),
        "seven" | "seventh" | "семь" | "седьмая" | "седьмой" => Some("7".to_string()),
        "eight" | "eighth" | "восемь" | "восьмая" | "восьмой" => Some("8".to_string()),
        "nine" | "nineth" | "ninth" | "девять" | "девятая" | "девятый" => Some("9".to_string()),
        "ten" | "tenth" | "десять" | "десятая" | "десятый" => Some("10".to_string()),
        _ => None,
    }
}

pub fn normalize_chapter_id(raw: &str) -> String {
    let raw = raw.trim().to_uppercase();
    if let Ok(val) = raw.parse::<i32>() {
        return val.to_string();
    }
    // Roman numerals conversion
    let mut roman_map = std::collections::HashMap::new();
    roman_map.insert('I', 1);
    roman_map.insert('V', 5);
    roman_map.insert('X', 10);
    roman_map.insert('L', 50);
    roman_map.insert('C', 100);
    roman_map.insert('D', 500);
    roman_map.insert('M', 1000);

    let mut total = 0;
    let mut last_val = 0;
    let mut valid_roman = true;

    for c in raw.chars().rev() {
        if let Some(&val) = roman_map.get(&c) {
            if val < last_val {
                total -= val;
            } else {
                total += val;
            }
            last_val = val;
        } else {
            valid_roman = false;
            break;
        }
    }

    if valid_roman && total > 0 {
        total.to_string()
    } else {
        raw.to_lowercase()
    }
}

pub fn is_meaningless(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 10 {
        return true;
    }
    !trimmed.chars().any(|c| c.is_alphanumeric())
}
