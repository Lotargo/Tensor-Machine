use std::collections::HashSet;

pub fn bucket_sort_contexts(mut docs: Vec<(usize, f32, String)>) -> Vec<(usize, f32, String)> {
    docs.sort_by(|a, b| {
        source_key(&a.2)
            .cmp(&source_key(&b.2))
            .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    docs
}

pub fn deduplicate_contexts(docs: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for doc in docs {
        let key = crate::normalizer::normalize_text(&doc).to_lowercase();
        if seen.insert(key) {
            out.push(doc);
        }
    }
    out
}

pub fn prune_query_history(query: &str) -> (String, String) {
    let marker = "> **User History:**";
    if let Some(idx) = query.find(marker) {
        let history = query[..idx].trim().to_string();
        let real_query = query[idx + marker.len()..]
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string();
        return (real_query, history);
    }
    (query.trim().to_string(), String::new())
}

fn source_key(doc: &str) -> String {
    doc.lines()
        .find_map(|line| line.strip_prefix("Source: "))
        .unwrap_or("")
        .to_string()
}
