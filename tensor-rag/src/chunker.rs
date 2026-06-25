use once_cell::sync::Lazy;
use regex::Regex;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChunkLevel {
    Macro,
    Medium,
    Micro,
}

impl ChunkLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            ChunkLevel::Macro => "macro",
            ChunkLevel::Medium => "medium",
            ChunkLevel::Micro => "micro",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextSpan {
    pub text: String,
    pub is_header: bool,
    pub is_new_paragraph: bool,
    pub tokens: usize,
    pub page_number: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HierarchicalChunkItem {
    pub text: String,
    pub embedding_text: String,
    pub chapter_id: String,
    pub header_chain: Vec<String>,
    pub page_number: u32,
    pub chunk_index: i32,
    pub parent_chunk_index: Option<i32>,
    pub macro_index: Option<i32>,
    pub medium_index: Option<i32>,
    pub level: ChunkLevel,
    pub zone_keywords: Vec<String>,
    pub entities: Vec<String>,
}

pub struct HierarchicalChunker {
    pub macro_tokens: usize,
    pub macro_overlap: usize,
    pub medium_tokens: usize,
    pub medium_overlap: usize,
    pub micro_tokens: usize,
    pub micro_overlap: usize,
}

impl Default for HierarchicalChunker {
    fn default() -> Self {
        Self {
            macro_tokens: 2048,
            macro_overlap: 400,
            medium_tokens: 512,
            medium_overlap: 128,
            micro_tokens: 128,
            micro_overlap: 32,
        }
    }
}

impl HierarchicalChunker {
    pub fn chunk(&self, text: &str, filename: &str) -> Vec<HierarchicalChunkItem> {
        let cleaned = clean_markdown(&normalize_document_layout(text));
        let spans = segments_to_spans(&cleaned);
        let macros = assemble_level(
            &spans,
            self.macro_tokens,
            self.macro_overlap,
            ChunkLevel::Macro,
        );
        let total_macros = macros.len();
        let mut result = Vec::new();

        for (macro_pos, mut macro_chunk) in macros.into_iter().enumerate() {
            macro_chunk.chunk_index = result.len() as i32;
            macro_chunk.zone_keywords = zone_keywords(macro_pos, total_macros);
            finish_chunk(&mut macro_chunk, filename);
            let macro_index = macro_chunk.chunk_index;
            let macro_spans = segments_to_spans(&macro_chunk.text);
            result.push(macro_chunk.clone());

            let mediums = assemble_level(
                &macro_spans,
                self.medium_tokens,
                self.medium_overlap,
                ChunkLevel::Medium,
            );
            for mut medium_chunk in mediums {
                medium_chunk.chunk_index = result.len() as i32;
                medium_chunk.parent_chunk_index = Some(macro_index);
                medium_chunk.macro_index = Some(macro_index);
                medium_chunk.zone_keywords = zone_keywords(macro_pos, total_macros);
                finish_chunk(&mut medium_chunk, filename);
                let medium_index = medium_chunk.chunk_index;
                let medium_spans = segments_to_spans(&medium_chunk.text);
                result.push(medium_chunk.clone());

                let micros = assemble_level(
                    &medium_spans,
                    self.micro_tokens,
                    self.micro_overlap,
                    ChunkLevel::Micro,
                );
                for mut micro_chunk in micros {
                    micro_chunk.chunk_index = result.len() as i32;
                    micro_chunk.parent_chunk_index = Some(medium_index);
                    micro_chunk.macro_index = Some(macro_index);
                    micro_chunk.medium_index = Some(medium_index);
                    micro_chunk.zone_keywords = zone_keywords(macro_pos, total_macros);
                    finish_chunk(&mut micro_chunk, filename);
                    result.push(micro_chunk);
                }
            }
        }

        result
    }
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
                        if current_len > (self.chunk_size as f64 * 0.4) as usize
                            || !next_chapter_id.is_empty()
                        {
                            emit_chunk(
                                &current_buffer,
                                &current_chapter_id,
                                &mut chunk_index_counter,
                            );

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
                        emit_chunk(
                            &current_buffer,
                            &current_chapter_id,
                            &mut chunk_index_counter,
                        );

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
            emit_chunk(
                &current_buffer,
                &current_chapter_id,
                &mut chunk_index_counter,
            );
        }

        chunks
    }
}

pub fn clean_markdown(raw: &str) -> String {
    static ESCAPED_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\\([^\n])").unwrap());
    static STYLE_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"[*_]{1,3}([^*_\n]+)[*_]{1,3}").unwrap());
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
        "four" | "fourth" | "четыре" | "четвертая" | "четвертый" => {
            Some("4".to_string())
        }
        "five" | "fifth" | "пять" | "пятая" | "пятый" => Some("5".to_string()),
        "six" | "sixth" | "шесть" | "шестая" | "шестой" => Some("6".to_string()),
        "seven" | "seventh" | "семь" | "седьмая" | "седьмой" => {
            Some("7".to_string())
        }
        "eight" | "eighth" | "восемь" | "восьмая" | "восьмой" => {
            Some("8".to_string())
        }
        "nine" | "nineth" | "ninth" | "девять" | "девятая" | "девятый" => {
            Some("9".to_string())
        }
        "ten" | "tenth" | "десять" | "десятая" | "десятый" => {
            Some("10".to_string())
        }
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

fn normalize_document_layout(text: &str) -> String {
    static HYPHEN_JOIN_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(\p{L})-\s*\n\s*(\p{L})").unwrap());
    HYPHEN_JOIN_RE.replace_all(text.trim(), "$1$2").into_owned()
}

fn segments_to_spans(text: &str) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    let mut paragraph_lines: Vec<String> = Vec::new();

    let flush_paragraph = |spans: &mut Vec<TextSpan>, paragraph_lines: &mut Vec<String>| {
        if paragraph_lines.is_empty() {
            return;
        }
        let paragraph = paragraph_lines.join("\n");
        paragraph_lines.clear();
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            return;
        }

        if is_markdown_table(paragraph) {
            spans.push(TextSpan {
                text: paragraph.to_string(),
                is_header: false,
                is_new_paragraph: true,
                tokens: count_tokens(paragraph).max(1),
                page_number: 0,
            });
            return;
        }

        for (idx, sentence) in segment_sentences(paragraph).into_iter().enumerate() {
            spans.push(TextSpan {
                tokens: count_tokens(&sentence).max(1),
                text: sentence,
                is_header: false,
                is_new_paragraph: idx == 0,
                page_number: 0,
            });
        }
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush_paragraph(&mut spans, &mut paragraph_lines);
            continue;
        }

        if is_implicit_header(trimmed) {
            flush_paragraph(&mut spans, &mut paragraph_lines);
            spans.push(TextSpan {
                text: trimmed.to_string(),
                is_header: true,
                is_new_paragraph: true,
                tokens: count_tokens(trimmed).max(1),
                page_number: 0,
            });
        } else {
            paragraph_lines.push(trimmed.to_string());
        }
    }

    flush_paragraph(&mut spans, &mut paragraph_lines);
    spans
}

fn assemble_level(
    spans: &[TextSpan],
    target_tokens: usize,
    overlap_tokens: usize,
    level: ChunkLevel,
) -> Vec<HierarchicalChunkItem> {
    let mut chunks = Vec::new();
    let mut buffer: Vec<TextSpan> = Vec::new();
    let mut tokens = 0usize;
    let mut header_stack: Vec<(usize, String)> = Vec::new();
    let mut current_headers = Vec::new();
    let mut chapter_id = "1".to_string();

    for span in spans {
        if span.is_header {
            let extracted_chapter = extract_chapter_from_header(&span.text);
            if !buffer.is_empty() && extracted_chapter.is_some() {
                emit_hierarchical_chunk(&mut chunks, &buffer, &chapter_id, &current_headers, level);
                buffer.clear();
                tokens = 0;
            } else if !buffer.is_empty() && tokens >= target_tokens / 3 {
                emit_hierarchical_chunk(&mut chunks, &buffer, &chapter_id, &current_headers, level);
                trim_overlap(&mut buffer, &mut tokens, overlap_tokens);
            }
            let level_num = get_header_level(&span.text);
            header_stack.retain(|(existing, _)| *existing < level_num);
            header_stack.push((level_num, strip_header_marker(&span.text)));
            current_headers = header_stack.iter().map(|(_, h)| h.clone()).collect();
            if let Some(extracted) = extracted_chapter {
                chapter_id = extracted;
            }
        }

        if tokens + span.tokens > target_tokens && !buffer.is_empty() {
            emit_hierarchical_chunk(&mut chunks, &buffer, &chapter_id, &current_headers, level);
            trim_overlap(&mut buffer, &mut tokens, overlap_tokens);
        }

        tokens += span.tokens;
        buffer.push(span.clone());
    }

    if !buffer.is_empty() {
        emit_hierarchical_chunk(&mut chunks, &buffer, &chapter_id, &current_headers, level);
    }

    chunks
}

fn emit_hierarchical_chunk(
    chunks: &mut Vec<HierarchicalChunkItem>,
    buffer: &[TextSpan],
    chapter_id: &str,
    header_chain: &[String],
    level: ChunkLevel,
) {
    let text = join_spans(buffer);
    if is_meaningless(&text) {
        return;
    }
    chunks.push(HierarchicalChunkItem {
        text,
        embedding_text: String::new(),
        chapter_id: chapter_id.to_string(),
        header_chain: header_chain.to_vec(),
        page_number: buffer.first().map(|s| s.page_number).unwrap_or(0),
        chunk_index: 0,
        parent_chunk_index: None,
        macro_index: None,
        medium_index: None,
        level,
        zone_keywords: Vec::new(),
        entities: Vec::new(),
    });
}

fn trim_overlap(buffer: &mut Vec<TextSpan>, tokens: &mut usize, overlap_tokens: usize) {
    let mut overlap = 0usize;
    let mut start = buffer.len();
    for (idx, span) in buffer.iter().enumerate().rev() {
        if overlap + span.tokens > overlap_tokens {
            break;
        }
        overlap += span.tokens;
        start = idx;
    }
    *buffer = buffer[start..].to_vec();
    *tokens = overlap;
}

fn join_spans(buffer: &[TextSpan]) -> String {
    let mut out = String::new();
    for (idx, span) in buffer.iter().enumerate() {
        if idx > 0 {
            if span.is_new_paragraph || buffer[idx - 1].is_header {
                out.push_str("\n\n");
            } else {
                out.push(' ');
            }
        }
        out.push_str(&span.text);
    }
    out
}

fn finish_chunk(chunk: &mut HierarchicalChunkItem, filename: &str) {
    let (max_chars, max_entities) = match chunk.level {
        ChunkLevel::Macro => (12_000, 32),
        ChunkLevel::Medium => (8_000, 24),
        ChunkLevel::Micro => (2_000, 8),
    };
    chunk.entities =
        crate::entity::extract_entities_with_limits(&chunk.text, max_chars, max_entities);
    let chain = if chunk.header_chain.is_empty() {
        String::new()
    } else {
        format!(" Context: {}.", chunk.header_chain.join(" > "))
    };
    chunk.embedding_text = format!(
        "File: {}. Level: {}. Zone: {}.{} {}",
        filename,
        chunk.level.as_str(),
        chunk.zone_keywords.join(", "),
        chain,
        chunk.text
    );
}

fn zone_keywords(index: usize, total: usize) -> Vec<String> {
    if total <= 1 {
        return vec![
            "Beginning".to_string(),
            "First Page".to_string(),
            "Last Page".to_string(),
        ];
    }
    let ratio = index as f32 / (total.saturating_sub(1) as f32);
    let mut zones = if ratio <= 0.15 {
        vec!["Beginning", "Start", "Introduction"]
    } else if ratio <= 0.40 {
        vec!["Early Context", "Development"]
    } else if ratio <= 0.60 {
        vec!["Middle", "Midpoint", "Body"]
    } else if ratio <= 0.85 {
        vec!["Climax", "Conflict", "Resolution Phase"]
    } else {
        vec!["Ending", "Conclusion", "Finale"]
    }
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    if index == 0 {
        zones.push("First Page".to_string());
    }
    if index + 1 == total {
        zones.push("Last Page".to_string());
    }
    zones
}

fn is_markdown_table(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines.len() > 1
        && lines
            .iter()
            .all(|line| line.trim().starts_with('|') && line.trim().ends_with('|'))
}

fn is_implicit_header(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || Regex::new(r"^\d+\.\s").unwrap().is_match(trimmed)
    {
        return false;
    }
    if trimmed.starts_with('#') {
        return true;
    }
    is_structural_heading_marker(trimmed) || is_standalone_terminal_heading(trimmed)
}

fn get_header_level(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.starts_with('#') {
        return trimmed.chars().take_while(|c| *c == '#').count().max(1);
    }
    let upper = trimmed.to_uppercase();
    if upper.starts_with("CHAPTER")
        || upper.starts_with("PART")
        || upper.starts_with("BOOK")
        || upper.starts_with("ГЛАВА")
        || upper.starts_with("ЧАСТЬ")
        || upper.starts_with("КНИГА")
    {
        1
    } else if is_standalone_terminal_heading(trimmed) {
        1
    } else {
        2
    }
}

fn strip_header_marker(text: &str) -> String {
    text.trim().trim_start_matches('#').trim().to_string()
}

fn is_structural_heading_marker(text: &str) -> bool {
    static STRUCTURAL_HEADING_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"(?ix)
            ^\s*
            (?:
                (?:chapter|part|book)\s+(?:\d+|[ivxlcdm]+|one|first|two|second|three|third|four|fourth|five|fifth|six|sixth|seven|seventh|eight|eighth|nine|ninth|ten|tenth)
                |
                (?:глава|часть|книга|раздел)\s+(?:\d+|[ivxlcdm]+|один|первая|первый|два|вторая|второй|три|третья|третий|четыре|четвертая|четвертый|пять|пятая|пятый|шесть|шестая|шестой|семь|седьмая|седьмой|восемь|восьмая|восьмой|девять|девятая|девятый|десять|десятая|десятый)
            )
            \.?
            (?:\s*[:.-]\s*[\p{L}\p{N}][^\n]{0,80})?
            \s*$
            ",
        )
        .unwrap()
    });
    STRUCTURAL_HEADING_RE.is_match(text)
}

fn is_standalone_terminal_heading(text: &str) -> bool {
    let normalized = text
        .trim()
        .trim_matches(|c: char| c == '.' || c == ':' || c == '-' || c.is_whitespace())
        .to_uppercase();
    matches!(
        normalized.as_str(),
        "EPILOGUE" | "EPILOG" | "PROLOGUE" | "PROLOG" | "ЭПИЛОГ" | "ПРОЛОГ"
    )
}

fn extract_chapter_from_header(text: &str) -> Option<String> {
    static CHAPTER_EXTRACT_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"(?i)(?:Chapter|Part|Глава|Часть)\s+((?:\d+|\b[IVXLCDM]+\b|[a-zA-Z\p{Cyrillic}]+))",
        )
        .unwrap()
    });
    CHAPTER_EXTRACT_REGEX
        .captures(text)
        .and_then(|caps| caps.get(1))
        .map(|m| parse_text_number(m.as_str()).unwrap_or_else(|| normalize_chapter_id(m.as_str())))
}

#[cfg(test)]
mod hierarchical_tests {
    use super::*;

    #[test]
    fn hierarchical_chunker_emits_three_levels_with_links() {
        let text = "# Chapter 1\n\nAlice Liddell met White Rabbit. The story starts here.\n\n## Section\n\nMore context follows. Even more context follows.";
        let chunker = HierarchicalChunker {
            macro_tokens: 64,
            macro_overlap: 8,
            medium_tokens: 24,
            medium_overlap: 4,
            micro_tokens: 10,
            micro_overlap: 2,
        };
        let chunks = chunker.chunk(text, "alice.md");

        assert!(chunks.iter().any(|c| c.level == ChunkLevel::Macro));
        assert!(
            chunks
                .iter()
                .any(|c| c.level == ChunkLevel::Medium && c.parent_chunk_index.is_some())
        );
        assert!(
            chunks
                .iter()
                .any(|c| c.level == ChunkLevel::Micro && c.medium_index.is_some())
        );
        assert!(
            chunks
                .iter()
                .any(|c| c.embedding_text.contains("File: alice.md"))
        );
    }

    #[test]
    fn hierarchical_chunker_detects_single_newline_chapter_headers() {
        let text = "Конец предыдущей главы.\n## ГЛАВА 27\nУтро началось в Блоке-7. Герой проснулся.\n## ГЛАВА 28\nСледующая глава началась позже.";
        let chunker = HierarchicalChunker {
            macro_tokens: 64,
            macro_overlap: 8,
            medium_tokens: 24,
            medium_overlap: 4,
            micro_tokens: 10,
            micro_overlap: 2,
        };

        let chunks = chunker.chunk(text, "book.md");

        assert!(chunks.iter().any(|c| c.chapter_id == "27"));
        assert!(chunks.iter().any(|c| c.chapter_id == "28"));
        assert!(
            chunks
                .iter()
                .filter(|c| c.chapter_id == "27")
                .all(|c| c.header_chain.iter().any(|h| h.contains("ГЛАВА 27")))
        );
        assert!(
            chunks
                .iter()
                .filter(|c| c.chapter_id == "27")
                .all(|c| !c.text.contains("Конец предыдущей главы."))
        );
    }

    #[test]
    fn hierarchical_chunker_does_not_promote_body_all_caps_lines() {
        let text = "# Утро.\n\n## ГЛАВА 1\n\nОн проснулся.\n\nОСНОВНОЙ НОСИТЕЛЬ\n\nПозиция была стабильной.\n\n## ГЛАВА 2\n\nСледующая глава началась позже.";
        let chunker = HierarchicalChunker {
            macro_tokens: 64,
            macro_overlap: 8,
            medium_tokens: 24,
            medium_overlap: 4,
            micro_tokens: 10,
            micro_overlap: 2,
        };

        let chunks = chunker.chunk(text, "utro.md");

        assert!(
            chunks
                .iter()
                .filter(|c| c.chapter_id == "1")
                .filter(|c| c.text.contains("Он проснулся") || c.text.contains("Позиция была"))
                .any(|c| c.header_chain.iter().any(|h| h == "ГЛАВА 1"))
        );
        assert!(
            chunks
                .iter()
                .all(|c| !c.header_chain.iter().any(|h| h == "ОСНОВНОЙ НОСИТЕЛЬ"))
        );
        assert!(chunks.iter().any(|c| c.chapter_id == "2"));
    }

    #[test]
    fn hierarchical_chunker_keeps_epilogue_as_terminal_heading() {
        let text = "## ГЛАВА 41\n\nФинальная глава.\n\nЭПИЛОГ\n\nПосле всего осталась тишина.";
        let chunker = HierarchicalChunker {
            macro_tokens: 64,
            macro_overlap: 8,
            medium_tokens: 24,
            medium_overlap: 4,
            micro_tokens: 10,
            micro_overlap: 2,
        };

        let chunks = chunker.chunk(text, "utro.md");

        assert!(
            chunks
                .iter()
                .any(|c| c.header_chain.iter().any(|h| h == "ЭПИЛОГ"))
        );
    }
}
