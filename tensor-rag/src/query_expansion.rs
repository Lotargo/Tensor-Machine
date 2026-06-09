use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

static TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\p{L}\d]+").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryExpansionMode {
    Off,
    Dictionary,
}

impl QueryExpansionMode {
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "dictionary" | "dict" | "synonyms" => Self::Dictionary,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SynonymDictionary {
    groups: Vec<Vec<String>>,
    term_to_group_indices: HashMap<String, Vec<usize>>,
}

impl SynonymDictionary {
    pub fn from_groups(groups: Vec<Vec<String>>) -> Self {
        let mut normalized_groups = Vec::new();
        let mut term_to_group_indices: HashMap<String, Vec<usize>> = HashMap::new();

        for group in groups {
            let mut seen = HashSet::new();
            let normalized = group
                .into_iter()
                .map(|term| normalize_term(&term))
                .filter(|term| !term.is_empty())
                .filter(|term| seen.insert(term.clone()))
                .collect::<Vec<_>>();

            if normalized.len() < 2 {
                continue;
            }

            let idx = normalized_groups.len();
            for term in &normalized {
                term_to_group_indices
                    .entry(term.clone())
                    .or_default()
                    .push(idx);
            }
            normalized_groups.push(normalized);
        }

        Self {
            groups: normalized_groups,
            term_to_group_indices,
        }
    }

    pub fn expand_terms(&self, terms: &[String], max_terms: usize) -> Vec<String> {
        if max_terms == 0 {
            return Vec::new();
        }

        let existing = terms
            .iter()
            .map(|term| normalize_term(term))
            .filter(|term| !term.is_empty())
            .collect::<HashSet<_>>();
        let mut expanded = Vec::new();
        let mut seen = existing.clone();

        for term in &existing {
            let Some(indices) = self.term_to_group_indices.get(term) else {
                continue;
            };

            for &idx in indices {
                let Some(group) = self.groups.get(idx) else {
                    continue;
                };
                for candidate in group {
                    for token in tokenize(candidate) {
                        if seen.insert(token.clone()) {
                            expanded.push(token);
                            if expanded.len() >= max_terms {
                                return expanded;
                            }
                        }
                    }
                }
            }
        }

        expanded
    }
}

pub fn expand_lexical_terms(terms: &[String]) -> Vec<String> {
    match crate::config::get_query_expansion_mode() {
        QueryExpansionMode::Off => Vec::new(),
        QueryExpansionMode::Dictionary => {
            dictionary().expand_terms(terms, crate::config::get_query_expansion_max_terms())
        }
    }
}

pub fn parse_synonym_dictionary(input: &str) -> SynonymDictionary {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(input) {
        return parse_json_dictionary(value);
    }

    let groups = input
        .lines()
        .filter_map(parse_synonym_line)
        .collect::<Vec<_>>();
    SynonymDictionary::from_groups(groups)
}

fn dictionary() -> &'static SynonymDictionary {
    static DICTIONARY: OnceLock<SynonymDictionary> = OnceLock::new();
    DICTIONARY.get_or_init(load_dictionary)
}

fn load_dictionary() -> SynonymDictionary {
    let paths = crate::config::get_query_expansion_dictionary_paths();
    tracing::info!(paths = ?paths, "Loading query expansion dictionary");
    let mut total_groups = 0usize;
    let mut total_terms = 0usize;
    let groups: Vec<Vec<String>> = paths
        .into_iter()
        .filter_map(|path| {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let parsed = parse_synonym_dictionary(&content);
                    total_groups += parsed.groups.len();
                    total_terms += parsed.term_to_group_indices.len();
                    tracing::info!(path = %path, groups = parsed.groups.len(), terms = parsed.term_to_group_indices.len(), "Loaded dictionary file");
                    Some(parsed.groups)
                }
                Err(e) => {
                    tracing::warn!(path = %path, error = %e, "Failed to load dictionary file");
                    None
                }
            }
        })
        .flatten()
        .collect();
    let dict = SynonymDictionary::from_groups(groups);
    tracing::info!(groups = total_groups, unique_terms = total_terms, "Query expansion dictionary loaded");
    dict
}

fn parse_json_dictionary(value: serde_json::Value) -> SynonymDictionary {
    let mut groups = Vec::new();

    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(group) = json_group(item) {
                    groups.push(group);
                }
            }
        }
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let mut group = vec![key];
                match value {
                    serde_json::Value::Array(items) => {
                        group.extend(
                            items
                                .into_iter()
                                .filter_map(|item| item.as_str().map(ToString::to_string)),
                        );
                    }
                    serde_json::Value::String(item) => group.push(item),
                    _ => {}
                }
                groups.push(group);
            }
        }
        _ => {}
    }

    SynonymDictionary::from_groups(groups)
}

fn json_group(value: serde_json::Value) -> Option<Vec<String>> {
    value.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(ToString::to_string))
            .collect::<Vec<_>>()
    })
}

fn parse_synonym_line(line: &str) -> Option<Vec<String>> {
    let trimmed = line.split('#').next().unwrap_or("").trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((lhs, rhs)) = trimmed.split_once("=>") {
        let mut group = split_terms(lhs);
        group.extend(split_terms(rhs));
        return Some(group);
    }

    Some(split_terms(trimmed))
}

fn split_terms(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn tokenize(input: &str) -> Vec<String> {
    TOKEN_RE
        .find_iter(input)
        .map(|m| m.as_str().to_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
}

fn normalize_term(input: &str) -> String {
    tokenize(input).join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_object_dictionary() {
        let dictionary = parse_synonym_dictionary(
            r#"{
                "sing": ["chant", "vocalize"],
                "lullaby": ["cradle song"]
            }"#,
        );

        let expanded = dictionary.expand_terms(&["sing".to_string()], 8);
        assert!(expanded.contains(&"chant".to_string()));
        assert!(expanded.contains(&"vocalize".to_string()));
    }

    #[test]
    fn parses_line_dictionary_and_splits_multiword_expansions() {
        let dictionary = parse_synonym_dictionary("song, tune, melody\nlullaby => cradle song");

        let expanded = dictionary.expand_terms(&["lullaby".to_string()], 8);
        assert!(expanded.contains(&"cradle".to_string()));
        assert!(expanded.contains(&"song".to_string()));
    }

    #[test]
    fn respects_expansion_limit() {
        let dictionary = parse_synonym_dictionary("alpha, beta, gamma, delta");

        let expanded = dictionary.expand_terms(&["alpha".to_string()], 2);
        assert_eq!(expanded.len(), 2);
    }
}
