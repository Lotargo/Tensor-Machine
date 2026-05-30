use crate::ir::{Filter, QueryRequest, RerankStrategy};
use lexpr::Value;

pub fn parse_dsl(input: &str) -> Result<QueryRequest, String> {
    let parsed = lexpr::from_str(input).map_err(|e| format!("Failed to parse s-expression: {}", e))?;

    match &parsed {
        Value::Cons(cell) => {
            let car = cell.car();
            let cdr = cell.cdr();

            if car.as_symbol() == Some("search") {
                parse_search_args(cdr)
            } else {
                Err("Expected a 'search' expression".to_string())
            }
        }
        _ => Err("Expected a list starting with 'search'".to_string()),
    }
}

fn parse_search_args(args: &Value) -> Result<QueryRequest, String> {
    let mut query = String::new();
    let mut filters: Option<Vec<Filter>> = None;
    let mut rerank_strategy = RerankStrategy::SemanticOnly;
    let mut final_top_k = 10;

    let mut current = args;
    while let Value::Cons(cell) = current {
        let key = cell.car().as_symbol().ok_or("Expected keyword symbol")?;
        let next_cell = cell.cdr().as_cons().ok_or(format!("Expected value after {}", key))?;
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
            _ => return Err(format!("Unknown parameter: {}", key)),
        }

        current = next_cell.cdr();
    }

    Ok(QueryRequest {
        query,
        filters,
        rerank_strategy,
        final_top_k,
    })
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
            let strategy = cell.car().as_symbol().ok_or("Expected rerank strategy name")?;
            if strategy == "hybrid" {
                let rest = cell.cdr().as_cons().ok_or("Expected :weights after hybrid")?;
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
                    let bm25_weight = r3.car().as_f64().ok_or("bm25 weight must be a float")? as f32;

                    Ok(RerankStrategy::Hybrid {
                        semantic_weight: sem_weight,
                        bm25_weight: bm25_weight,
                    })
                } else {
                    Err("Expected weights list".to_string())
                }
            } else if strategy == "semantic" {
                Ok(RerankStrategy::SemanticOnly)
            } else if strategy == "bm25" {
                Ok(RerankStrategy::Bm25Only)
            } else {
                Err(format!("Unknown rerank strategy: {}", strategy))
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
        let request = parse_dsl(input).expect("Failed to parse valid DSL");

        assert_eq!(request.query, "quantum physics");
        assert_eq!(request.final_top_k, 5);

        match request.rerank_strategy {
            RerankStrategy::Hybrid { semantic_weight, bm25_weight } => {
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

    #[test]
    fn test_parse_semantic_rerank() {
         let input = r#"(search :query "test" :rerank (semantic) :final-top-k 10)"#;
         let request = parse_dsl(input).expect("Failed to parse valid DSL");
         assert_eq!(request.query, "test");
         assert_eq!(request.final_top_k, 10);
         match request.rerank_strategy {
             RerankStrategy::SemanticOnly => {}
             _ => panic!("Expected SemanticOnly rerank strategy"),
         }
    }
}
