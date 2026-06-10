use once_cell::sync::Lazy;
use regex::Regex;
use sqlx::{Postgres, QueryBuilder};

pub fn json_array_to_strings(value: serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn tokenize_lexical_query(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = query
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .map(|word| word.replace(|c: char| !c.is_alphanumeric(), ""))
        .filter(|word| !word.is_empty())
        .collect();

    for term in crate::query_expansion::expand_lexical_terms(&terms) {
        if !terms.iter().any(|existing| existing == &term) {
            terms.push(term);
        }
    }

    terms
}

pub fn extract_precise_lookup_phrase(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(trimmed) {
        return Some(quoted);
    }

    let lower = trimmed.to_lowercase();
    let markers = [
        "exact phrase",
        "exact string",
        "find phrase",
        "find string",
        "find line",
        "quote",
        "цитата",
        "цитату",
        "фраза",
        "фразу",
        "строка",
        "строку",
        "найди строку",
        "найди фразу",
        "найти строку",
        "найти фразу",
    ];

    for marker in markers {
        if let Some(pos) = lower.find(marker) {
            let after = trimmed[pos + marker.len()..]
                .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, ':' | '-' | '—'))
                .trim();
            if after.chars().count() >= 3 {
                return Some(after.to_string());
            }
        }
    }

    None
}

pub fn precise_or_query_phrase(query: &str) -> String {
    extract_precise_lookup_phrase(query).unwrap_or_else(|| query.trim().to_string())
}

fn extract_quoted_phrase(query: &str) -> Option<String> {
    let pairs = [('"', '"'), ('\'', '\''), ('«', '»'), ('“', '”'), ('„', '“')];
    for (open, close) in pairs {
        let Some(start) = query.find(open) else {
            continue;
        };
        let after_start = start + open.len_utf8();
        let Some(relative_end) = query[after_start..].find(close) else {
            continue;
        };
        let end = after_start + relative_end;
        let candidate = query[after_start..end].trim();
        if candidate.chars().count() >= 3 {
            return Some(candidate.to_string());
        }
    }
    None
}

pub fn structured_heading_like_patterns(query: &str) -> Vec<String> {
    structured_heading_terms(query)
        .into_iter()
        .take(6)
        .map(|term| format!("%{}%", escape_like_pattern(&term)))
        .collect()
}

pub fn structured_heading_terms(query: &str) -> Vec<String> {
    tokenize_lexical_query(query)
        .into_iter()
        .map(|term| term.to_lowercase())
        .filter(|term| term.chars().count() >= 3)
        .filter(|term| !is_structured_lookup_noise_term(term))
        .collect()
}

pub fn extract_chapter_id(query: &str) -> Option<String> {
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"(?i)(?:Chapter|Part|Глава|Часть)\s+((?:\d+|\b[IVXLCDM]+\b|[a-zA-Z\p{Cyrillic}]+))",
        )
        .unwrap()
    });
    RE.captures(query).and_then(|caps| caps.get(1)).map(|m| {
        crate::chunker::parse_text_number(m.as_str())
            .unwrap_or_else(|| crate::chunker::normalize_chapter_id(m.as_str()))
    })
}

pub fn is_structured_lookup_noise_term(term: &str) -> bool {
    matches!(
        term,
        "chapter"
            | "part"
            | "section"
            | "heading"
            | "find"
            | "show"
            | "open"
            | "text"
            | "full"
            | "глава"
            | "главу"
            | "главы"
            | "часть"
            | "раздел"
            | "раздела"
            | "заголовок"
            | "найди"
            | "найти"
            | "покажи"
            | "открой"
            | "полный"
            | "текст"
    )
}

pub fn push_header_chain_match<'args>(
    builder: &mut QueryBuilder<'args, Postgres>,
    heading_patterns: &'args [String],
) {
    builder.push(
        r#"
        EXISTS (
            SELECT 1
            FROM jsonb_array_elements_text(COALESCE(header_chain, '[]'::jsonb)) AS h(header_text)
            WHERE "#,
    );
    for (idx, pattern) in heading_patterns.iter().enumerate() {
        if idx > 0 {
            builder.push(" AND ");
        }
        builder
            .push("lower(h.header_text) LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\'");
    }
    builder.push(")");
}

pub fn build_tsquery_or(terms: &[String]) -> String {
    terms.join(" | ")
}

pub fn escape_like_pattern(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_heading_patterns_drop_navigation_words() {
        let patterns = structured_heading_like_patterns("show full text of chapter Rabbit Hole");
        assert_eq!(patterns, vec!["%rabbit%".to_string(), "%hole%".to_string()]);
    }

    #[test]
    fn structured_heading_patterns_escape_like_wildcards() {
        let patterns = structured_heading_like_patterns("open section 100%_ready");
        assert_eq!(patterns, vec!["%100ready%".to_string()]);
    }

    #[test]
    fn extract_chapter_id_normalizes_numbers() {
        assert_eq!(extract_chapter_id("show Chapter IV"), Some("4".to_string()));
        assert_eq!(
            extract_chapter_id("show Chapter five"),
            Some("5".to_string())
        );
    }

    #[test]
    fn extract_chapter_id_russian() {
        assert_eq!(extract_chapter_id("Глава 27"), Some("27".to_string()));
        assert_eq!(extract_chapter_id("Часть вторая"), Some("2".to_string()));
    }

    #[test]
    fn precise_lookup_extracts_quoted_phrase() {
        assert_eq!(
            extract_precise_lookup_phrase("find line \"Alice met the Rabbit\""),
            Some("Alice met the Rabbit".to_string())
        );
        assert_eq!(
            extract_precise_lookup_phrase("найди строку «Алиса увидела кролика»"),
            Some("Алиса увидела кролика".to_string())
        );
    }

    #[test]
    fn precise_lookup_extracts_marker_tail() {
        assert_eq!(
            extract_precise_lookup_phrase("exact phrase: Alice met the Rabbit"),
            Some("Alice met the Rabbit".to_string())
        );
    }

    #[test]
    fn lexical_query_tokenization_has_no_builtin_domain_expansion() {
        let terms = tokenize_lexical_query("what song did the duchess sing?");
        assert_eq!(terms, vec!["what", "song", "did", "the", "duchess", "sing"]);
    }
}
