use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ClusterAssignment {
    pub cluster_id: i32,
    pub distance_to_centroid: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticClusterPoint {
    pub chunk_index: i32,
    pub cluster_id: i32,
    pub distance_to_centroid: f32,
    pub layout_hint: [f32; 3],
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticClusterSummary {
    pub cluster_id: i32,
    pub size: usize,
    pub medoid_chunk_index: i32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticClusterLayout {
    pub algorithm: String,
    pub projection: String,
    pub points: Vec<SemanticClusterPoint>,
    pub clusters: Vec<SemanticClusterSummary>,
}

pub trait DimensionReducer {
    fn reduce(&self, vectors: &[Vec<f32>]) -> Vec<Vec<f32>>;
}

pub trait Clusterer {
    fn cluster(&self, vectors: &[Vec<f32>]) -> Vec<ClusterAssignment>;
}

#[derive(Debug, Clone)]
pub struct PcaFallbackReducer {
    pub components: usize,
}

impl DimensionReducer for PcaFallbackReducer {
    fn reduce(&self, vectors: &[Vec<f32>]) -> Vec<Vec<f32>> {
        vectors
            .iter()
            .map(|v| v.iter().take(self.components).copied().collect())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct RadiusClusterer {
    pub min_cluster_size: usize,
    pub similarity_threshold: f32,
}

impl Default for RadiusClusterer {
    fn default() -> Self {
        Self {
            min_cluster_size: 2,
            similarity_threshold: 0.88,
        }
    }
}

impl Clusterer for RadiusClusterer {
    fn cluster(&self, vectors: &[Vec<f32>]) -> Vec<ClusterAssignment> {
        let mut labels = vec![-1; vectors.len()];
        let mut current = 0;
        for i in 0..vectors.len() {
            if labels[i] != -1 {
                continue;
            }
            let mut group = vec![i];
            for j in (i + 1)..vectors.len() {
                if cosine(&vectors[i], &vectors[j]) >= self.similarity_threshold {
                    group.push(j);
                }
            }
            if group.len() >= self.min_cluster_size {
                for idx in group {
                    labels[idx] = current;
                }
                current += 1;
            }
        }
        labels
            .into_iter()
            .map(|cluster_id| ClusterAssignment {
                cluster_id,
                distance_to_centroid: 0.0,
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct HdbscanClusterer {
    pub min_cluster_size: usize,
    pub min_samples: usize,
    pub projection_components: usize,
    pub fallback: RadiusClusterer,
}

impl Default for HdbscanClusterer {
    fn default() -> Self {
        Self {
            min_cluster_size: 4,
            min_samples: 2,
            projection_components: 32,
            fallback: RadiusClusterer::default(),
        }
    }
}

impl Clusterer for HdbscanClusterer {
    fn cluster(&self, vectors: &[Vec<f32>]) -> Vec<ClusterAssignment> {
        if vectors.len() < self.min_cluster_size || vectors.iter().any(|v| v.is_empty()) {
            return self.fallback.cluster(vectors);
        }

        let prepared = prepare_density_vectors(vectors, self.projection_components);
        if prepared.len() != vectors.len() || prepared.iter().any(|v| v.is_empty()) {
            return self.fallback.cluster(vectors);
        }

        let config = hdbscan::HdbscanHyperParams::builder()
            .min_cluster_size(self.min_cluster_size)
            .min_samples(self.min_samples)
            .build();
        let clusterer = hdbscan::Hdbscan::new(&prepared, config);
        let labels = match clusterer.cluster() {
            Ok(labels) if labels.len() == vectors.len() => labels,
            _ => return self.fallback.cluster(vectors),
        };

        let centroids = calculate_centroids(&prepared, &labels);
        labels
            .into_iter()
            .enumerate()
            .map(|(idx, cluster_id)| {
                let distance_to_centroid = centroids
                    .get(&cluster_id)
                    .map(|centroid| euclidean_distance(&prepared[idx], centroid))
                    .unwrap_or(0.0);
                ClusterAssignment {
                    cluster_id,
                    distance_to_centroid,
                }
            })
            .collect()
    }
}

pub fn cluster_diversity_bonus(assignments: &[ClusterAssignment]) -> Vec<f32> {
    let mut seen: HashMap<i32, usize> = HashMap::new();
    assignments
        .iter()
        .map(|assignment| {
            if assignment.cluster_id < 0 {
                return 0.2;
            }
            let count = seen.entry(assignment.cluster_id).or_insert(0);
            let bonus = match *count {
                0 => 1.0,
                1 => 0.5,
                _ => 0.2,
            };
            *count += 1;
            bonus
        })
        .collect()
}

pub fn build_semantic_cluster_layout(chunk_vectors: &[(i32, Vec<f32>)]) -> SemanticClusterLayout {
    let chunk_indexes = chunk_vectors
        .iter()
        .map(|(chunk_index, _)| *chunk_index)
        .collect::<Vec<_>>();
    let vectors = chunk_vectors
        .iter()
        .map(|(_, vector)| vector.clone())
        .collect::<Vec<_>>();
    let clusterer = HdbscanClusterer::default();
    let assignments = clusterer.cluster(&vectors);
    let prepared = prepare_density_vectors(&vectors, clusterer.projection_components);

    let points = assignments
        .iter()
        .enumerate()
        .map(|(idx, assignment)| SemanticClusterPoint {
            chunk_index: chunk_indexes[idx],
            cluster_id: assignment.cluster_id,
            distance_to_centroid: finite_f32(assignment.distance_to_centroid),
            layout_hint: vector_layout_hint(prepared.get(idx)),
        })
        .collect::<Vec<_>>();
    let clusters = summarize_semantic_clusters(&points);

    SemanticClusterLayout {
        algorithm: "hdbscan".to_string(),
        projection: "normalized-prefix-projection".to_string(),
        points,
        clusters,
    }
}

fn prepare_density_vectors(vectors: &[Vec<f32>], components: usize) -> Vec<Vec<f32>> {
    let target_dim = vectors
        .iter()
        .map(|v| v.len())
        .min()
        .unwrap_or(0)
        .min(components.max(2));
    if target_dim == 0 {
        return Vec::new();
    }

    vectors
        .iter()
        .map(|vector| {
            let mut projected = vector
                .iter()
                .take(target_dim)
                .map(|value| if value.is_finite() { *value } else { 0.0 })
                .collect::<Vec<_>>();
            l2_normalize(&mut projected);
            projected
        })
        .collect()
}

fn summarize_semantic_clusters(points: &[SemanticClusterPoint]) -> Vec<SemanticClusterSummary> {
    let mut groups: HashMap<i32, Vec<&SemanticClusterPoint>> = HashMap::new();
    for point in points {
        if point.cluster_id < 0 {
            continue;
        }
        groups.entry(point.cluster_id).or_default().push(point);
    }

    let mut summaries = groups
        .into_iter()
        .map(|(cluster_id, mut points)| {
            points.sort_by(|a, b| {
                a.distance_to_centroid
                    .partial_cmp(&b.distance_to_centroid)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            SemanticClusterSummary {
                cluster_id,
                size: points.len(),
                medoid_chunk_index: points
                    .first()
                    .map(|point| point.chunk_index)
                    .unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    summaries.sort_by_key(|summary| summary.cluster_id);
    summaries
}

fn vector_layout_hint(vector: Option<&Vec<f32>>) -> [f32; 3] {
    let Some(vector) = vector else {
        return [0.0, 0.0, 0.0];
    };
    [
        finite_f32(vector.first().copied().unwrap_or(0.0)),
        finite_f32(vector.get(1).copied().unwrap_or(0.0)),
        finite_f32(vector.get(2).copied().unwrap_or(0.0)),
    ]
}

fn finite_f32(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn calculate_centroids(vectors: &[Vec<f32>], labels: &[i32]) -> HashMap<i32, Vec<f32>> {
    let mut sums: HashMap<i32, Vec<f32>> = HashMap::new();
    let mut counts: HashMap<i32, usize> = HashMap::new();

    for (vector, label) in vectors.iter().zip(labels.iter().copied()) {
        if label < 0 {
            continue;
        }
        let sum = sums.entry(label).or_insert_with(|| vec![0.0; vector.len()]);
        for (idx, value) in vector.iter().enumerate() {
            sum[idx] += *value;
        }
        *counts.entry(label).or_insert(0) += 1;
    }

    for (label, sum) in sums.iter_mut() {
        let count = counts.get(label).copied().unwrap_or(1).max(1) as f32;
        for value in sum.iter_mut() {
            *value /= count;
        }
    }

    sums
}

fn l2_normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm == 0.0 {
        return;
    }
    for value in vector.iter_mut() {
        *value /= norm;
    }
}

fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let delta = x - y;
            delta * delta
        })
        .sum::<f32>()
        .sqrt()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0;
    let mut an = 0.0;
    let mut bn = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        an += x * x;
        bn += y * y;
    }
    if an == 0.0 || bn == 0.0 {
        0.0
    } else {
        dot / (an.sqrt() * bn.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::{Clusterer, HdbscanClusterer};
    use std::collections::HashSet;

    #[test]
    fn hdbscan_clusterer_finds_multiple_dense_groups() {
        let vectors = vec![
            vec![1.0, 1.0, 0.0],
            vec![1.1, 1.0, 0.0],
            vec![0.9, 1.0, 0.0],
            vec![1.0, 1.1, 0.0],
            vec![-1.0, -1.0, 0.0],
            vec![-1.1, -1.0, 0.0],
            vec![-0.9, -1.0, 0.0],
            vec![-1.0, -1.1, 0.0],
            vec![0.0, 5.0, 0.0],
        ];
        let clusterer = HdbscanClusterer {
            min_cluster_size: 3,
            min_samples: 2,
            projection_components: 3,
            ..HdbscanClusterer::default()
        };

        let assignments = clusterer.cluster(&vectors);
        let dense_labels = assignments
            .iter()
            .filter_map(|assignment| (assignment.cluster_id >= 0).then_some(assignment.cluster_id))
            .collect::<HashSet<_>>();

        assert!(dense_labels.len() >= 2);
        assert_eq!(assignments.len(), vectors.len());
    }
}
