use crate::db::DbPools;
use std::collections::{HashMap, HashSet};

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

pub async fn build_search_response(
    db_pools: &DbPools,
    indices: Vec<usize>,
    scores: Vec<f32>,
    query: &str,
) -> Result<serde_json::Value, String> {
    if indices.is_empty() {
        return Ok(serde_json::json!({
            "indices": Vec::<usize>::new(),
            "scores": Vec::<f32>::new(),
            "documents": Vec::<String>::new(),
            "token_limit": crate::config::get_search_max_response_tokens(),
            "returned_tokens": 0,
            "warning": serde_json::Value::Null
        }));
    }

    let mut details = db_pools.fetch_chunk_details(&indices).await?;

    let mut score_map = HashMap::new();
    let mut rank_map = HashMap::new();
    for (rank, (&id, &score)) in indices.iter().zip(scores.iter()).enumerate() {
        score_map.insert(id, score);
        rank_map.insert(id, rank);
    }

    let explicit_structured_lookup = is_explicit_structured_lookup(query);
    let precise_lookup_phrase = crate::db::helpers::extract_precise_lookup_phrase(query);
    let original_details = details.clone();
    let original_ids: HashSet<usize> = original_details.iter().map(|detail| detail.id).collect();
    let mut seen_detail_ids: HashSet<usize> = original_ids.clone();

    let max_neighbors = if explicit_structured_lookup {
        0
    } else {
        indices.len().saturating_mul(2).clamp(2, 12)
    };
    let neighbor_details = db_pools
        .fetch_neighbor_chunks(&original_details, max_neighbors)
        .await?;

    for detail in neighbor_details {
        if !seen_detail_ids.insert(detail.id) {
            continue;
        }
        let parent_score = original_details
            .iter()
            .find(|d| d.doc_uuid == detail.doc_uuid && (d.chunk_index - detail.chunk_index).abs() <= 1)
            .and_then(|d| score_map.get(&d.id))
            .copied()
            .unwrap_or(0.0);
        let inherited = parent_score * 0.8;
        score_map.insert(detail.id, inherited);
        rank_map.insert(detail.id, indices.len() + details.len());
        details.push(detail);
    }

    let max_span_chunks = if explicit_structured_lookup {
        indices.len().saturating_mul(4).clamp(8, 40)
    } else {
        0
    };

    let expanded_details = db_pools
        .fetch_structured_span_details(query, &original_details, max_span_chunks)
        .await?;

    for detail in expanded_details {
        if !seen_detail_ids.insert(detail.id) {
            continue;
        }

        let inherited = inherited_structured_score(&detail, &original_details, &score_map);
        score_map.insert(detail.id, inherited);
        rank_map.insert(detail.id, indices.len() + details.len());
        details.push(detail);
    }

    struct ProcessedChunk {
        id: Option<usize>,
        chunk_index: i32,
        text: String,
        header_chain: Vec<String>,
        score: f32,
        rank: usize,
    }

    let mut doc_groups: HashMap<String, (String, Vec<ProcessedChunk>)> = HashMap::new();
    let mut add_to_group = |doc_uuid: String, filename: String, pc: ProcessedChunk| {
        let entry = doc_groups
            .entry(doc_uuid)
            .or_insert_with(|| (filename, Vec::new()));
        entry.1.push(pc);
    };

    for detail in details {
        let score = *score_map.get(&detail.id).unwrap_or(&0.0);
        let rank = *rank_map.get(&detail.id).unwrap_or(&usize::MAX);

        let text = precise_lookup_phrase
            .as_deref()
            .and_then(|phrase| precise_match_excerpt(&detail.text, phrase))
            .unwrap_or_else(|| detail.text.clone());

        let pc = ProcessedChunk {
            id: Some(detail.id),
            chunk_index: detail.chunk_index,
            text,
            header_chain: detail.header_chain.clone(),
            score,
            rank,
        };

        add_to_group(detail.doc_uuid.clone(), detail.filename.clone(), pc);
    }

    struct DocumentGroup {
        _doc_uuid: String,
        filename: String,
        _max_score: f32,
        best_rank: usize,
        chunks: Vec<ProcessedChunk>,
    }

    let mut groups = Vec::new();
    for (doc_uuid, (filename, mut chunks)) in doc_groups {
        chunks.sort_by(|a, b| b.text.len().cmp(&a.text.len()));
        let mut unique_chunks: Vec<ProcessedChunk> = Vec::new();
        for chunk in chunks {
            let is_subset = unique_chunks
                .iter()
                .any(|existing| existing.text.contains(&chunk.text));
            if !is_subset {
                unique_chunks.push(chunk);
            }
        }

        unique_chunks.sort_by_key(|c| c.chunk_index);
        if !explicit_structured_lookup && unique_chunks.len() > 2 {
            unique_chunks.sort_by(|a, b| {
                a.rank.cmp(&b.rank).then_with(|| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            });
            unique_chunks.truncate(2);
            unique_chunks.sort_by_key(|c| c.chunk_index);
        }

        let max_score = unique_chunks.iter().map(|c| c.score).fold(0.0f32, f32::max);
        let best_rank = unique_chunks
            .iter()
            .map(|c| c.rank)
            .min()
            .unwrap_or(usize::MAX);

        groups.push(DocumentGroup {
            _doc_uuid: doc_uuid,
            filename,
            _max_score: max_score,
            best_rank,
            chunks: unique_chunks,
        });
    }

    groups.sort_by_key(|g| g.best_rank);

    let token_limit = crate::config::get_search_max_response_tokens();
    let mut returned_documents = Vec::new();
    let mut returned_indices = Vec::new();
    let mut returned_scores = Vec::new();
    let mut returned_doc_scores = Vec::new();
    let mut total_tokens = 0;
    let mut trimmed = false;

    for g in groups {
        let doc_max_score = g
            .chunks
            .iter()
            .map(|c| c.score)
            .fold(0.0f32, |a, b| a.max(b));
        let mut doc_text = String::new();
        doc_text.push_str(&format!("Source: {}\n", g.filename));

        if let Some(first) = g.chunks.first() {
            if !first.header_chain.is_empty() {
                doc_text.push_str(&format!("Context: {}\n", first.header_chain.join(" > ")));
            }
        }
        doc_text.push('\n');

        let mut last_header_chain: Option<Vec<String>> = None;
        let mut last_text: Option<String> = None;

        for chunk in g.chunks {
            let mut chunk_text = chunk.text.clone();

            if let Some(ref last_headers) = last_header_chain {
                if last_headers != &chunk.header_chain && !chunk.header_chain.is_empty() {
                    doc_text.push_str(&format!(
                        ">>> Context: {} <<<\n",
                        chunk.header_chain.join(" > ")
                    ));
                }
            }

            if let Some(ref prev) = last_text {
                chunk_text = trim_overlap(prev, &chunk_text);
            }

            doc_text.push_str(&chunk_text);
            doc_text.push('\n');

            last_header_chain = Some(chunk.header_chain.clone());
            last_text = Some(chunk.text);

            if let Some(id) = chunk.id {
                returned_indices.push(id);
                returned_scores.push(chunk.score);
            }
        }

        let doc_tokens = crate::chunker::count_tokens(&doc_text);
        if total_tokens + doc_tokens > token_limit {
            trimmed = true;
            break;
        }

        total_tokens += doc_tokens;
        returned_documents.push(doc_text);
        returned_doc_scores.push(doc_max_score);
    }

    let warning = if trimmed {
        Some(format!(
            "Search response was trimmed to {} tokens; trailing results were dropped.",
            token_limit
        ))
    } else {
        None
    };

    Ok(serde_json::json!({
        "indices": returned_indices,
        "scores": returned_scores,
        "document_scores": returned_doc_scores,
        "documents": returned_documents,
        "token_limit": token_limit,
        "returned_tokens": total_tokens,
        "warning": warning
    }))
}

fn inherited_structured_score(
    expanded: &crate::db::types::ChunkDetail,
    seeds: &[crate::db::types::ChunkDetail],
    score_map: &HashMap<usize, f32>,
) -> f32 {
    seeds
        .iter()
        .filter(|seed| {
            seed.doc_uuid == expanded.doc_uuid
                && (seed.chapter_id == expanded.chapter_id
                    || seed.macro_index == Some(expanded.chunk_index)
                    || seed.chunk_index == expanded.chunk_index)
        })
        .filter_map(|seed| score_map.get(&seed.id).copied())
        .fold(0.0f32, f32::max)
        * 0.99
}

fn is_explicit_structured_lookup(query: &str) -> bool {
    let lower = query.to_lowercase();
    [
        "chapter",
        "section",
        "heading",
        "full text",
        "whole chapter",
        "глава",
        "главу",
        "главы",
        "раздел",
        "раздела",
        "заголовок",
        "полный текст",
        "весь текст",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn precise_match_excerpt(text: &str, phrase: &str) -> Option<String> {
    if phrase.trim().chars().count() < 3 {
        return None;
    }

    let text_lower = text.to_lowercase();
    let phrase_lower = phrase.to_lowercase();
    let byte_pos = text_lower.find(&phrase_lower)?;

    if let Some(paragraph) = paragraph_around_byte(text, byte_pos) {
        if paragraph.chars().count() <= 1_500 {
            return Some(paragraph);
        }
    }

    Some(char_window_around_byte(text, byte_pos, 500))
}

fn paragraph_around_byte(text: &str, byte_pos: usize) -> Option<String> {
    let before = &text[..byte_pos];
    let after = &text[byte_pos..];
    let start = before.rfind("\n\n").map(|idx| idx + 2).unwrap_or(0);
    let end = after
        .find("\n\n")
        .map(|idx| byte_pos + idx)
        .unwrap_or(text.len());
    let paragraph = text[start..end].trim();
    if paragraph.is_empty() {
        None
    } else {
        Some(paragraph.to_string())
    }
}

fn char_window_around_byte(text: &str, byte_pos: usize, radius: usize) -> String {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let center = chars
        .iter()
        .position(|(idx, _)| *idx >= byte_pos)
        .unwrap_or(chars.len().saturating_sub(1));
    let start_char = center.saturating_sub(radius);
    let end_char = (center + radius).min(chars.len());

    let start_byte = chars.get(start_char).map(|(idx, _)| *idx).unwrap_or(0);
    let end_byte = chars
        .get(end_char)
        .map(|(idx, _)| *idx)
        .unwrap_or(text.len());
    let excerpt = text[start_byte..end_byte].trim();

    match (start_byte > 0, end_byte < text.len()) {
        (true, true) => format!("...{}...", excerpt),
        (true, false) => format!("...{}", excerpt),
        (false, true) => format!("{}...", excerpt),
        (false, false) => excerpt.to_string(),
    }
}

fn trim_overlap(prev: &str, curr: &str) -> String {
    let prev_chars: Vec<char> = prev.chars().collect();
    let curr_chars: Vec<char> = curr.chars().collect();
    let max_len = prev_chars.len().min(curr_chars.len());

    for len in (10..=max_len).rev() {
        let suffix = &prev_chars[prev_chars.len() - len..];
        let prefix = &curr_chars[..len];
        if suffix == prefix {
            let trimmed: String = curr_chars[len..].iter().collect();
            return trimmed.trim().to_string();
        }
    }
    curr.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_overlap_exact() {
        let prev = "This is a sentence that ends with hello world.";
        let curr = "hello world. And this is a new sentence.";
        let result = trim_overlap(prev, curr);
        assert_eq!(result, "And this is a new sentence.");
    }

    #[test]
    fn test_trim_overlap_no_match() {
        let prev = "This is a sentence that ends with hello.";
        let curr = "world. And this is a new sentence.";
        let result = trim_overlap(prev, curr);
        assert_eq!(result, curr.to_string());
    }

    #[test]
    fn test_trim_overlap_short_match() {
        let prev = "This is a sentence that ends with abc";
        let curr = "abc And this is a new sentence.";
        let result = trim_overlap(prev, curr);
        assert_eq!(result, curr.to_string());
    }

    #[test]
    fn test_precise_match_excerpt_returns_matching_paragraph() {
        let text = "Intro paragraph.\n\nThe important line is here with Alice.\nStill same paragraph.\n\nTail.";
        let excerpt = precise_match_excerpt(text, "important line").unwrap();
        assert_eq!(
            excerpt,
            "The important line is here with Alice.\nStill same paragraph."
        );
    }

    #[tokio::test]
    async fn test_build_search_response_empty() {
        let db_pools = DbPools::new();
        let result = build_search_response(&db_pools, vec![], vec![], "")
            .await
            .unwrap();
        assert_eq!(result["indices"].as_array().unwrap().len(), 0);
        assert_eq!(result["documents"].as_array().unwrap().len(), 0);
    }
}
