use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{BTreeSet, HashMap};

static NAMED_ENTITY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(?:[\p{Lu}][\p{Ll}\p{M}]+|[А-ЯЁ][а-яё]+)(?:\s+(?:[\p{Lu}][\p{Ll}\p{M}]+|[А-ЯЁ][а-яё]+)){0,2}\b").unwrap()
});
static TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\p{L}\d]+").unwrap());

pub fn extract_entities(text: &str) -> Vec<String> {
    let mut entities = BTreeSet::new();
    for mat in NAMED_ENTITY_RE.find_iter(text) {
        let candidate = mat.as_str().trim();
        if candidate.chars().count() < 3 || is_sentence_start(text, mat.start()) {
            continue;
        }
        if crate::normalizer::is_stopword(candidate) {
            continue;
        }
        entities.insert(candidate.to_string());
    }
    entities.into_iter().take(32).collect()
}

pub fn extract_query_entities(query: &str) -> Vec<String> {
    let mut entities = extract_entities(query);
    if entities.len() < 2 {
        for token in TOKEN_RE.find_iter(query) {
            let token = token.as_str();
            if token.chars().count() >= 4 && !crate::normalizer::is_stopword(token) {
                entities.push(token.to_string());
            }
        }
    }
    merge_entities(entities)
}

pub fn merge_entities(entities: Vec<String>) -> Vec<String> {
    let mut by_key: HashMap<String, String> = HashMap::new();
    for entity in entities {
        let key = crate::normalizer::stem_phrase(&entity);
        if key.is_empty() {
            continue;
        }
        by_key
            .entry(key)
            .and_modify(|existing| {
                if entity.chars().count() > existing.chars().count() {
                    *existing = entity.clone();
                }
            })
            .or_insert(entity);
    }
    let mut merged: Vec<String> = by_key.into_values().collect();
    merged.sort_by_key(|s| s.to_lowercase());
    merged.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    merged
}

fn is_sentence_start(text: &str, start: usize) -> bool {
    let prefix = &text[..start];
    let trimmed = prefix.trim_end();
    trimmed.is_empty() || trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_multiword_entities() {
        let entities = extract_entities("We met Alice Liddell near White Rabbit. Then she left.");
        assert!(entities.iter().any(|e| e == "Alice Liddell"));
        assert!(entities.iter().any(|e| e == "White Rabbit"));
    }
}
