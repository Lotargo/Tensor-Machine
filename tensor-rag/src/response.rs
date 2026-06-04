use std::collections::{HashMap, HashSet};
use crate::db::DbPools;

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

    // 1. Fetch details for retrieved chunks
    let details = db_pools.fetch_chunk_details(&indices).await?;

    // Create a map from chunk ID to its search score and rank
    let mut score_map = HashMap::new();
    let mut rank_map = HashMap::new();
    for (rank, (&id, &score)) in indices.iter().zip(scores.iter()).enumerate() {
        score_map.insert(id, score);
        rank_map.insert(id, rank);
    }

    // 2. Summary Injection
    // Identify top unique documents (up to 10 unique documents)
    let mut unique_docs = Vec::new();
    for d in &details {
        if !d.doc_uuid.is_empty() && !unique_docs.contains(&d.doc_uuid) {
            unique_docs.push(d.doc_uuid.clone());
            if unique_docs.len() >= 10 {
                break;
            }
        }
    }

    // For each unique doc, check if we have a chunk with index 0
    let mut summary_keys = Vec::new();
    for doc_uuid in &unique_docs {
        let has_summary = details.iter().any(|d| d.doc_uuid == *doc_uuid && d.chunk_index == 0);
        if !has_summary {
            summary_keys.push((doc_uuid.clone(), 0));
        }
    }

    // Fetch summary chunks (chunk_index = 0)
    let summary_chunks = if !summary_keys.is_empty() {
        db_pools.fetch_parent_chunks(&summary_keys).await.unwrap_or_default()
    } else {
        HashMap::new()
    };

    // 3. Collect neighbor keys and parent keys for all retrieved chunks
    let mut neighbor_keys = Vec::new();
    let mut parent_keys = Vec::new();
    for d in &details {
        if !d.doc_uuid.is_empty() {
            neighbor_keys.push((d.doc_uuid.clone(), d.chunk_index - 1));
            neighbor_keys.push((d.doc_uuid.clone(), d.chunk_index + 1));
            if let Some(macro_idx) = d.macro_index {
                parent_keys.push((d.doc_uuid.clone(), macro_idx));
            } else if let Some(parent_idx) = d.parent_chunk_index {
                parent_keys.push((d.doc_uuid.clone(), parent_idx));
            }
        }
    }

    let neighbors_map = if !neighbor_keys.is_empty() {
        db_pools.fetch_neighbors(&neighbor_keys).await.unwrap_or_default()
    } else {
        HashMap::new()
    };

    let parents_map = if !parent_keys.is_empty() {
        db_pools.fetch_parent_chunks(&parent_keys).await.unwrap_or_default()
    } else {
        HashMap::new()
    };

    // 4. Construct a list of ProcessedChunks
    struct ProcessedChunk {
        id: Option<usize>,
        chunk_index: i32,
        text: String,
        header_chain: Vec<String>,
        is_summary: bool,
        score: f32,
        rank: usize,
    }

    let mut doc_groups: HashMap<String, (String, Vec<ProcessedChunk>)> = HashMap::new(); // key: doc_uuid -> (filename, chunks)

    // Helper to add processed chunk to its document group
    let mut add_to_group = |doc_uuid: String, filename: String, pc: ProcessedChunk| {
        let entry = doc_groups.entry(doc_uuid.clone()).or_insert_with(|| (filename, Vec::new()));
        entry.1.push(pc);
    };

    // Add injected summaries
    for ((doc_uuid, _), summary_detail) in summary_chunks {
        let pc = ProcessedChunk {
            id: Some(summary_detail.id),
            chunk_index: 0,
            text: summary_detail.text.clone(),
            header_chain: summary_detail.header_chain.clone(),
            is_summary: true,
            score: 0.0,
            rank: usize::MAX, // default rank for injected summaries
        };
        add_to_group(doc_uuid, summary_detail.filename, pc);
    }

    // Process retrieved chunks
    for detail in details {
        let prev_key = (detail.doc_uuid.clone(), detail.chunk_index - 1);
        let next_key = (detail.doc_uuid.clone(), detail.chunk_index + 1);
        let parent_key = detail
            .macro_index
            .or(detail.parent_chunk_index)
            .map(|idx| (detail.doc_uuid.clone(), idx));

        let prev_text = neighbors_map.get(&prev_key);
        let next_text = neighbors_map.get(&next_key);

        let mut context_window = if let Some(parent_key) = parent_key {
            parents_map
                .get(&parent_key)
                .map(|parent| parent.text.clone())
                .unwrap_or_else(|| detail.text.clone())
        } else {
            detail.text.clone()
        };

        if let Some(prev) = prev_text {
            if !prev.trim().is_empty() {
                context_window = format!("{} [...] {}", prev.trim(), context_window);
            }
        }
        if let Some(next) = next_text {
            if !next.trim().is_empty() {
                context_window = format!("{} [...] {}", context_window, next.trim());
            }
        }

        let score = *score_map.get(&detail.id).unwrap_or(&0.0);
        let rank = *rank_map.get(&detail.id).unwrap_or(&usize::MAX);

        let pc = ProcessedChunk {
            id: Some(detail.id),
            chunk_index: detail.chunk_index,
            text: context_window,
            header_chain: detail.header_chain.clone(),
            is_summary: false,
            score,
            rank,
        };

        add_to_group(detail.doc_uuid.clone(), detail.filename.clone(), pc);
    }

    // 5. Structure Document Groups
    struct DocumentGroup {
        _doc_uuid: String,
        filename: String,
        _max_score: f32,
        best_rank: usize,
        chunks: Vec<ProcessedChunk>,
    }

    let mut groups = Vec::new();
    for (doc_uuid, (filename, mut chunks)) in doc_groups {
        // Subset Deduplication:
        // Sort by text length descending so we check larger ones first
        chunks.sort_by(|a, b| b.text.len().cmp(&a.text.len()));
        let mut unique_chunks: Vec<ProcessedChunk> = Vec::new();
        for chunk in chunks {
            let is_subset = unique_chunks.iter().any(|existing| {
                existing.text.contains(&chunk.text)
            });
            if !is_subset {
                unique_chunks.push(chunk);
            }
        }

        // Sort chunks by chunk_index ascending (reading order)
        unique_chunks.sort_by_key(|c| c.chunk_index);

        let max_score = unique_chunks.iter().map(|c| c.score).fold(0.0f32, f32::max);
        let best_rank = unique_chunks.iter().map(|c| c.rank).min().unwrap_or(usize::MAX);

        groups.push(DocumentGroup {
            _doc_uuid: doc_uuid,
            filename,
            _max_score: max_score,
            best_rank,
            chunks: unique_chunks,
        });
    }

    // Bucket Sort: Sort documents by best_rank (original relevance rank) ascending
    groups.sort_by_key(|g| g.best_rank);

    // 6. Format containerized output and apply token limit
    let token_limit = crate::config::get_search_max_response_tokens();
    let mut returned_documents = Vec::new();
    let mut returned_indices = Vec::new();
    let mut returned_scores = Vec::new();
    let mut total_tokens = 0;
    let mut trimmed = false;

    // First generate Table of Contents (Index)
    let mut toc = String::new();
    toc.push_str("=== ДОКУМЕНТЫ В ПОИСКЕ ===\n");
    for (idx, g) in groups.iter().enumerate() {
        toc.push_str(&format!("[DOC_{}] {}\n", idx + 1, g.filename));
    }
    toc.push_str("==========================\n\n");

    let toc_tokens = crate::chunker::count_tokens(&toc);
    total_tokens += toc_tokens;

    for (doc_idx, g) in groups.into_iter().enumerate() {
        let mut doc_text = String::new();
        let doc_num = doc_idx + 1;
        doc_text.push_str(&format!(">>> НАЧАЛО ДОКУМЕНТА [DOC_{}] <<<\n", doc_num));
        doc_text.push_str(&format!("Title: {}\n", g.filename));

        if let Some(first) = g.chunks.first() {
            if !first.header_chain.is_empty() {
                doc_text.push_str(&format!("Context: {}\n", first.header_chain.join(" > ")));
            }
        }
        doc_text.push_str("\n");

        let mut last_index: Option<i32> = None;
        let mut last_header_chain: Option<Vec<String>> = None;
        let mut last_text: Option<String> = None;

        for chunk in g.chunks {
            let mut chunk_text = chunk.text.clone();
            
            if chunk.is_summary {
                doc_text.push_str(">>> Summary: <<<\n");
            } else {
                // Gap detection
                if let Some(last_idx) = last_index {
                    if chunk.chunk_index != last_idx + 1 {
                        doc_text.push_str("\n... [Proximity Gap] ...\n\n");
                    }
                }
                // Context change detection
                if let Some(ref last_headers) = last_header_chain {
                    if last_headers != &chunk.header_chain && !chunk.header_chain.is_empty() {
                        doc_text.push_str(&format!(">>> Context: {} <<<\n", chunk.header_chain.join(" > ")));
                    }
                }
                // Trim overlap with previous chunk
                if let Some(ref prev) = last_text {
                    chunk_text = trim_overlap(prev, &chunk_text);
                }
            }

            doc_text.push_str(&chunk_text);
            doc_text.push_str("\n");

            last_index = Some(chunk.chunk_index);
            last_header_chain = Some(chunk.header_chain.clone());
            last_text = Some(chunk.text); // keep original expanded text for overlap check

            if let Some(id) = chunk.id {
                returned_indices.push(id);
                returned_scores.push(chunk.score);
            }
        }

        doc_text.push_str(&format!(">>> КОНЕЦ ДОКУМЕНТА [DOC_{}] <<<\n", doc_num));

        let doc_tokens = crate::chunker::count_tokens(&doc_text);
        if total_tokens + doc_tokens > token_limit {
            trimmed = true;
            break;
        }

        total_tokens += doc_tokens;
        returned_documents.push(doc_text);
    }

    // Prepend Table of Contents if we returned any documents
    if !returned_documents.is_empty() {
        returned_documents.insert(0, toc);
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
        "documents": returned_documents,
        "token_limit": token_limit,
        "returned_tokens": total_tokens,
        "warning": warning
    }))
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
        // Overlap is less than 10 characters ("abc" is 3 chars), so it should not be trimmed.
        let result = trim_overlap(prev, curr);
        assert_eq!(result, curr.to_string());
    }

    #[tokio::test]
    async fn test_build_search_response_empty() {
        let db_pools = DbPools::new();
        let result = build_search_response(&db_pools, vec![], vec![]).await.unwrap();
        assert_eq!(result["indices"].as_array().unwrap().len(), 0);
        assert_eq!(result["documents"].as_array().unwrap().len(), 0);
    }
}
