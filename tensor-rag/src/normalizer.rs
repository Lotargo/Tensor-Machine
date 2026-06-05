use once_cell::sync::Lazy;
use regex::Regex;
use rust_stemmers::{Algorithm, Stemmer};
use std::collections::{HashMap, HashSet};

static HYPHEN_JOIN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\p{L})-\s*\n\s*(\p{L})").unwrap());
static WS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
static TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\p{L}\d]+").unwrap());
static ENGLISH_STEMMER: Lazy<Stemmer> = Lazy::new(|| Stemmer::create(Algorithm::English));
static RUSSIAN_STEMMER: Lazy<Stemmer> = Lazy::new(|| Stemmer::create(Algorithm::Russian));

static STOPWORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "a",
        "an",
        "and",
        "are",
        "as",
        "at",
        "be",
        "by",
        "for",
        "from",
        "in",
        "into",
        "is",
        "it",
        "of",
        "on",
        "or",
        "that",
        "the",
        "this",
        "to",
        "with",
        "и",
        "в",
        "во",
        "не",
        "что",
        "он",
        "на",
        "я",
        "с",
        "со",
        "как",
        "а",
        "то",
        "все",
        "она",
        "так",
        "его",
        "но",
        "да",
        "ты",
        "к",
        "у",
        "же",
        "вы",
        "за",
        "бы",
        "по",
        "только",
        "ее",
        "мне",
        "было",
        "вот",
        "от",
        "меня",
        "еще",
        "нет",
        "о",
        "из",
        "ему",
        "теперь",
        "когда",
        "даже",
        "ну",
        "вдруг",
        "ли",
        "если",
        "уже",
        "или",
        "ни",
        "быть",
        "был",
        "него",
        "до",
        "вас",
        "нибудь",
        "опять",
        "уж",
        "вам",
        "ведь",
        "там",
        "потом",
        "себя",
    ]
    .into_iter()
    .collect()
});

pub fn normalize_text(input: &str) -> String {
    let text = HYPHEN_JOIN_RE.replace_all(input, "$1$2");
    WS_RE.replace_all(text.trim(), " ").into_owned()
}

pub fn tokens(input: &str) -> Vec<String> {
    TOKEN_RE
        .find_iter(input)
        .map(|m| m.as_str().to_lowercase())
        .filter(|token| !is_stopword(token))
        .collect()
}

pub fn is_stopword(input: &str) -> bool {
    STOPWORDS.contains(input.to_lowercase().as_str())
}

pub fn stem_phrase(input: &str) -> String {
    tokens(input)
        .into_iter()
        .map(|token| stem_token(&token))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn top_terms(input: &str, limit: usize) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for token in tokens(input) {
        *counts.entry(stem_token(&token)).or_insert(0) += 1;
    }
    let mut terms: Vec<_> = counts.into_iter().collect();
    terms.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    terms.truncate(limit);
    terms
}

fn stem_token(token: &str) -> String {
    let lower = token.to_lowercase();
    if lower.chars().any(is_cyrillic) {
        return RUSSIAN_STEMMER.stem(&lower).into_owned();
    }
    if lower.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return ENGLISH_STEMMER.stem(&lower).into_owned();
    }
    let suffixes = [
        "иями", "ями", "ами", "ого", "ему", "ыми", "ими", "ing", "tion", "ions", "ed", "es", "s",
        "ый", "ий", "ая", "ое", "ые", "ов", "ев", "ам", "ям", "ах", "ях", "ом", "ем",
    ];
    for suffix in suffixes {
        if lower.chars().count() > suffix.chars().count() + 3 && lower.ends_with(suffix) {
            return lower.trim_end_matches(suffix).to_string();
        }
    }
    lower
}

fn is_cyrillic(ch: char) -> bool {
    ('\u{0400}'..='\u{04ff}').contains(&ch)
        || ('\u{0500}'..='\u{052f}').contains(&ch)
        || ('\u{2de0}'..='\u{2dff}').contains(&ch)
        || ('\u{a640}'..='\u{a69f}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_russian_word_forms_with_library_stemmer() {
        assert_eq!(stem_phrase("кошками кошке кошка"), "кошк кошк кошк");
    }

    #[test]
    fn stems_english_word_forms_with_library_stemmer() {
        assert_eq!(stem_phrase("running runs runner"), "run run runner");
    }

    #[test]
    fn joins_hyphenated_line_breaks() {
        assert_eq!(normalize_text("инфор-\nмация"), "информация");
    }
}
