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
    query
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .map(|word| word.replace(|c: char| !c.is_alphanumeric(), ""))
        .filter(|word| !word.is_empty())
        .collect()
}

pub fn structured_heading_like_patterns(query: &str) -> Vec<String> {
    tokenize_lexical_query(query)
        .into_iter()
        .map(|term| term.to_lowercase())
        .filter(|term| term.chars().count() >= 3)
        .filter(|term| !is_structured_lookup_noise_term(term))
        .take(6)
        .map(|term| format!("%{}%", escape_like_pattern(&term)))
        .collect()
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
}
