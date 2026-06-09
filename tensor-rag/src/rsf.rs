use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RsfChannel {
    Dense,
    Bm25,
    Entity,
    Structural,
    Recency,
    Cluster,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsfConfig {
    pub dense_weight: f32,
    pub bm25_weight: f32,
    pub entity_weight: f32,
    pub structural_weight: f32,
    pub recency_weight: f32,
    pub cluster_weight: f32,
}

impl Default for RsfConfig {
    fn default() -> Self {
        Self {
            dense_weight: std::env::var("RSF_DENSE_WEIGHT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.30),
            bm25_weight: std::env::var("RSF_BM25_WEIGHT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.35),
            entity_weight: std::env::var("RSF_ENTITY_WEIGHT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.10),
            structural_weight: std::env::var("RSF_STRUCTURAL_WEIGHT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.10),
            recency_weight: std::env::var("RSF_RECENCY_WEIGHT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.05),
            cluster_weight: std::env::var("RSF_CLUSTER_WEIGHT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.10),
        }
    }
}

pub fn compute_rsf(
    ids: &[usize],
    channels: &[(RsfChannel, Vec<f32>)],
    config: &RsfConfig,
) -> Vec<(usize, f32)> {
    compute_rsf_with_query(ids, channels, config, None)
}

pub fn compute_rsf_with_query(
    ids: &[usize],
    channels: &[(RsfChannel, Vec<f32>)],
    config: &RsfConfig,
    query: Option<&str>,
) -> Vec<(usize, f32)> {
    let adjusted = query.map(|q| adaptive_config(config, q)).unwrap_or_else(|| config.clone());

    let mut scores = vec![0.0f32; ids.len()];
    for (channel, raw_scores) in channels {
        let normalized = minmax(raw_scores);
        let weight = channel_weight(*channel, &adjusted);
        for (idx, score) in normalized.into_iter().enumerate().take(scores.len()) {
            scores[idx] += weight * score;
        }
    }

    let mut fused: Vec<(usize, f32)> = ids.iter().copied().zip(scores).collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

pub fn adaptive_config(config: &RsfConfig, query: &str) -> RsfConfig {
    let wc = word_count(query);
    if wc >= 6 {
        return config.clone();
    }

    let short_factor = (wc as f32 / 6.0).clamp(0.0, 1.0);

    let dense_boost = 0.15 * (1.0 - short_factor);
    let bm25_penalty = 0.10 * (1.0 - short_factor);

    let new_dense = (config.dense_weight + dense_boost).min(0.60);
    let new_bm25 = (config.bm25_weight - bm25_penalty).max(0.15);

    let scale = (config.dense_weight + config.bm25_weight + config.entity_weight
        + config.structural_weight + config.recency_weight + config.cluster_weight)
        / (new_dense + new_bm25 + config.entity_weight
            + config.structural_weight + config.recency_weight + config.cluster_weight);

    RsfConfig {
        dense_weight: new_dense,
        bm25_weight: new_bm25,
        entity_weight: config.entity_weight * scale,
        structural_weight: config.structural_weight * scale,
        recency_weight: config.recency_weight * scale,
        cluster_weight: config.cluster_weight * scale,
    }
}

pub fn reciprocal_rank_fusion(lists: &[Vec<usize>], k: usize) -> Vec<(usize, f32)> {
    let mut scores: HashMap<usize, f32> = HashMap::new();
    let k = k.max(1) as f32;
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *scores.entry(*id).or_insert(0.0) += 1.0 / (k + rank as f32 + 1.0);
        }
    }
    let mut fused: Vec<_> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

fn minmax(values: &[f32]) -> Vec<f32> {
    if values.is_empty() {
        return Vec::new();
    }
    let min = values.iter().copied().fold(f32::INFINITY, f32::min);
    let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if (max - min).abs() < f32::EPSILON {
        return values
            .iter()
            .map(|v| if *v > 0.0 { 1.0 } else { 0.0 })
            .collect();
    }
    values.iter().map(|v| (v - min) / (max - min)).collect()
}

fn channel_weight(channel: RsfChannel, config: &RsfConfig) -> f32 {
    match channel {
        RsfChannel::Dense => config.dense_weight,
        RsfChannel::Bm25 => config.bm25_weight,
        RsfChannel::Entity => config.entity_weight,
        RsfChannel::Structural => config.structural_weight,
        RsfChannel::Recency => config.recency_weight,
        RsfChannel::Cluster => config.cluster_weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuses_weighted_channels() {
        let fused = compute_rsf(
            &[10, 20],
            &[
                (RsfChannel::Dense, vec![0.2, 0.9]),
                (RsfChannel::Bm25, vec![1.0, 0.0]),
            ],
            &RsfConfig::default(),
        );
        assert_eq!(fused[0].0, 10);
    }
}
