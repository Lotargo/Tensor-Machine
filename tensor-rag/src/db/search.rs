use super::DbPools;
use super::helpers::{
    build_tsquery_or, escape_like_pattern, extract_chapter_id, json_array_to_strings,
    precise_or_query_phrase, push_header_chain_match, structured_heading_like_patterns,
    structured_heading_terms, tokenize_lexical_query,
};
use super::types::ChunkDetail;
use qdrant_client::qdrant::{
    Condition, FieldCondition, Filter as QFilter, Match, PointId, ScrollPointsBuilder,
    SearchPoints, VectorsOutput, WithVectorsSelector, condition::ConditionOneOf,
    r#match::MatchValue, point_id::PointIdOptions,
};
use sqlx::{Postgres, QueryBuilder, Row};

#[allow(deprecated)]
pub async fn search_qdrant(
    pools: &DbPools,
    query: &ndarray::Array1<f32>,
    filters: &Option<Vec<crate::ir::Filter>>,
    top_k: usize,
) -> Result<(Vec<usize>, Vec<ndarray::Array1<f32>>), String> {
    if pools.qdrant_cb.is_open() {
        return Err("Circuit breaker is open for Qdrant".to_string());
    }
    let client_opt = pools.qdrant_clients.get("default");

    if let Some(client) = client_opt {
        let collection = crate::config::get_qdrant_collection();
        let mut search_points = SearchPoints {
            collection_name: collection.clone(),
            vector: query.to_vec(),
            limit: top_k as u64,
            with_vectors: Some(WithVectorsSelector {
                selector_options: Some(
                    qdrant_client::qdrant::with_vectors_selector::SelectorOptions::Enable(true),
                ),
            }),
            with_payload: None,
            ..Default::default()
        };

        if let Some(filters) = filters {
            let must: Vec<Condition> = filters
                .iter()
                .map(|filter| {
                    let condition_one_of = match filter.operator.as_str() {
                        "==" | "=" => ConditionOneOf::Field(FieldCondition {
                            key: filter.field.clone(),
                            r#match: Some(Match {
                                match_value: match &filter.value {
                                    serde_json::Value::String(s) => {
                                        Some(MatchValue::Keyword(s.clone()))
                                    }
                                    serde_json::Value::Number(n) => {
                                        n.as_i64().map(MatchValue::Integer)
                                    }
                                    serde_json::Value::Bool(b) => Some(MatchValue::Boolean(*b)),
                                    _ => None,
                                },
                            }),
                            ..Default::default()
                        }),
                        ">=" => ConditionOneOf::Field(FieldCondition {
                            key: filter.field.clone(),
                            range: Some(qdrant_client::qdrant::Range {
                                gte: filter.value.as_f64(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ">" => ConditionOneOf::Field(FieldCondition {
                            key: filter.field.clone(),
                            range: Some(qdrant_client::qdrant::Range {
                                gt: filter.value.as_f64(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        "<=" => ConditionOneOf::Field(FieldCondition {
                            key: filter.field.clone(),
                            range: Some(qdrant_client::qdrant::Range {
                                lte: filter.value.as_f64(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        "<" => ConditionOneOf::Field(FieldCondition {
                            key: filter.field.clone(),
                            range: Some(qdrant_client::qdrant::Range {
                                lt: filter.value.as_f64(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        "in" => ConditionOneOf::Field(FieldCondition {
                            key: filter.field.clone(),
                            r#match: Some(Match {
                                match_value: match &filter.value {
                                    serde_json::Value::Array(arr) => {
                                        if let Some(first) = arr.first() {
                                            if first.is_string() {
                                                Some(MatchValue::Keywords(
                                                    qdrant_client::qdrant::RepeatedStrings {
                                                        strings: arr
                                                            .iter()
                                                            .filter_map(|v| {
                                                                v.as_str().map(|s| s.to_string())
                                                            })
                                                            .collect(),
                                                    },
                                                ))
                                            } else if first.is_i64() {
                                                Some(MatchValue::Integers(
                                                    qdrant_client::qdrant::RepeatedIntegers {
                                                        integers: arr
                                                            .iter()
                                                            .filter_map(|v| v.as_i64())
                                                            .collect(),
                                                    },
                                                ))
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    }
                                    _ => None,
                                },
                            }),
                            ..Default::default()
                        }),
                        _ => ConditionOneOf::Field(FieldCondition {
                            key: filter.field.clone(),
                            r#match: Some(Match {
                                match_value: match &filter.value {
                                    serde_json::Value::String(s) => {
                                        Some(MatchValue::Keyword(s.clone()))
                                    }
                                    serde_json::Value::Number(n) => {
                                        n.as_i64().map(MatchValue::Integer)
                                    }
                                    serde_json::Value::Bool(b) => Some(MatchValue::Boolean(*b)),
                                    _ => None,
                                },
                            }),
                            ..Default::default()
                        }),
                    };
                    Condition {
                        condition_one_of: Some(condition_one_of),
                    }
                })
                .collect();
            search_points.filter = Some(QFilter {
                must,
                ..Default::default()
            });
        }

        let response = match client.search_points(search_points).await {
            Ok(res) => res,
            Err(e) => {
                pools.qdrant_cb.record_failure();
                return Err(e.to_string());
            }
        };

        let mut ids = Vec::new();
        let mut vectors = Vec::new();
        for point in &response.result {
            let id = match &point.id {
                Some(PointId {
                    point_id_options: Some(PointIdOptions::Num(n)),
                }) => *n as usize,
                _ => continue,
            };
            ids.push(id);
            if let Some(VectorsOutput {
                vectors_options:
                    Some(qdrant_client::qdrant::vectors_output::VectorsOptions::Vector(v)),
            }) = &point.vectors
            {
                let vec_data =
                    if let Some(qdrant_client::qdrant::vector_output::Vector::Dense(dense)) =
                        &v.vector
                    {
                        dense.data.clone()
                    } else {
                        #[allow(deprecated)]
                        v.data.clone()
                    };
                vectors.push(ndarray::Array1::from(vec_data));
            } else {
                vectors.push(ndarray::Array1::from_elem(
                    crate::config::get_embedding_dim(),
                    0.1,
                ));
            }
        }
        pools.qdrant_cb.record_success();
        Ok((ids, vectors))
    } else {
        // Mock fallback
        let mut ids = Vec::new();
        let mut vectors = Vec::new();
        for i in 0..top_k {
            ids.push(i);
            vectors.push(ndarray::Array1::from_elem(
                crate::config::get_embedding_dim(),
                0.1,
            ));
        }
        pools.qdrant_cb.record_success();
        Ok((ids, vectors))
    }
}

pub async fn fetch_bm25_scores(pools: &DbPools, ids: &[usize], query: &str) -> Vec<f32> {
    if pools.postgres_cb.is_open() {
        return vec![1.0; ids.len()]; // Fallback
    }
    if let Some(pool) = &pools.postgres {
        let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
        let lexical_terms = tokenize_lexical_query(query);
        let tsquery_or = build_tsquery_or(&lexical_terms);
        let phrase_query = precise_or_query_phrase(query);
        let phrase_pattern = format!("%{}%", escape_like_pattern(&phrase_query));
        let opening_lookup = is_opening_lookup_query(query);

        let rows = if !tsquery_or.is_empty() || !phrase_query.is_empty() {
            sqlx::query(
                r#"
                SELECT
                    id,
                    (
                        CASE
                            WHEN $2 <> '' AND lower(text) LIKE lower($3) ESCAPE '\' THEN 10.0
                            ELSE 0.0
                        END
                        + CASE
                            WHEN $2 <> '' THEN ts_rank_cd(to_tsvector('simple', text), websearch_to_tsquery('simple', $2))
                            ELSE 0.0
                        END
                        + CASE
                            WHEN $4 <> '' THEN ts_rank_cd(to_tsvector('simple', text), to_tsquery('simple', $4))
                            ELSE 0.0
                        END
                        + CASE
                            WHEN $5 AND COALESCE(chunk_index, 999999) <= 2 THEN 25.0
                            WHEN $5 AND COALESCE(chunk_index, 999999) <= 8 THEN 12.0
                            WHEN $5 AND COALESCE(chunk_level, '') = 'macro' AND COALESCE(chunk_index, 999999) <= 20 THEN 8.0
                            ELSE 0.0
                        END
                    )::real AS rank
                FROM documents
                WHERE id = ANY($1)
                "#,
            )
            .bind(&ids_i64[..])
            .bind(&phrase_query)
            .bind(&phrase_pattern)
            .bind(&tsquery_or)
            .bind(opening_lookup)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query(
                "SELECT id, 0.0::real as rank 
                 FROM documents 
                 WHERE id = ANY($1)",
            )
            .bind(&ids_i64[..])
            .fetch_all(pool)
            .await
        };

        if let Ok(rows) = rows {
            let mut map = std::collections::HashMap::new();
            for row in rows {
                let id: i64 = row.try_get("id").unwrap_or(-1);
                let score: f32 = row.try_get("rank").unwrap_or(0.0);
                map.insert(id, score);
            }
            pools.postgres_cb.record_success();
            return ids_i64
                .iter()
                .map(|&id| *map.get(&id).unwrap_or(&0.0))
                .collect();
        } else {
            pools.postgres_cb.record_failure();
        }
    }
    vec![0.0; ids.len()]
}

pub async fn search_opening_candidate_ids(
    pools: &DbPools,
    query: &str,
    doc_uuid: Option<&str>,
    limit: usize,
) -> Vec<usize> {
    if !is_opening_lookup_query(query) {
        return Vec::new();
    }

    let Some(pool) = &pools.postgres else {
        return Vec::new();
    };

    let mut builder = QueryBuilder::<Postgres>::new(
        r#"
        SELECT id
        FROM documents
        WHERE 1=1
        "#,
    );

    if let Some(uuid) = doc_uuid {
        builder.push(" AND doc_uuid = ").push_bind(uuid);
    }

    builder.push(
        r#"
        ORDER BY
            chunk_index ASC,
            CASE chunk_level
                WHEN 'macro' THEN 0
                WHEN 'medium' THEN 1
                ELSE 2
            END ASC
        LIMIT "#,
    );
    builder.push_bind(limit as i64);

    match builder.build().fetch_all(pool).await {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|row| row.try_get::<i64, _>("id").ok())
            .map(|id| id as usize)
            .collect(),
        Err(error) => {
            tracing::warn!("Opening candidate search failed: {}", error);
            Vec::new()
        }
    }
}

fn is_opening_lookup_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    let markers = [
        "с чего начинается",
        "начало романа",
        "начинается роман",
        "первые главы",
        "первая глава",
        "первой главе",
        "первые страницы",
        "первая страница",
        "opening",
        "opening scene",
        "beginning",
        "begins",
        "first chapter",
        "first pages",
    ];

    markers.iter().any(|marker| lower.contains(marker))
}

pub async fn fetch_document_chunk_vectors(
    pools: &DbPools,
    doc_uuid: &str,
    limit: usize,
) -> Result<Vec<(usize, Vec<f32>)>, String> {
    if pools.qdrant_cb.is_open() {
        return Err("Circuit breaker is open for Qdrant".to_string());
    }

    let Some(client) = pools.qdrant_clients.get("default") else {
        return Ok(Vec::new());
    };

    let collection = crate::config::get_qdrant_collection();
    let response = client
        .scroll(
            ScrollPointsBuilder::new(collection)
                .filter(QFilter::must([Condition::matches(
                    "doc_uuid",
                    doc_uuid.to_string(),
                )]))
                .limit(limit as u32)
                .with_payload(false)
                .with_vectors(true),
        )
        .await
        .map_err(|error| {
            pools.qdrant_cb.record_failure();
            error.to_string()
        })?;

    let mut chunk_vectors = Vec::new();
    for point in response.result {
        let Some(PointId {
            point_id_options: Some(PointIdOptions::Num(id)),
        }) = point.id
        else {
            continue;
        };
        let Some(vector) = point.vectors.and_then(|vectors| vectors.get_vector()) else {
            continue;
        };
        let qdrant_client::qdrant::vector_output::Vector::Dense(dense) = vector else {
            continue;
        };
        chunk_vectors.push((id as usize, dense.data));
    }

    pools.qdrant_cb.record_success();
    Ok(chunk_vectors)
}

pub async fn search_keyword_candidate_ids(
    pools: &DbPools,
    query: &str,
    doc_uuid: Option<&str>,
    chapter_id: Option<&str>,
    limit: usize,
) -> Vec<usize> {
    let Some(pool) = &pools.postgres else {
        return Vec::new();
    };

    let phrase_query = precise_or_query_phrase(query);
    let lexical_terms = tokenize_lexical_query(query);
    let tsquery_or = build_tsquery_or(&lexical_terms);
    if phrase_query.is_empty() && tsquery_or.is_empty() {
        return Vec::new();
    }

    let phrase_pattern = format!("%{}%", escape_like_pattern(&phrase_query));
    let mut builder = QueryBuilder::<Postgres>::new(
        r#"
        SELECT id
        FROM documents
        WHERE 1=1
        "#,
    );

    if let Some(uuid) = doc_uuid {
        builder.push(" AND doc_uuid = ").push_bind(uuid);
    }
    if let Some(chapter) = chapter_id {
        builder.push(" AND chapter_id = ").push_bind(chapter);
    }
    builder
        .push(" AND (")
        .push("lower(text) LIKE lower(")
        .push_bind(&phrase_pattern)
        .push(") ESCAPE '\\'")
        .push(" OR to_tsvector('simple', text) @@ websearch_to_tsquery('simple', ")
        .push_bind(&phrase_query)
        .push(")")
        .push(" OR to_tsvector('simple', text) @@ to_tsquery('simple', ")
        .push_bind(&tsquery_or)
        .push("))")
        .push(
            r#"
            ORDER BY (
                CASE
                    WHEN lower(text) LIKE lower("#,
        )
        .push_bind(&phrase_pattern)
        .push(
            r#") ESCAPE '\' THEN 100.0 ELSE 0.0 END
                + CASE
                    WHEN "#,
        )
        .push_bind(&phrase_query)
        .push(
            r#" <> '' THEN ts_rank_cd(to_tsvector('simple', text), websearch_to_tsquery('simple', "#,
        )
        .push_bind(&phrase_query)
        .push(
            r#")) ELSE 0.0 END
                + CASE
                    WHEN "#,
        )
        .push_bind(&tsquery_or)
        .push(
            r#" <> '' THEN ts_rank_cd(to_tsvector('simple', text), to_tsquery('simple', "#,
        )
        .push_bind(&tsquery_or)
        .push(
            r#")) ELSE 0.0 END
            ) DESC,
            chunk_index ASC
            LIMIT "#,
        );
    builder.push_bind(limit as i64);

    let query = builder.build();
    match query.fetch_all(pool).await {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|row| row.try_get::<i64, _>("id").ok())
            .map(|id| id as usize)
            .collect(),
        Err(error) => {
            tracing::warn!("Keyword candidate search failed: {}", error);
            Vec::new()
        }
    }
}

pub async fn search_structured_candidate_ids(
    pools: &DbPools,
    query: &str,
    doc_uuid: Option<&str>,
    chapter_id: Option<&str>,
    limit: usize,
) -> Vec<usize> {
    let Some(pool) = &pools.postgres else {
        return Vec::new();
    };

    let heading_patterns = structured_heading_like_patterns(query);
    if chapter_id.is_none() && heading_patterns.is_empty() {
        return Vec::new();
    }

    let mut builder = QueryBuilder::<Postgres>::new(
        r#"
        SELECT id
        FROM documents
        WHERE 1=1
        "#,
    );

    if let Some(uuid) = doc_uuid {
        builder.push(" AND doc_uuid = ").push_bind(uuid);
    }

    builder.push(" AND (");
    let mut has_condition = false;
    if let Some(chapter) = chapter_id {
        builder.push("chapter_id = ").push_bind(chapter);
        has_condition = true;
    }
    if !heading_patterns.is_empty() {
        if has_condition {
            builder.push(" OR ");
        }
        push_header_chain_match(&mut builder, &heading_patterns);
    }
    builder.push(")");

    builder.push(
        r#"
        ORDER BY
            (
                CASE WHEN "#,
    );
    if let Some(chapter) = chapter_id {
        builder.push("chapter_id = ").push_bind(chapter);
    } else {
        builder.push("FALSE");
    }
    builder.push(" THEN 100 ELSE 0 END");
    if !heading_patterns.is_empty() {
        builder.push(" + CASE WHEN ");
        push_header_chain_match(&mut builder, &heading_patterns);
        builder.push(" THEN 50 ELSE 0 END");
    }
    builder.push(
        r#"
            ) DESC,
            CASE chunk_level
                WHEN 'macro' THEN 0
                WHEN 'medium' THEN 1
                ELSE 2
            END ASC,
            chunk_index ASC
        LIMIT "#,
    );
    builder.push_bind(limit as i64);

    let query = builder.build();
    match query.fetch_all(pool).await {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|row| row.try_get::<i64, _>("id").ok())
            .map(|id| id as usize)
            .collect(),
        Err(error) => {
            tracing::warn!("Structured candidate search failed: {}", error);
            Vec::new()
        }
    }
}

pub async fn fetch_metadata(pools: &DbPools, ids: &[usize]) -> Vec<String> {
    if pools.postgres_cb.is_open() {
        return ids
            .iter()
            .map(|id| format!("Metadata for doc {}", id))
            .collect();
    }
    if let Some(pool) = &pools.postgres {
        let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
        let rows = sqlx::query("SELECT id, text FROM documents WHERE id = ANY($1)")
            .bind(&ids_i64[..])
            .fetch_all(pool)
            .await;

        if let Ok(rows) = rows {
            let mut map = std::collections::HashMap::new();
            for row in rows {
                let id: i64 = row.try_get("id").unwrap_or(-1);
                let text: String = row.try_get("text").unwrap_or_default();
                map.insert(id, text);
            }
            pools.postgres_cb.record_success();
            return ids_i64
                .iter()
                .map(|&id| {
                    map.get(&id)
                        .cloned()
                        .unwrap_or_else(|| format!("Metadata for doc {}", id))
                })
                .collect();
        } else {
            pools.postgres_cb.record_failure();
        }
    }
    ids.iter()
        .map(|id| format!("Metadata for doc {}", id))
        .collect()
}

pub async fn fetch_chunk_details(
    pools: &DbPools,
    ids: &[usize],
) -> Result<Vec<ChunkDetail>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
    let rows = sqlx::query(
        "SELECT id, text,
                COALESCE(filename, '') as filename,
                COALESCE(doc_uuid, '') as doc_uuid,
                COALESCE(chapter_id, '') as chapter_id,
                COALESCE(chunk_index, 0) as chunk_index,
                COALESCE(chunk_level, 'medium') as chunk_level,
                parent_chunk_index, macro_index, medium_index,
                COALESCE(header_chain, '[]'::jsonb) as header_chain,
                COALESCE(zone_keywords, '[]'::jsonb) as zone_keywords,
                COALESCE(entities, '[]'::jsonb) as entities
         FROM documents WHERE id = ANY($1)",
    )
    .bind(&ids_i64[..])
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch chunk details: {}", e))?;

    let mut details = Vec::new();
    for row in rows {
        details.push(row_to_chunk_detail(row));
    }
    Ok(details)
}

pub async fn fetch_neighbor_chunks(
    pools: &DbPools,
    seed_details: &[ChunkDetail],
    max_neighbors: usize,
) -> Result<Vec<ChunkDetail>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    if seed_details.is_empty() || max_neighbors == 0 {
        return Ok(Vec::new());
    }

    let mut expanded = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for detail in seed_details {
        if expanded.len() >= max_neighbors {
            break;
        }

        let indices: Vec<i32> = [detail.chunk_index - 1, detail.chunk_index + 1]
            .into_iter()
            .filter(|&i| i >= 0)
            .collect();

        if indices.is_empty() || detail.doc_uuid.is_empty() {
            continue;
        }

        let sql = chunk_detail_select_sql(
            "WHERE doc_uuid = $1 AND chunk_level = $2 AND chunk_index = ANY($3)
             ORDER BY chunk_index ASC",
        );

        let rows = sqlx::query(&sql)
            .bind(&detail.doc_uuid)
            .bind(&detail.chunk_level)
            .bind(&indices)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Failed to fetch neighbor chunks: {}", e))?;

        for row in rows {
            let neighbor = row_to_chunk_detail(row);
            if seen_ids.insert(neighbor.id) && neighbor.id != detail.id {
                expanded.push(neighbor);
                if expanded.len() >= max_neighbors {
                    break;
                }
            }
        }
    }

    Ok(expanded)
}

pub async fn fetch_structured_span_details(
    pools: &DbPools,
    query: &str,
    seed_details: &[ChunkDetail],
    max_spans: usize,
) -> Result<Vec<ChunkDetail>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;

    if seed_details.is_empty() || query.trim().is_empty() || max_spans == 0 {
        return Ok(Vec::new());
    }

    let requested_chapter = extract_chapter_id(query);
    let heading_terms = structured_heading_terms(query);
    if requested_chapter.is_none() && heading_terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut expanded = Vec::new();
    let mut seen_chapters = std::collections::HashSet::new();
    let mut seen_macros = std::collections::HashSet::new();

    for detail in seed_details {
        if expanded.len() >= max_spans {
            break;
        }

        if let Some(chapter) = requested_chapter.as_deref() {
            if !detail.doc_uuid.is_empty()
                && detail.chapter_id == chapter
                && seen_chapters.insert((detail.doc_uuid.clone(), detail.chapter_id.clone()))
            {
                let sql = chunk_detail_select_sql(
                    "WHERE doc_uuid = $1 AND chapter_id = $2 AND chunk_level = 'macro'
                     ORDER BY chunk_index ASC
                     LIMIT $3",
                );
                let rows = sqlx::query(&sql)
                    .bind(&detail.doc_uuid)
                    .bind(&detail.chapter_id)
                    .bind(max_spans as i64)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| format!("Failed to fetch chapter span: {}", e))?;

                for row in rows {
                    expanded.push(row_to_chunk_detail(row));
                    if expanded.len() >= max_spans {
                        break;
                    }
                }
            }
        }

        if expanded.len() >= max_spans || heading_terms.is_empty() {
            continue;
        }

        if header_chain_matches_terms(&detail.header_chain, &heading_terms) {
            let macro_index = if detail.chunk_level == "macro" {
                detail.chunk_index
            } else {
                detail.macro_index.unwrap_or(detail.chunk_index)
            };

            if !detail.doc_uuid.is_empty()
                && seen_macros.insert((detail.doc_uuid.clone(), macro_index))
            {
                let sql = chunk_detail_select_sql(
                    "WHERE doc_uuid = $1 AND chunk_level = 'macro' AND chunk_index = $2
                     ORDER BY chunk_index ASC
                     LIMIT 1",
                );
                let rows = sqlx::query(&sql)
                    .bind(&detail.doc_uuid)
                    .bind(macro_index)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| format!("Failed to fetch heading span: {}", e))?;

                for row in rows {
                    expanded.push(row_to_chunk_detail(row));
                }
            }
        }
    }

    Ok(expanded)
}

fn header_chain_matches_terms(header_chain: &[String], terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let joined = header_chain.join(" ").to_lowercase();
    !joined.is_empty() && terms.iter().all(|term| joined.contains(term))
}

fn chunk_detail_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT id, text,
                COALESCE(filename, '') as filename,
                COALESCE(doc_uuid, '') as doc_uuid,
                COALESCE(chapter_id, '') as chapter_id,
                COALESCE(chunk_index, 0) as chunk_index,
                COALESCE(chunk_level, 'medium') as chunk_level,
                parent_chunk_index, macro_index, medium_index,
                COALESCE(header_chain, '[]'::jsonb) as header_chain,
                COALESCE(zone_keywords, '[]'::jsonb) as zone_keywords,
                COALESCE(entities, '[]'::jsonb) as entities
         FROM documents {}",
        where_clause
    )
}

fn row_to_chunk_detail(row: sqlx::postgres::PgRow) -> ChunkDetail {
    let id: i64 = row.try_get("id").unwrap_or(-1);
    ChunkDetail {
        id: id as usize,
        text: row.try_get("text").unwrap_or_default(),
        filename: row.try_get("filename").unwrap_or_default(),
        doc_uuid: row.try_get("doc_uuid").unwrap_or_default(),
        chapter_id: row.try_get("chapter_id").unwrap_or_default(),
        chunk_index: row.try_get("chunk_index").unwrap_or(0),
        chunk_level: row
            .try_get("chunk_level")
            .unwrap_or_else(|_| "medium".to_string()),
        parent_chunk_index: row.try_get("parent_chunk_index").unwrap_or(None),
        macro_index: row.try_get("macro_index").unwrap_or(None),
        medium_index: row.try_get("medium_index").unwrap_or(None),
        header_chain: json_array_to_strings(
            row.try_get("header_chain").unwrap_or(serde_json::json!([])),
        ),
        zone_keywords: json_array_to_strings(
            row.try_get("zone_keywords")
                .unwrap_or(serde_json::json!([])),
        ),
        entities: json_array_to_strings(row.try_get("entities").unwrap_or(serde_json::json!([]))),
    }
}

pub async fn fetch_neighbors(
    pools: &DbPools,
    keys: &[(String, i32)],
) -> Result<std::collections::HashMap<(String, i32), String>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;
    if keys.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let mut query_str = String::from("SELECT doc_uuid, chunk_index, text FROM documents WHERE ");
    for i in 0..keys.len() {
        if i > 0 {
            query_str.push_str(" OR ");
        }
        query_str.push_str(&format!(
            "(doc_uuid = ${} AND chunk_index = ${})",
            i * 2 + 1,
            i * 2 + 2
        ));
    }

    let mut query = sqlx::query(&query_str);
    for (uuid, idx) in keys {
        query = query.bind(uuid).bind(*idx);
    }

    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch neighbors: {}", e))?;

    let mut map = std::collections::HashMap::new();
    for r in rows {
        let doc_uuid: String = r.try_get("doc_uuid").unwrap_or_default();
        let chunk_index: i32 = r.try_get("chunk_index").unwrap_or(-1);
        let text: String = r.try_get("text").unwrap_or_default();
        map.insert((doc_uuid, chunk_index), text);
    }
    Ok(map)
}

pub async fn fetch_parent_chunks(
    pools: &DbPools,
    keys: &[(String, i32)],
) -> Result<std::collections::HashMap<(String, i32), ChunkDetail>, String> {
    let pool = pools
        .postgres
        .as_ref()
        .ok_or("PostgreSQL pool not initialized")?;
    if keys.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let mut query_str = String::from(
        "SELECT id, text, COALESCE(filename, '') as filename,
                COALESCE(doc_uuid, '') as doc_uuid,
                COALESCE(chapter_id, '') as chapter_id,
                COALESCE(chunk_index, 0) as chunk_index,
                COALESCE(chunk_level, 'medium') as chunk_level,
                parent_chunk_index, macro_index, medium_index,
                COALESCE(header_chain, '[]'::jsonb) as header_chain,
                COALESCE(zone_keywords, '[]'::jsonb) as zone_keywords,
                COALESCE(entities, '[]'::jsonb) as entities
         FROM documents WHERE ",
    );
    for i in 0..keys.len() {
        if i > 0 {
            query_str.push_str(" OR ");
        }
        query_str.push_str(&format!(
            "(doc_uuid = ${} AND chunk_index = ${})",
            i * 2 + 1,
            i * 2 + 2
        ));
    }

    let mut query = sqlx::query(&query_str);
    for (uuid, idx) in keys {
        query = query.bind(uuid).bind(*idx);
    }

    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch parents: {}", e))?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let doc_uuid: String = row.try_get("doc_uuid").unwrap_or_default();
        let chunk_index: i32 = row.try_get("chunk_index").unwrap_or(0);
        map.insert(
            (doc_uuid.clone(), chunk_index),
            ChunkDetail {
                id: row.try_get::<i64, _>("id").unwrap_or(-1) as usize,
                text: row.try_get("text").unwrap_or_default(),
                filename: row.try_get("filename").unwrap_or_default(),
                doc_uuid,
                chapter_id: row.try_get("chapter_id").unwrap_or_default(),
                chunk_index,
                chunk_level: row
                    .try_get("chunk_level")
                    .unwrap_or_else(|_| "medium".to_string()),
                parent_chunk_index: row.try_get("parent_chunk_index").unwrap_or(None),
                macro_index: row.try_get("macro_index").unwrap_or(None),
                medium_index: row.try_get("medium_index").unwrap_or(None),
                header_chain: json_array_to_strings(
                    row.try_get("header_chain").unwrap_or(serde_json::json!([])),
                ),
                zone_keywords: json_array_to_strings(
                    row.try_get("zone_keywords")
                        .unwrap_or(serde_json::json!([])),
                ),
                entities: json_array_to_strings(
                    row.try_get("entities").unwrap_or(serde_json::json!([])),
                ),
            },
        );
    }
    Ok(map)
}

pub async fn fetch_entity_scores(
    pools: &DbPools,
    ids: &[usize],
    query_entities: &[String],
) -> Vec<f32> {
    if ids.is_empty() || query_entities.len() < 2 {
        return vec![0.0; ids.len()];
    }
    let Some(pool) = &pools.postgres else {
        return vec![0.0; ids.len()];
    };
    let ids_i64: Vec<i64> = ids.iter().map(|&x| x as i64).collect();
    let rows = sqlx::query(
        "SELECT id, COALESCE(entities, '[]'::jsonb) as entities FROM documents WHERE id = ANY($1)",
    )
    .bind(&ids_i64[..])
    .fetch_all(pool)
    .await;
    let Ok(rows) = rows else {
        return vec![0.0; ids.len()];
    };
    let wanted: Vec<String> = query_entities
        .iter()
        .map(|s| crate::normalizer::stem_phrase(s))
        .filter(|s| !s.is_empty())
        .collect();
    if wanted.is_empty() {
        return vec![0.0; ids.len()];
    }
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let id: i64 = row.try_get("id").unwrap_or(-1);
        let entities =
            json_array_to_strings(row.try_get("entities").unwrap_or(serde_json::json!([])));
        let hits = entities
            .iter()
            .filter(|entity| {
                let entity_key = crate::normalizer::stem_phrase(entity);
                if entity_key.is_empty() {
                    return false;
                }
                wanted
                    .iter()
                    .any(|wanted| entity_key.contains(wanted) || wanted.contains(&entity_key))
            })
            .count();
        map.insert(id, hits as f32 / wanted.len().max(1) as f32);
    }
    ids_i64
        .iter()
        .map(|id| *map.get(&id).unwrap_or(&0.0))
        .collect()
}

pub async fn search_entity_candidate_ids(
    pools: &DbPools,
    query_entities: &[String],
    doc_uuid: Option<&str>,
    limit: usize,
) -> Vec<usize> {
    if query_entities.len() < 2 {
        return Vec::new();
    }
    let Some(pool) = &pools.postgres else {
        return Vec::new();
    };
    let wanted: Vec<String> = query_entities
        .iter()
        .map(|s| crate::normalizer::stem_phrase(s))
        .filter(|s| !s.is_empty())
        .collect();
    if wanted.is_empty() {
        return Vec::new();
    }
    let rows = if let Some(uuid) = doc_uuid {
        sqlx::query("SELECT id, COALESCE(entities, '[]'::jsonb) as entities FROM documents WHERE doc_uuid = $1 AND jsonb_array_length(COALESCE(entities, '[]'::jsonb)) > 0 LIMIT $2")
            .bind(uuid)
            .bind((limit * 5).max(50) as i64)
            .fetch_all(pool)
            .await
    } else {
        sqlx::query("SELECT id, COALESCE(entities, '[]'::jsonb) as entities FROM documents WHERE jsonb_array_length(COALESCE(entities, '[]'::jsonb)) > 0 LIMIT $1")
            .bind((limit * 5).max(50) as i64)
            .fetch_all(pool)
            .await
    };
    let Ok(rows) = rows else {
        return Vec::new();
    };
    let mut scored = Vec::new();
    for row in rows {
        let id: i64 = row.try_get("id").unwrap_or(-1);
        let entities =
            json_array_to_strings(row.try_get("entities").unwrap_or(serde_json::json!([])));
        let hits = entities
            .iter()
            .filter(|entity| {
                let entity_key = crate::normalizer::stem_phrase(entity);
                if entity_key.is_empty() {
                    return false;
                }
                wanted
                    .iter()
                    .any(|wanted| entity_key.contains(wanted) || wanted.contains(&entity_key))
            })
            .count();
        if hits >= 2 || hits == wanted.len() {
            scored.push((id as usize, hits));
        }
    }
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().take(limit).map(|(id, _)| id).collect()
}
