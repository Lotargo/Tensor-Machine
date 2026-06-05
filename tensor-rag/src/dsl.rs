use crate::ir::{AnchorSearchMode, AstNode, Filter, QueryRequest, RerankStrategy};
use lexpr::Value;

pub fn parse_dsl(input: &str) -> Result<AstNode, String> {
    let parsed =
        lexpr::from_str(input).map_err(|e| format!("Failed to parse s-expression: {}", e))?;

    match &parsed {
        Value::Cons(cell) => {
            let car = cell.car();
            let cdr = cell.cdr();

            match car.as_symbol() {
                Some("search") => {
                    let req = parse_search_args(cdr)?;
                    Ok(AstNode::Search(req))
                }
                Some("multi-hop") => parse_multi_hop_args(cdr),
                Some("self-rag") => parse_self_rag_args(cdr),
                Some("corrective-rag") => parse_corrective_rag_args(cdr),
                Some("colbert-retrieval") => parse_colbert_retrieval_args(cdr),
                Some("add-metadata") => parse_add_metadata_args(cdr),
                Some("add-anchor") => parse_add_anchor_args(cdr),
                Some("add-edge") => parse_add_edge_args(cdr),
                _ => Err(
                    "Expected a 'search', 'multi-hop', 'self-rag', 'corrective-rag', 'colbert-retrieval', 'add-metadata', 'add-anchor', or 'add-edge' expression"
                        .to_string(),
                ),
            }
        }
        _ => Err("Expected a list expression".to_string()),
    }
}

fn parse_add_metadata_args(args: &Value) -> Result<AstNode, String> {
    let mut doc_uuid = String::new();
    let mut key = String::new();
    let mut value = serde_json::Value::Null;

    let mut current = args;
    while let Value::Cons(cell) = current {
        let key_sym = cell.car().as_symbol().ok_or("Expected keyword symbol")?;
        let next_cell = cell
            .cdr()
            .as_cons()
            .ok_or(format!("Expected value after {}", key_sym))?;
        let val = next_cell.car();

        match key_sym {
            ":doc-uuid" => doc_uuid = val.as_str().ok_or("doc-uuid must be a string")?.to_string(),
            ":key" => key = val.as_str().ok_or("key must be a string")?.to_string(),
            ":value" => {
                value = match val {
                    Value::String(s) => serde_json::json!(s),
                    Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            serde_json::json!(i)
                        } else {
                            serde_json::json!(n.as_f64().unwrap_or(0.0))
                        }
                    }
                    _ => return Err("value must be a string or number".to_string()),
                };
            }
            _ => return Err(format!("Unknown parameter: {}", key_sym)),
        }
        current = next_cell.cdr();
    }

    if doc_uuid.is_empty() {
        return Err("Missing :doc-uuid".to_string());
    }
    if key.is_empty() {
        return Err("Missing :key".to_string());
    }

    Ok(AstNode::AddMetadata {
        doc_uuid,
        key,
        value,
    })
}

fn parse_add_anchor_args(args: &Value) -> Result<AstNode, String> {
    let mut doc_uuid = String::new();
    let mut name = String::new();
    let mut text = String::new();

    let mut current = args;
    while let Value::Cons(cell) = current {
        let key_sym = cell.car().as_symbol().ok_or("Expected keyword symbol")?;
        let next_cell = cell
            .cdr()
            .as_cons()
            .ok_or(format!("Expected value after {}", key_sym))?;
        let val = next_cell.car();

        match key_sym {
            ":doc-uuid" => doc_uuid = val.as_str().ok_or("doc-uuid must be a string")?.to_string(),
            ":name" => name = val.as_str().ok_or("name must be a string")?.to_string(),
            ":text" => text = val.as_str().ok_or("text must be a string")?.to_string(),
            _ => return Err(format!("Unknown parameter: {}", key_sym)),
        }
        current = next_cell.cdr();
    }

    if doc_uuid.is_empty() {
        return Err("Missing :doc-uuid".to_string());
    }
    if name.is_empty() {
        return Err("Missing :name".to_string());
    }
    if text.is_empty() {
        return Err("Missing :text".to_string());
    }

    Ok(AstNode::AddAnchor {
        doc_uuid,
        name,
        text,
    })
}

fn parse_add_edge_args(args: &Value) -> Result<AstNode, String> {
    let mut source = String::new();
    let mut target = String::new();
    let mut edge_type = String::new();
    let mut metadata = None;

    let mut current = args;
    while let Value::Cons(cell) = current {
        let key_sym = cell.car().as_symbol().ok_or("Expected keyword symbol")?;
        let next_cell = cell
            .cdr()
            .as_cons()
            .ok_or(format!("Expected value after {}", key_sym))?;
        let val = next_cell.car();

        match key_sym {
            ":source" => source = val.as_str().ok_or("source must be a string")?.to_string(),
            ":target" => target = val.as_str().ok_or("target must be a string")?.to_string(),
            ":type" => edge_type = val.as_str().ok_or("type must be a string")?.to_string(),
            ":metadata" => {
                let meta_str = val.as_str().ok_or("metadata must be a JSON string")?;
                metadata = Some(
                    serde_json::from_str(meta_str)
                        .map_err(|e| format!("Invalid metadata JSON: {}", e))?,
                );
            }
            _ => return Err(format!("Unknown parameter: {}", key_sym)),
        }
        current = next_cell.cdr();
    }

    if source.is_empty() {
        return Err("Missing :source".to_string());
    }
    if target.is_empty() {
        return Err("Missing :target".to_string());
    }
    if edge_type.is_empty() {
        return Err("Missing :type".to_string());
    }

    Ok(AstNode::AddEdge {
        source,
        target,
        edge_type,
        metadata,
    })
}

fn parse_multi_hop_args(args: &Value) -> Result<AstNode, String> {
    let mut query = String::new();
    let mut hops = 1;

    let mut current = args;
    while let Value::Cons(cell) = current {
        let key = cell.car().as_symbol().ok_or("Expected keyword symbol")?;
        let next_cell = cell
            .cdr()
            .as_cons()
            .ok_or(format!("Expected value after {}", key))?;
        let value = next_cell.car();

        match key {
            ":query" => query = value.as_str().ok_or("Query must be a string")?.to_string(),
            ":hops" => hops = value.as_u64().ok_or("hops must be an integer")? as usize,
            _ => return Err(format!("Unknown parameter: {}", key)),
        }
        current = next_cell.cdr();
    }
    Ok(AstNode::MultiHop { query, hops })
}

fn parse_self_rag_args(args: &Value) -> Result<AstNode, String> {
    let mut query = String::new();
    let mut threshold = 0.5;
    let mut low_plan = None;
    let mut high_plan = None;

    let mut current = args;
    while let Value::Cons(cell) = current {
        let key = cell.car().as_symbol().ok_or("Expected keyword symbol")?;
        let next_cell = cell
            .cdr()
            .as_cons()
            .ok_or(format!("Expected value after {}", key))?;
        let value = next_cell.car();

        match key {
            ":query" => query = value.as_str().ok_or("Query must be a string")?.to_string(),
            ":threshold" => threshold = value.as_f64().ok_or("threshold must be a float")? as f32,
            ":low-confidence" => low_plan = Some(Box::new(parse_inner_node(value)?)),
            ":high-confidence" => high_plan = Some(Box::new(parse_inner_node(value)?)),
            _ => return Err(format!("Unknown parameter: {}", key)),
        }
        current = next_cell.cdr();
    }

    Ok(AstNode::SelfRag {
        query,
        threshold,
        low_plan: low_plan.ok_or("Missing :low-confidence plan")?,
        high_plan: high_plan.ok_or("Missing :high-confidence plan")?,
    })
}

fn parse_corrective_rag_args(args: &Value) -> Result<AstNode, String> {
    let mut query = String::new();
    let mut fallback_query = String::new();

    let mut current = args;
    while let Value::Cons(cell) = current {
        let key = cell.car().as_symbol().ok_or("Expected keyword symbol")?;
        let next_cell = cell
            .cdr()
            .as_cons()
            .ok_or(format!("Expected value after {}", key))?;
        let value = next_cell.car();

        match key {
            ":query" => query = value.as_str().ok_or("Query must be a string")?.to_string(),
            ":fallback-query" => {
                fallback_query = value
                    .as_str()
                    .ok_or("Fallback query must be a string")?
                    .to_string()
            }
            _ => return Err(format!("Unknown parameter: {}", key)),
        }
        current = next_cell.cdr();
    }

    Ok(AstNode::CorrectiveRag {
        query,
        fallback_query,
    })
}

fn parse_inner_node(val: &Value) -> Result<AstNode, String> {
    match val {
        Value::Cons(cell) => {
            let car = cell.car();
            let cdr = cell.cdr();

            match car.as_symbol() {
                Some("search") => {
                    let req = parse_search_args(cdr)?;
                    Ok(AstNode::Search(req))
                }
                Some("multi-hop") => parse_multi_hop_args(cdr),
                Some("self-rag") => parse_self_rag_args(cdr),
                Some("corrective-rag") => parse_corrective_rag_args(cdr),
                Some("colbert-retrieval") => parse_colbert_retrieval_args(cdr),
                Some("add-metadata") => parse_add_metadata_args(cdr),
                Some("add-anchor") => parse_add_anchor_args(cdr),
                Some("add-edge") => parse_add_edge_args(cdr),
                _ => {
                    Err("Expected an inner expression list starting with known keyword".to_string())
                }
            }
        }
        _ => Err("Expected a valid inner AST node list".to_string()),
    }
}

fn parse_search_args(args: &Value) -> Result<QueryRequest, String> {
    let mut query = String::new();
    let mut filters: Option<Vec<Filter>> = None;
    let mut rerank_strategy = RerankStrategy::Hybrid {
        semantic_weight: 0.7,
        bm25_weight: 0.3,
    };
    let mut final_top_k = 10;
    let mut doc_uuid: Option<String> = None;
    let mut alpha: Option<f32> = None;
    let mut anchor_name: Option<String> = None;
    let mut anchor_mode: Option<AnchorSearchMode> = None;
    let mut metadata_filters: Option<Vec<Filter>> = None;

    let mut current = args;
    while let Value::Cons(cell) = current {
        let key = cell.car().as_symbol().ok_or("Expected keyword symbol")?;
        let next_cell = cell
            .cdr()
            .as_cons()
            .ok_or(format!("Expected value after {}", key))?;
        let value = next_cell.car();

        match key {
            ":query" => {
                query = value.as_str().ok_or("Query must be a string")?.to_string();
            }
            ":filters" => {
                filters = Some(parse_filters(value)?);
            }
            ":rerank" => {
                rerank_strategy = parse_rerank(value)?;
            }
            ":final-top-k" => {
                final_top_k = value.as_u64().ok_or("final-top-k must be an integer")? as usize;
            }
            ":doc-uuid" => {
                doc_uuid = Some(
                    value
                        .as_str()
                        .ok_or("doc-uuid must be a string")?
                        .to_string(),
                );
            }
            ":alpha" => {
                alpha = Some(value.as_f64().ok_or("alpha must be a float")? as f32);
            }
            ":anchor-name" => {
                anchor_name = Some(
                    value
                        .as_str()
                        .ok_or("anchor-name must be a string")?
                        .to_string(),
                );
            }
            ":anchor-mode" => {
                let mode_str = value
                    .as_symbol()
                    .or_else(|| {
                        value.as_str().map(|s| {
                            // Create a symbol-like Value from string
                            return s;
                        })
                    })
                    .ok_or("anchor-mode must be a symbol (exact, semantic, or both)")?;
                anchor_mode = match mode_str {
                    "exact" => Some(AnchorSearchMode::Exact),
                    "semantic" => Some(AnchorSearchMode::Semantic),
                    "both" => Some(AnchorSearchMode::Both),
                    _ => {
                        return Err("anchor-mode must be one of: exact, semantic, both".to_string());
                    }
                };
            }
            ":metadata-filters" => {
                metadata_filters = Some(parse_filters_list(value)?);
            }
            _ => return Err(format!("Unknown parameter: {}", key)),
        }

        current = next_cell.cdr();
    }

    Ok(QueryRequest {
        query,
        filters,
        rerank_strategy,
        final_top_k,
        doc_uuid,
        alpha,
        anchor_name,
        anchor_mode,
        metadata_filters,
    })
}

fn parse_filters_list(filter_val: &Value) -> Result<Vec<Filter>, String> {
    let mut parsed = Vec::new();
    match filter_val {
        Value::Cons(cell) => {
            let op_sym = cell.car().as_symbol().ok_or("Expected filter operator")?;
            let rest = cell.cdr().as_cons().ok_or("Expected filter operands")?;
            let field_sym = rest.car().as_symbol().ok_or("Expected filter field")?;
            let val_cell = rest.cdr().as_cons().ok_or("Expected filter value")?;
            let val = val_cell.car();

            let json_val = match val {
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        serde_json::json!(i)
                    } else if let Some(f) = n.as_f64() {
                        serde_json::json!(f)
                    } else {
                        return Err("Unsupported number format in filter".to_string());
                    }
                }
                Value::String(s) => serde_json::json!(s),
                _ => return Err("Unsupported value type in filter".to_string()),
            };

            parsed.push(Filter {
                field: field_sym.to_string(),
                operator: op_sym.to_string(),
                value: json_val,
            });
        }
        Value::Vector(vec) => {
            for item in vec {
                if let Value::Cons(cell) = item {
                    let op_sym = cell.car().as_symbol().ok_or("Expected filter operator")?;
                    let rest = cell.cdr().as_cons().ok_or("Expected filter operands")?;
                    let field_sym = rest.car().as_symbol().ok_or("Expected filter field")?;
                    let val_cell = rest.cdr().as_cons().ok_or("Expected filter value")?;
                    let val = val_cell.car();

                    let json_val = match val {
                        Value::Number(n) => {
                            if let Some(i) = n.as_i64() {
                                serde_json::json!(i)
                            } else if let Some(f) = n.as_f64() {
                                serde_json::json!(f)
                            } else {
                                return Err("Unsupported number format in filter".to_string());
                            }
                        }
                        Value::String(s) => serde_json::json!(s),
                        _ => return Err("Unsupported value type in filter".to_string()),
                    };

                    parsed.push(Filter {
                        field: field_sym.to_string(),
                        operator: op_sym.to_string(),
                        value: json_val,
                    });
                }
            }
        }
        _ => return Err("Expected a filter expression or list".to_string()),
    }
    Ok(parsed)
}

fn parse_filters(filter_val: &Value) -> Result<Vec<Filter>, String> {
    // For now, we expect a single filter expression like (>= year 2023)
    // In the future, this could be a list of filters.
    let mut parsed_filters = Vec::new();

    match filter_val {
        Value::Cons(cell) => {
            let op_sym = cell.car().as_symbol().ok_or("Expected filter operator")?;
            let rest = cell.cdr().as_cons().ok_or("Expected filter operands")?;
            let field_sym = rest.car().as_symbol().ok_or("Expected filter field")?;
            let val_cell = rest.cdr().as_cons().ok_or("Expected filter value")?;
            let val = val_cell.car();

            let json_val = match val {
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        serde_json::json!(i)
                    } else if let Some(f) = n.as_f64() {
                        serde_json::json!(f)
                    } else {
                        return Err("Unsupported number format in filter".to_string());
                    }
                }
                Value::String(s) => serde_json::json!(s),
                _ => return Err("Unsupported value type in filter".to_string()),
            };

            parsed_filters.push(Filter {
                field: field_sym.to_string(),
                operator: op_sym.to_string(),
                value: json_val,
            });
        }
        _ => return Err("Expected a filter expression list".to_string()),
    }

    Ok(parsed_filters)
}

fn parse_rerank(rerank_val: &Value) -> Result<RerankStrategy, String> {
    match rerank_val {
        Value::Cons(cell) => {
            let strategy = cell
                .car()
                .as_symbol()
                .ok_or("Expected rerank strategy name")?;
            if strategy == "hybrid" {
                let rest = cell
                    .cdr()
                    .as_cons()
                    .ok_or("Expected :weights after hybrid")?;
                let weights_key = rest.car().as_symbol().ok_or("Expected :weights keyword")?;
                if weights_key != ":weights" {
                    return Err("Expected :weights keyword".to_string());
                }

                let weights_list = rest.cdr().as_cons().ok_or("Expected weights list")?.car();

                // Parse (sem 0.7 bm25 0.3)
                if let Value::Cons(w_cell) = weights_list {
                    let sem_sym = w_cell.car().as_symbol().ok_or("Expected sem symbol")?;
                    if sem_sym != "sem" {
                        return Err("Expected sem symbol".to_string());
                    }
                    let r1 = w_cell.cdr().as_cons().ok_or("Expected sem weight")?;
                    let sem_weight = r1.car().as_f64().ok_or("sem weight must be a float")? as f32;

                    let r2 = r1.cdr().as_cons().ok_or("Expected bm25 symbol")?;
                    let bm25_sym = r2.car().as_symbol().ok_or("Expected bm25 symbol")?;
                    if bm25_sym != "bm25" {
                        return Err("Expected bm25 symbol".to_string());
                    }

                    let r3 = r2.cdr().as_cons().ok_or("Expected bm25 weight")?;
                    let bm25_weight =
                        r3.car().as_f64().ok_or("bm25 weight must be a float")? as f32;

                    Ok(RerankStrategy::Hybrid {
                        semantic_weight: sem_weight,
                        bm25_weight,
                    })
                } else {
                    Err("Expected weights list".to_string())
                }
            } else if strategy == "semantic" {
                Ok(RerankStrategy::SemanticOnly)
            } else if strategy == "bm25" {
                Ok(RerankStrategy::Bm25Only)
            } else if strategy == "mmr" {
                let rest = cell
                    .cdr()
                    .as_cons()
                    .ok_or("Expected :diversity after mmr")?;
                let diversity_key = rest
                    .car()
                    .as_symbol()
                    .ok_or("Expected :diversity keyword")?;
                if diversity_key != ":diversity" {
                    return Err("Expected :diversity keyword".to_string());
                }
                let diversity_val = rest
                    .cdr()
                    .as_cons()
                    .ok_or("Expected diversity value")?
                    .car();
                let diversity = diversity_val.as_f64().ok_or("diversity must be a float")? as f32;
                Ok(RerankStrategy::Mmr { diversity })
            } else if strategy == "rrf" {
                let rest = cell.cdr().as_cons().ok_or("Expected :k after rrf")?;
                let k_key = rest.car().as_symbol().ok_or("Expected :k keyword")?;
                if k_key != ":k" {
                    return Err("Expected :k keyword".to_string());
                }
                let k_val = rest.cdr().as_cons().ok_or("Expected k value")?.car();
                let k = k_val.as_u64().ok_or("k must be an integer")? as usize;
                Ok(RerankStrategy::Rrf { k })
            } else if strategy == "rsf" {
                Ok(RerankStrategy::Rsf {
                    dense_weight: None,
                    bm25_weight: None,
                    entity_weight: None,
                })
            } else {
                let rest_json = if let Some(rest) = cell.cdr().as_cons() {
                    let mut args_map = serde_json::Map::new();
                    let mut current = rest;
                    loop {
                        if current.car().is_null() {
                            break;
                        }
                        let key = current
                            .car()
                            .as_symbol()
                            .unwrap_or("")
                            .trim_start_matches(':');
                        if let Some(next) = current.cdr().as_cons() {
                            let val = next.car();
                            let json_val = if let Some(n) = val.as_i64() {
                                serde_json::json!(n)
                            } else if let Some(f) = val.as_f64() {
                                serde_json::json!(f)
                            } else if let Some(s) = val.as_symbol() {
                                serde_json::json!(s)
                            } else if let Some(s) = val.as_str() {
                                serde_json::json!(s)
                            } else {
                                serde_json::json!(val.to_string())
                            };
                            args_map.insert(key.to_string(), json_val);
                            if let Some(nnext) = next.cdr().as_cons() {
                                current = nnext;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    serde_json::Value::Object(args_map)
                } else {
                    serde_json::json!({})
                };

                Ok(RerankStrategy::Dynamic {
                    name: strategy.to_string(),
                    args: rest_json,
                })
            }
        }
        _ => Err("Expected a rerank expression list".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hybrid_rerank() {
        let input = r#"(search :query "quantum physics"
                                :filters (>= year 2023)
                                :rerank (hybrid :weights (sem 0.7 bm25 0.3))
                                :final-top-k 5)"#;
        let ast = parse_dsl(input).expect("Failed to parse valid DSL");

        match ast {
            AstNode::Search(request) => {
                assert_eq!(request.query, "quantum physics");
                assert_eq!(request.final_top_k, 5);

                match request.rerank_strategy {
                    RerankStrategy::Hybrid {
                        semantic_weight,
                        bm25_weight,
                    } => {
                        assert_eq!(semantic_weight, 0.7);
                        assert_eq!(bm25_weight, 0.3);
                    }
                    _ => panic!("Expected Hybrid rerank strategy"),
                }

                let filters = request.filters.expect("Expected filters");
                assert_eq!(filters.len(), 1);
                assert_eq!(filters[0].field, "year");
                assert_eq!(filters[0].operator, ">=");
                assert_eq!(filters[0].value, serde_json::json!(2023));
            }
            _ => panic!("Expected Search AST Node"),
        }
    }

    #[test]
    fn test_parse_semantic_rerank() {
        let input = r#"(search :query "test" :rerank (semantic) :final-top-k 10)"#;
        let ast = parse_dsl(input).expect("Failed to parse valid DSL");
        match ast {
            AstNode::Search(request) => {
                assert_eq!(request.query, "test");
                assert_eq!(request.final_top_k, 10);
                match request.rerank_strategy {
                    RerankStrategy::SemanticOnly => {}
                    _ => panic!("Expected SemanticOnly rerank strategy"),
                }
            }
            _ => panic!("Expected Search AST Node"),
        }
    }

    #[test]
    fn test_parse_multi_hop() {
        let input = r#"(multi-hop :query "multi test" :hops 3)"#;
        let ast = parse_dsl(input).expect("Failed to parse multi-hop");
        match ast {
            AstNode::MultiHop { query, hops } => {
                assert_eq!(query, "multi test");
                assert_eq!(hops, 3);
            }
            _ => panic!("Expected MultiHop AST Node"),
        }
    }

    #[test]
    fn test_parse_self_rag() {
        let input = r#"(self-rag :query "self rag test"
                                  :threshold 0.85
                                  :low-confidence (search :query "fallback" :rerank (semantic) :final-top-k 5)
                                  :high-confidence (search :query "self rag test" :rerank (semantic) :final-top-k 5))"#;
        let ast = parse_dsl(input).expect("Failed to parse self-rag");
        match ast {
            AstNode::SelfRag {
                query,
                threshold,
                low_plan,
                high_plan,
            } => {
                assert_eq!(query, "self rag test");
                assert_eq!(threshold, 0.85);
                assert!(matches!(*low_plan, AstNode::Search(_)));
                assert!(matches!(*high_plan, AstNode::Search(_)));
            }
            _ => panic!("Expected SelfRag AST Node"),
        }
    }

    #[test]
    fn test_parse_corrective_rag() {
        let input =
            r#"(corrective-rag :query "corrective test" :fallback-query "simple fallback")"#;
        let ast = parse_dsl(input).expect("Failed to parse corrective-rag");
        match ast {
            AstNode::CorrectiveRag {
                query,
                fallback_query,
            } => {
                assert_eq!(query, "corrective test");
                assert_eq!(fallback_query, "simple fallback");
            }
            _ => panic!("Expected CorrectiveRag AST Node"),
        }
    }

    #[test]
    fn test_parse_mmr() {
        let input = r#"(search :query "mmr test" :rerank (mmr :diversity 0.6) :final-top-k 5)"#;
        let ast = parse_dsl(input).expect("Failed to parse mmr");
        match ast {
            AstNode::Search(request) => match request.rerank_strategy {
                RerankStrategy::Mmr { diversity } => assert_eq!(diversity, 0.6),
                _ => panic!("Expected Mmr rerank strategy"),
            },
            _ => panic!("Expected Search AST Node"),
        }
    }

    #[test]
    fn test_parse_rrf() {
        let input = r#"(search :query "rrf test" :rerank (rrf :k 60) :final-top-k 10)"#;
        let ast = parse_dsl(input).expect("Failed to parse rrf");
        match ast {
            AstNode::Search(request) => match request.rerank_strategy {
                RerankStrategy::Rrf { k } => assert_eq!(k, 60),
                _ => panic!("Expected Rrf rerank strategy"),
            },
            _ => panic!("Expected Search AST Node"),
        }
    }
}

#[test]
fn test_parse_dynamic() {
    let input = r#"(search :query "dynamic test" :rerank (my-custom-algo :param1 42 :param2 "val") :final-top-k 10)"#;
    let ast = parse_dsl(input).expect("Failed to parse dynamic");
    match ast {
        AstNode::Search(request) => match request.rerank_strategy {
            RerankStrategy::Dynamic { name, args } => {
                assert_eq!(name, "my-custom-algo");
                assert_eq!(args["param1"], 42);
                assert_eq!(args["param2"], "val");
            }
            _ => panic!("Expected Dynamic rerank strategy"),
        },
        _ => panic!("Expected Search AST Node"),
    }
}

fn parse_colbert_retrieval_args(args: &Value) -> Result<AstNode, String> {
    let mut query = String::new();
    let mut docs_plan = None;

    let mut current = args;
    while let Value::Cons(cell) = current {
        let key = cell.car().as_symbol().ok_or("Expected keyword symbol")?;
        let next_cell = cell
            .cdr()
            .as_cons()
            .ok_or(format!("Expected value after {}", key))?;
        let value = next_cell.car();

        match key {
            ":query" => query = value.as_str().ok_or("Query must be a string")?.to_string(),
            ":docs" => docs_plan = Some(Box::new(parse_inner_node(value)?)),
            _ => return Err(format!("Unknown parameter: {}", key)),
        }

        current = next_cell.cdr();
    }

    Ok(AstNode::ColbertRetrieval {
        query,
        docs: docs_plan.ok_or("Missing :docs parameter")?,
    })
}
