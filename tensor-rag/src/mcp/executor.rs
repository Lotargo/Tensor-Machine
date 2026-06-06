use once_cell::sync::Lazy;
use regex::Regex;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

fn extract_chapter_id(query: &str) -> Option<String> {
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"(?i)(?:Chapter|Part|Глава|Часть)\s+((?:\d+|\b[IVXLCDM]+\b|[a-zA-Z\p{Cyrillic}]+))",
        )
        .unwrap()
    });
    if let Some(caps) = RE.captures(query) {
        if let Some(m) = caps.get(1) {
            let raw = m.as_str();
            if let Some(num) = crate::chunker::parse_text_number(raw) {
                return Some(num);
            } else {
                return Some(crate::chunker::normalize_chapter_id(raw));
            }
        }
    }
    None
}

// async executor
pub fn execute_program<'a>(
    program: &'a crate::ir::Program,
    vm: &'a mut crate::vm::VM,
    db_pools: &'a crate::db::DbPools,
    instruction_count: &'a mut usize,
    max_instructions: usize,
) -> Pin<Box<dyn Future<Output = Result<Option<crate::vm::Value>, String>> + Send + 'a>> {
    Box::pin(async move {
        for op in &program.instructions {
            if *instruction_count >= max_instructions {
                return Err(format!(
                    "Execution error: Instruction limit exceeded ({})",
                    max_instructions
                ));
            }
            *instruction_count += 1;

            match &op.op {
                crate::ir::OpCode::RetrieveQdrant {
                    query,
                    filters,
                    top_k,
                } => {
                    // Generate real query embedding via local model server (with E5 "query: " prefixing)
                    let query_vec = db_pools.get_embedding(query, true).await?;
                    vm.query_vector = query_vec.clone();

                    let mut updated_filters = filters.clone().unwrap_or_default();
                    let chapter_filter = extract_chapter_id(query);
                    if let Some(ref chap) = chapter_filter {
                        updated_filters.push(crate::ir::Filter {
                            field: "chapter_id".to_string(),
                            operator: "==".to_string(),
                            value: serde_json::Value::String(chap.clone()),
                        });
                    }
                    let filters_opt = if !updated_filters.is_empty() {
                        Some(updated_filters)
                    } else {
                        None
                    };

                    let (mut ids, mut vectors) = db_pools
                        .search_qdrant(&query_vec, &filters_opt, *top_k)
                        .await?;

                    let postgres_doc_uuid = filters_opt.as_ref().and_then(|f_list| {
                        f_list.iter().find_map(|filter| {
                            if filter.field == "doc_uuid"
                                && (filter.operator == "==" || filter.operator == "=")
                            {
                                filter.value.as_str().map(ToString::to_string)
                            } else {
                                None
                            }
                        })
                    });

                    let fallback_limit = ((*top_k + 4) / 5).clamp(10, 30);
                    let mut pg_ids = db_pools
                        .search_keyword_candidate_ids(
                            query,
                            postgres_doc_uuid.as_deref(),
                            chapter_filter.as_deref(),
                            fallback_limit,
                        )
                        .await;
                    let structured_ids = db_pools
                        .search_structured_candidate_ids(
                            query,
                            postgres_doc_uuid.as_deref(),
                            chapter_filter.as_deref(),
                            fallback_limit,
                        )
                        .await;
                    for id in structured_ids {
                        if !pg_ids.contains(&id) {
                            pg_ids.push(id);
                        }
                    }

                    if !pg_ids.is_empty() {
                        if let Some(client) = db_pools.qdrant_clients.get("default") {
                            let collection = crate::config::get_qdrant_collection();
                            let point_ids: Vec<qdrant_client::qdrant::PointId> = pg_ids
                                .iter()
                                .filter(|id| !ids.contains(id))
                                .map(|&id| qdrant_client::qdrant::PointId {
                                    point_id_options: Some(
                                        qdrant_client::qdrant::point_id::PointIdOptions::Num(
                                            id as u64,
                                        ),
                                    ),
                                })
                                .collect();

                            if !point_ids.is_empty() {
                                let request = qdrant_client::qdrant::GetPoints {
                                    collection_name: collection.clone(),
                                    ids: point_ids,
                                    with_vectors: Some(qdrant_client::qdrant::WithVectorsSelector {
                                        selector_options: Some(
                                            qdrant_client::qdrant::with_vectors_selector::SelectorOptions::Enable(true),
                                        ),
                                    }),
                                    with_payload: Some(qdrant_client::qdrant::WithPayloadSelector {
                                        selector_options: Some(
                                            qdrant_client::qdrant::with_payload_selector::SelectorOptions::Enable(false),
                                        ),
                                    }),
                                    ..Default::default()
                                };
                                if let Ok(response) = client.get_points(request).await {
                                    for point in response.result {
                                        let id = match &point.id {
                                            Some(qdrant_client::qdrant::PointId {
                                                point_id_options: Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)),
                                            }) => *n as usize,
                                            _ => continue,
                                        };

                                        let vec_data = if let Some(qdrant_client::qdrant::VectorsOutput {
                                            vectors_options: Some(qdrant_client::qdrant::vectors_output::VectorsOptions::Vector(v)),
                                        }) = &point.vectors {
                                            if let Some(qdrant_client::qdrant::vector_output::Vector::Dense(dense)) = &v.vector {
                                                dense.data.clone()
                                            } else {
                                                #[allow(deprecated)]
                                                v.data.clone()
                                            }
                                        } else {
                                            vec![0.1; crate::config::get_embedding_dim()]
                                        };

                                        ids.push(id);
                                        vectors.push(ndarray::Array1::from(vec_data));
                                    }
                                }
                            }
                        }
                    }

                    for id in pg_ids {
                        if !ids.contains(&id) {
                            ids.push(id);
                            vectors
                                .push(ndarray::Array1::zeros(crate::config::get_embedding_dim()));
                        }
                    }

                    let query_entities = crate::entity::extract_query_entities(query);
                    if query_entities.len() >= 2 {
                        let doc_uuid_filter = filters_opt.as_ref().and_then(|f_list| {
                            f_list.iter().find_map(|filter| {
                                if filter.field == "doc_uuid"
                                    && (filter.operator == "==" || filter.operator == "=")
                                {
                                    filter.value.as_str().map(ToString::to_string)
                                } else {
                                    None
                                }
                            })
                        });
                        let entity_ids = db_pools
                            .search_entity_candidate_ids(
                                &query_entities,
                                doc_uuid_filter.as_deref(),
                                fallback_limit,
                            )
                            .await;

                        if !entity_ids.is_empty() {
                            if let Some(client) = db_pools.qdrant_clients.get("default") {
                                let collection = crate::config::get_qdrant_collection();
                                let point_ids: Vec<qdrant_client::qdrant::PointId> = entity_ids
                                    .iter()
                                    .filter(|id| !ids.contains(id))
                                    .map(|&id| qdrant_client::qdrant::PointId {
                                        point_id_options: Some(
                                            qdrant_client::qdrant::point_id::PointIdOptions::Num(
                                                id as u64,
                                            ),
                                        ),
                                    })
                                    .collect();

                                if !point_ids.is_empty() {
                                    let request = qdrant_client::qdrant::GetPoints {
                                        collection_name: collection.clone(),
                                        ids: point_ids,
                                        with_vectors: Some(qdrant_client::qdrant::WithVectorsSelector {
                                            selector_options: Some(
                                                qdrant_client::qdrant::with_vectors_selector::SelectorOptions::Enable(true),
                                            ),
                                        }),
                                        with_payload: Some(qdrant_client::qdrant::WithPayloadSelector {
                                            selector_options: Some(
                                                qdrant_client::qdrant::with_payload_selector::SelectorOptions::Enable(false),
                                            ),
                                        }),
                                        ..Default::default()
                                    };
                                    if let Ok(response) = client.get_points(request).await {
                                        for point in response.result {
                                            let id = match &point.id {
                                                Some(qdrant_client::qdrant::PointId {
                                                    point_id_options: Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)),
                                                }) => *n as usize,
                                                _ => continue,
                                            };

                                            let vec_data = if let Some(qdrant_client::qdrant::VectorsOutput {
                                                vectors_options: Some(qdrant_client::qdrant::vectors_output::VectorsOptions::Vector(v)),
                                            }) = &point.vectors {
                                                if let Some(qdrant_client::qdrant::vector_output::Vector::Dense(dense)) = &v.vector {
                                                    dense.data.clone()
                                                } else {
                                                    #[allow(deprecated)]
                                                    v.data.clone()
                                                }
                                            } else {
                                                vec![0.1; crate::config::get_embedding_dim()]
                                            };

                                            ids.push(id);
                                            vectors.push(ndarray::Array1::from(vec_data));
                                        }
                                    }
                                }
                            }

                            for id in entity_ids {
                                if !ids.contains(&id) {
                                    ids.push(id);
                                    vectors.push(ndarray::Array1::zeros(
                                        crate::config::get_embedding_dim(),
                                    ));
                                }
                            }
                        }
                    }

                    let batch = crate::arrow::qdrant_to_record_batch(&ids, &vectors)
                        .map_err(|e| format!("Failed to convert qdrant results: {}", e))?;
                    vm.batch = Some(Arc::new(batch));
                }
                crate::ir::OpCode::RetrievePostgres { query } => {
                    let batch = vm
                        .batch
                        .as_ref()
                        .ok_or("No batch found for RetrievePostgres")?;

                    let id_col = batch.column_by_name("id").ok_or("Missing id column")?;
                    let id_arr = id_col
                        .as_any()
                        .downcast_ref::<arrow::array::Int64Array>()
                        .ok_or("Not Int64Array")?;
                    let real_ids: Vec<usize> =
                        id_arr.iter().map(|v| v.unwrap_or(0) as usize).collect();
                    let scores = db_pools.fetch_bm25_scores(&real_ids, query).await;

                    vm.stack.push(crate::vm::Value::ArrowArray(Arc::new(
                        arrow::array::Float32Array::from(scores),
                    )));
                }
                crate::ir::OpCode::RetrieveEntityScores { query } => {
                    let batch = vm
                        .batch
                        .as_ref()
                        .ok_or("No batch found for RetrieveEntityScores")?;

                    let id_col = batch.column_by_name("id").ok_or("Missing id column")?;
                    let id_arr = id_col
                        .as_any()
                        .downcast_ref::<arrow::array::Int64Array>()
                        .ok_or("Not Int64Array")?;
                    let real_ids: Vec<usize> =
                        id_arr.iter().map(|v| v.unwrap_or(0) as usize).collect();
                    let query_entities = crate::entity::extract_query_entities(query);
                    let scores = db_pools
                        .fetch_entity_scores(&real_ids, &query_entities)
                        .await;

                    vm.stack.push(crate::vm::Value::ArrowArray(Arc::new(
                        arrow::array::Float32Array::from(scores),
                    )));
                }
                crate::ir::OpCode::IfConfidence(threshold, low_plan, high_plan) => {
                    let a = vm.stack.pop().ok_or("Stack underflow in IfConfidence")?;
                    let mut max_score = 0.0;
                    match &a {
                        crate::vm::Value::Tensor1D(t) => {
                            for &score in t.iter() {
                                if score > max_score {
                                    max_score = score;
                                }
                            }
                        }
                        crate::vm::Value::ArrowArray(arr) => {
                            let t_f32 = arr
                                .as_any()
                                .downcast_ref::<arrow::array::Float32Array>()
                                .ok_or("Not Float32Array")?;
                            for score in t_f32.iter().flatten() {
                                if score > max_score {
                                    max_score = score;
                                }
                            }
                        }
                        _ => return Err("Invalid type for IfConfidence condition".to_string()),
                    }

                    vm.stack.push(a);

                    let branch_plan = if max_score >= *threshold {
                        high_plan
                    } else {
                        low_plan
                    };

                    if let Some(res) = execute_program(
                        branch_plan,
                        vm,
                        db_pools,
                        instruction_count,
                        max_instructions,
                    )
                    .await?
                    {
                        return Ok(Some(res));
                    }
                }
                crate::ir::OpCode::RetrieveAnchors { query, mode, top_k } => {
                    let val = vm.stack.pop().ok_or("Stack underflow in RetrieveAnchors")?;
                    let main_result = match val {
                        crate::vm::Value::Result(r) => r,
                        _ => return Err("Expected TopKResult for RetrieveAnchors".to_string()),
                    };

                    let mut doc_uuids = Vec::new();
                    if let Some(pool) = &db_pools.postgres {
                        use sqlx::Row;
                        let ids_i64: Vec<i64> =
                            main_result.indices.iter().map(|&x| x as i64).collect();
                        let rows =
                            sqlx::query("SELECT id, doc_uuid FROM documents WHERE id = ANY($1)")
                                .bind(&ids_i64[..])
                                .fetch_all(pool)
                                .await
                                .map_err(|e| {
                                    format!("Failed to resolve doc_uuids for anchors: {}", e)
                                })?;

                        let mut uuid_map = std::collections::HashMap::new();
                        for r in rows {
                            let id: i64 = r.try_get("id").unwrap_or(-1);
                            let doc_uuid: String = r.try_get("doc_uuid").unwrap_or_default();
                            uuid_map.insert(id, doc_uuid);
                        }

                        for &doc_id in &main_result.indices {
                            doc_uuids
                                .push(uuid_map.get(&(doc_id as i64)).cloned().unwrap_or_default());
                        }
                    } else {
                        return Err("PostgreSQL pool not initialized".to_string());
                    }

                    let mut anchor_scores = vec![0.0f32; main_result.indices.len()];

                    match mode {
                        crate::ir::AnchorSearchMode::Exact => {
                            let results = db_pools.search_anchors_bm25(query, *top_k).await?;
                            for (_, doc_uuid, _, score) in results {
                                if let Some(idx) = doc_uuids.iter().position(|u| u == &doc_uuid) {
                                    if score > anchor_scores[idx] {
                                        anchor_scores[idx] = score;
                                    }
                                }
                            }
                        }
                        crate::ir::AnchorSearchMode::Semantic => {
                            let query_vector = db_pools.get_embedding(query, true).await?;
                            let (ids, scores, _) =
                                db_pools.search_anchors_dense(&query_vector, *top_k).await?;

                            if !ids.is_empty() {
                                if let Some(pool) = &db_pools.postgres {
                                    use sqlx::Row;
                                    let rows = sqlx::query(
                                        "SELECT id, doc_uuid FROM anchors WHERE id = ANY($1)",
                                    )
                                    .bind(&ids[..])
                                    .fetch_all(pool)
                                    .await
                                    .map_err(|e| {
                                        format!("Failed to map anchor IDs to doc_uuids: {}", e)
                                    })?;

                                    let mut anchor_uuid_map = std::collections::HashMap::new();
                                    for r in rows {
                                        let id: i64 = r.try_get("id").unwrap_or(-1);
                                        let doc_uuid: String =
                                            r.try_get("doc_uuid").unwrap_or_default();
                                        anchor_uuid_map.insert(id, doc_uuid);
                                    }

                                    for (i, &anchor_id) in ids.iter().enumerate() {
                                        if let Some(doc_uuid) = anchor_uuid_map.get(&anchor_id) {
                                            if let Some(idx) =
                                                doc_uuids.iter().position(|u| u == doc_uuid)
                                            {
                                                let score = scores[i];
                                                if score > anchor_scores[idx] {
                                                    anchor_scores[idx] = score;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        crate::ir::AnchorSearchMode::Both => {
                            let exact_results = db_pools.search_anchors_bm25(query, *top_k).await?;
                            for (_, doc_uuid, _, score) in exact_results {
                                if let Some(idx) = doc_uuids.iter().position(|u| u == &doc_uuid) {
                                    if score > anchor_scores[idx] {
                                        anchor_scores[idx] = score;
                                    }
                                }
                            }
                            let query_vector = db_pools.get_embedding(query, true).await?;
                            let (ids, scores, _) =
                                db_pools.search_anchors_dense(&query_vector, *top_k).await?;

                            if !ids.is_empty() {
                                if let Some(pool) = &db_pools.postgres {
                                    use sqlx::Row;
                                    let rows = sqlx::query(
                                        "SELECT id, doc_uuid FROM anchors WHERE id = ANY($1)",
                                    )
                                    .bind(&ids[..])
                                    .fetch_all(pool)
                                    .await
                                    .map_err(|e| {
                                        format!("Failed to map anchor IDs to doc_uuids: {}", e)
                                    })?;

                                    let mut anchor_uuid_map = std::collections::HashMap::new();
                                    for r in rows {
                                        let id: i64 = r.try_get("id").unwrap_or(-1);
                                        let doc_uuid: String =
                                            r.try_get("doc_uuid").unwrap_or_default();
                                        anchor_uuid_map.insert(id, doc_uuid);
                                    }

                                    for (i, &anchor_id) in ids.iter().enumerate() {
                                        if let Some(doc_uuid) = anchor_uuid_map.get(&anchor_id) {
                                            if let Some(idx) =
                                                doc_uuids.iter().position(|u| u == doc_uuid)
                                            {
                                                let score = scores[i];
                                                if score > anchor_scores[idx] {
                                                    anchor_scores[idx] = score;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    vm.stack.push(crate::vm::Value::Result(main_result));
                    vm.stack
                        .push(crate::vm::Value::Tensor1D(ndarray::Array1::from(
                            anchor_scores,
                        )));
                }
                crate::ir::OpCode::AnchorFusion {
                    main_weight,
                    anchor_weight,
                } => {
                    let anchor_val = vm.stack.pop().ok_or("Stack underflow in AnchorFusion")?;
                    let main_val = vm.stack.pop().ok_or("Stack underflow in AnchorFusion")?;

                    let anchor_scores =
                        match anchor_val {
                            crate::vm::Value::Tensor1D(t) => t.to_vec(),
                            crate::vm::Value::ArrowArray(arr) => {
                                let t_f32 = arr
                                    .as_any()
                                    .downcast_ref::<arrow::array::Float32Array>()
                                    .ok_or("Not Float32Array")?;
                                t_f32.iter().map(|v| v.unwrap_or(0.0)).collect()
                            }
                            _ => return Err(
                                "Expected Tensor1D or ArrowArray for AnchorFusion anchor scores"
                                    .to_string(),
                            ),
                        };

                    let main_result = match main_val {
                        crate::vm::Value::Result(r) => r,
                        _ => {
                            return Err(
                                "Expected TopKResult for AnchorFusion main result".to_string()
                            );
                        }
                    };

                    let mut fused_scores = Vec::new();
                    for i in 0..main_result.indices.len() {
                        let m = main_result.scores.get(i).copied().unwrap_or(0.0);
                        let a = anchor_scores.get(i).copied().unwrap_or(0.0);
                        fused_scores.push(m * main_weight + a * anchor_weight);
                    }

                    vm.stack
                        .push(crate::vm::Value::Result(crate::vm::TopKResult {
                            indices: main_result.indices,
                            scores: fused_scores,
                        }));
                }
                crate::ir::OpCode::FilterByMetadata { filters } => {
                    let val = vm
                        .stack
                        .pop()
                        .ok_or("Stack underflow in FilterByMetadata")?;
                    let main_result = match val {
                        crate::vm::Value::Result(r) => r,
                        _ => return Err("Expected TopKResult for FilterByMetadata".to_string()),
                    };

                    let mut filtered_indices = Vec::new();
                    let mut filtered_scores = Vec::new();

                    if let Some(pool) = &db_pools.postgres {
                        use sqlx::Row;
                        let ids_i64: Vec<i64> =
                            main_result.indices.iter().map(|&x| x as i64).collect();
                        let rows =
                            sqlx::query("SELECT id, metadata FROM documents WHERE id = ANY($1)")
                                .bind(&ids_i64[..])
                                .fetch_all(pool)
                                .await
                                .map_err(|e| {
                                    format!("Failed to fetch metadata for filtering: {}", e)
                                })?;

                        let mut metadata_map = std::collections::HashMap::new();
                        for r in rows {
                            let id: i64 = r.try_get("id").unwrap_or(-1);
                            let metadata_val: serde_json::Value =
                                r.try_get("metadata").unwrap_or(serde_json::json!({}));
                            metadata_map.insert(id, metadata_val);
                        }

                        for (i, &doc_id) in main_result.indices.iter().enumerate() {
                            let doc_metadata = metadata_map
                                .get(&(doc_id as i64))
                                .cloned()
                                .unwrap_or(serde_json::json!({}));

                            let matches_all = filters.iter().all(|f| {
                                let val = match doc_metadata.get(&f.field) {
                                    Some(v) => v,
                                    None => return false,
                                };
                                match f.operator.as_str() {
                                    "==" | "=" => val == &f.value,
                                    "!=" => val != &f.value,
                                    ">=" => {
                                        if let (Some(v_num), Some(f_num)) =
                                            (val.as_f64(), f.value.as_f64())
                                        {
                                            v_num >= f_num
                                        } else {
                                            false
                                        }
                                    }
                                    ">" => {
                                        if let (Some(v_num), Some(f_num)) =
                                            (val.as_f64(), f.value.as_f64())
                                        {
                                            v_num > f_num
                                        } else {
                                            false
                                        }
                                    }
                                    "<=" => {
                                        if let (Some(v_num), Some(f_num)) =
                                            (val.as_f64(), f.value.as_f64())
                                        {
                                            v_num <= f_num
                                        } else {
                                            false
                                        }
                                    }
                                    "<" => {
                                        if let (Some(v_num), Some(f_num)) =
                                            (val.as_f64(), f.value.as_f64())
                                        {
                                            v_num < f_num
                                        } else {
                                            false
                                        }
                                    }
                                    "in" => {
                                        if let Some(arr) = f.value.as_array() {
                                            arr.contains(val)
                                        } else {
                                            false
                                        }
                                    }
                                    _ => false,
                                }
                            });

                            if matches_all {
                                filtered_indices.push(doc_id);
                                filtered_scores.push(main_result.scores[i]);
                            }
                        }
                    }

                    vm.stack
                        .push(crate::vm::Value::Result(crate::vm::TopKResult {
                            indices: filtered_indices,
                            scores: filtered_scores,
                        }));
                }
                crate::ir::OpCode::RetrieveEdges { doc_uuid } => {
                    let _ = db_pools.fetch_edges_by_doc_uuid(doc_uuid, None).await?;
                    vm.stack.push(crate::vm::Value::Scalar(1.0));
                }
                crate::ir::OpCode::AddMetadata {
                    doc_uuid,
                    key,
                    value,
                } => {
                    db_pools
                        .update_document_metadata(doc_uuid, key, value.clone())
                        .await?;
                    vm.stack.push(crate::vm::Value::Scalar(1.0));
                }
                crate::ir::OpCode::AddAnchor {
                    doc_uuid,
                    name,
                    text,
                } => {
                    let anchor_id = db_pools.insert_anchor(doc_uuid, name, text).await?;
                    let vector = db_pools.get_embedding(text, true).await?;
                    db_pools
                        .insert_anchor_embedding(anchor_id, vector, doc_uuid, name)
                        .await?;
                    vm.stack.push(crate::vm::Value::Scalar(1.0));
                }
                crate::ir::OpCode::AddEdge {
                    source,
                    target,
                    edge_type,
                    metadata,
                } => {
                    db_pools
                        .insert_edge(source, target, edge_type, metadata.clone())
                        .await?;
                    vm.stack.push(crate::vm::Value::Scalar(1.0));
                }
                _ => {
                    if let Some(res) = vm.step(&op.op)? {
                        return Ok(Some(res));
                    }
                }
            }
        }
        Ok(None)
    })
}
