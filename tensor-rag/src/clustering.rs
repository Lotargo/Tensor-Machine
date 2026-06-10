use ndarray::{Array1, Array2};
use rand::SeedableRng;
use std::collections::HashMap;

// exact semantic layout limit is now loaded dynamically from config


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
pub struct UmapReducer {
    pub n_components: usize,
    pub n_neighbors: usize,
    pub min_dist: f32,
    pub seed: u64,
}

impl Default for UmapReducer {
    fn default() -> Self {
        Self {
            n_components: 3,
            n_neighbors: 15,
            min_dist: 0.1,
            seed: 42,
        }
    }
}

impl DimensionReducer for UmapReducer {
    fn reduce(&self, vectors: &[Vec<f32>]) -> Vec<Vec<f32>> {
        let n = vectors.len();
        if n < 2 || n <= self.n_neighbors {
            return PcaFallbackReducer {
                components: self.n_components,
            }
            .reduce(vectors);
        }
        if vectors.iter().any(|v| v.len() < 2) {
            return PcaFallbackReducer {
                components: self.n_components,
            }
            .reduce(vectors);
        }

        let min_dim = vectors.iter().map(|v| v.len()).min().unwrap_or(0);
        if min_dim < 2 {
            return Vec::new();
        }

        let mut data_arr = Array2::zeros((n, min_dim));
        for (i, v) in vectors.iter().enumerate() {
            let mut norm = 0.0f32;
            for j in 0..min_dim {
                let val = v.get(j).copied().unwrap_or(0.0);
                data_arr[[i, j]] = val;
                norm += val * val;
            }
            norm = norm.sqrt();
            if norm > 0.0 {
                for j in 0..min_dim {
                    data_arr[[i, j]] /= norm;
                }
            }
        }

        let n_neighbors = self.n_neighbors.min(n - 1);

        let knn_data: Vec<Array1<f32>> = data_arr
            .rows()
            .into_iter()
            .map(|row| row.to_owned())
            .collect();

        let (knn_indices, knn_dists) = compute_brute_force_knn(&knn_data, n_neighbors);

        let init = if self.n_components <= min_dim {
            pca_init(&knn_data, self.n_components, self.seed)
        } else {
            random_init(n, self.n_components, self.seed)
        };

        let config = umap_rs::UmapConfig {
            n_components: self.n_components,
            manifold: umap_rs::ManifoldParams {
                min_dist: self.min_dist,
                ..Default::default()
            },
            graph: umap_rs::GraphParams {
                n_neighbors,
                ..Default::default()
            },
            ..Default::default()
        };

        let umap = umap_rs::Umap::new(config);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            umap.fit(
                data_arr.view(),
                knn_indices.view(),
                knn_dists.view(),
                init.view(),
            )
        }));

        match result {
            Ok(fitted) => {
                let embedding = fitted.into_embedding();
                embedding
                    .rows()
                    .into_iter()
                    .map(|row| row.to_vec())
                    .collect()
            }
            Err(_) => PcaFallbackReducer {
                components: self.n_components,
            }
            .reduce(vectors),
        }
    }
}

fn compute_brute_force_knn(data: &[Array1<f32>], k: usize) -> (Array2<u32>, Array2<f32>) {
    let n = data.len();
    let mut indices = Array2::zeros((n, k));
    let mut dists = Array2::zeros((n, k));

    for i in 0..n {
        let mut pairs: Vec<(usize, f32)> = data
            .iter()
            .enumerate()
            .map(|(j, v)| {
                let d = (&data[i] - v).mapv(|x| x * x).sum().sqrt();
                (j, d)
            })
            .collect();
        pairs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (idx, (j, d)) in pairs.iter().skip(1).take(k).enumerate() {
            indices[[i, idx]] = *j as u32;
            dists[[i, idx]] = *d;
        }
    }

    (indices, dists)
}

fn pca_init(data: &[Array1<f32>], n_components: usize, seed: u64) -> Array2<f32> {
    let n = data.len();
    let dim = data[0].len();
    if n == 0 || dim == 0 {
        return Array2::zeros((n, n_components));
    }

    let mean = {
        let mut sum = Array1::zeros(dim);
        for v in data {
            sum += v;
        }
        sum / n as f32
    };

    let centered: Vec<Array1<f32>> = data.iter().map(|v| v - &mean).collect();

    let n_comp = n_components.min(dim).min(n);

    let mut result = Array2::zeros((n, n_comp));

    for c in 0..n_comp {
        let mut v = Array1::from_elem(dim, {
            use rand::Rng;
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed + c as u64);
            rng.random_range(-1.0..1.0)
        });
        let s: f32 = v.dot(&v);
        let norm = s.sqrt();
        if norm > 0.0 {
            v /= norm;
        }

        for _ in 0..3 {
            let v_dot_data: Vec<f32> = centered.iter().map(|x| v.dot(x)).collect();
            let mut new_v = Array1::zeros(dim);
            for (i, x) in centered.iter().enumerate() {
                new_v = &new_v + &(x * v_dot_data[i]);
            }
            let ns: f32 = new_v.dot(&new_v);
            let new_norm = ns.sqrt();
            if new_norm > 0.0 {
                new_v /= new_norm;
            }
            v = new_v;
        }

        for (i, x) in centered.iter().enumerate() {
            result[[i, c]] = v.dot(x);
        }
    }

    result
}

fn random_init(n: usize, n_components: usize, seed: u64) -> Array2<f32> {
    use rand::Rng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut result = Array2::zeros((n, n_components));
    for val in result.iter_mut() {
        *val = rng.random_range(-0.01..0.01);
    }
    result
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

pub fn build_semantic_cluster_layout(
    chunk_vectors: &[(i32, Vec<f32>)],
    umap_config: Option<UmapReducer>,
) -> SemanticClusterLayout {
    let chunk_indexes = chunk_vectors
        .iter()
        .map(|(chunk_index, _)| *chunk_index)
        .collect::<Vec<_>>();
    let vectors = chunk_vectors
        .iter()
        .map(|(_, vector)| vector.clone())
        .collect::<Vec<_>>();
    if vectors.len() > crate::config::get_exact_semantic_layout_limit() {
        return build_coarse_semantic_cluster_layout(&chunk_indexes, &vectors);
    }

    let clusterer = HdbscanClusterer::default();
    let assignments = clusterer.cluster(&vectors);

    let reducer = umap_config.unwrap_or_default();
    let umap_coords = reducer.reduce(&vectors);
    let use_umap = umap_coords.len() == vectors.len() && !umap_coords.is_empty();

    let points = assignments
        .iter()
        .enumerate()
        .map(|(idx, assignment)| {
            let layout_hint = if use_umap && idx < umap_coords.len() {
                let coords = &umap_coords[idx];
                [
                    finite_f32(coords.first().copied().unwrap_or(0.0)),
                    finite_f32(coords.get(1).copied().unwrap_or(0.0)),
                    finite_f32(coords.get(2).copied().unwrap_or(0.0)),
                ]
            } else {
                vector_layout_hint(vectors.get(idx))
            };
            SemanticClusterPoint {
                chunk_index: chunk_indexes[idx],
                cluster_id: assignment.cluster_id,
                distance_to_centroid: finite_f32(assignment.distance_to_centroid),
                layout_hint,
            }
        })
        .collect::<Vec<_>>();
    let clusters = summarize_semantic_clusters(&points);

    SemanticClusterLayout {
        algorithm: "hdbscan".to_string(),
        projection: if use_umap {
            "umap-3d".to_string()
        } else {
            "normalized-prefix-projection".to_string()
        },
        points,
        clusters,
    }
}

fn build_coarse_semantic_cluster_layout(
    chunk_indexes: &[i32],
    vectors: &[Vec<f32>],
) -> SemanticClusterLayout {
    let hints = vectors
        .iter()
        .map(|vector| vector_layout_hint(Some(vector)))
        .collect::<Vec<_>>();
    let mut bucket_counts: HashMap<(i32, i32, i32), usize> = HashMap::new();
    for hint in &hints {
        *bucket_counts.entry(layout_bucket(*hint)).or_insert(0) += 1;
    }

    let mut bucket_labels = HashMap::new();
    let mut next_cluster_id = 0;
    for (bucket, count) in &bucket_counts {
        if *count >= 4 {
            bucket_labels.insert(*bucket, next_cluster_id);
            next_cluster_id += 1;
        }
    }

    let points = hints
        .into_iter()
        .enumerate()
        .map(|(idx, layout_hint)| {
            let cluster_id = bucket_labels
                .get(&layout_bucket(layout_hint))
                .copied()
                .unwrap_or(-1);
            SemanticClusterPoint {
                chunk_index: chunk_indexes[idx],
                cluster_id,
                distance_to_centroid: 0.0,
                layout_hint,
            }
        })
        .collect::<Vec<_>>();
    let clusters = summarize_semantic_clusters(&points);

    SemanticClusterLayout {
        algorithm: "coarse-projection".to_string(),
        projection: "normalized-prefix-projection".to_string(),
        points,
        clusters,
    }
}

fn layout_bucket(layout_hint: [f32; 3]) -> (i32, i32, i32) {
    (
        (layout_hint[0] * 150.0).round() as i32,
        (layout_hint[1] * 150.0).round() as i32,
        (layout_hint[2] * 150.0).round() as i32,
    )
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

pub fn summarize_semantic_clusters(points: &[SemanticClusterPoint]) -> Vec<SemanticClusterSummary> {
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
    use super::{Clusterer, DimensionReducer, HdbscanClusterer, UmapReducer};
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

    #[test]
    fn umap_reducer_reduces_dimensions() {
        let vectors: Vec<Vec<f32>> = (0..50)
            .map(|i| {
                let base = (i / 10) as f32;
                (0..8)
                    .map(|j| base + (j as f32 * 0.1) + ((i % 10) as f32 * 0.01))
                    .collect()
            })
            .collect();

        let reducer = UmapReducer {
            n_components: 2,
            n_neighbors: 5,
            min_dist: 0.1,
            seed: 42,
        };
        let reduced = reducer.reduce(&vectors);
        assert_eq!(reduced.len(), 50);
        assert_eq!(reduced[0].len(), 2);
    }

    #[test]
    fn umap_reducer_few_points_falls_back() {
        let vectors = vec![vec![1.0, 2.0, 3.0], vec![1.1, 2.0, 3.0]];
        let reducer = UmapReducer::default();
        let reduced = reducer.reduce(&vectors);
        assert_eq!(reduced.len(), 2);
        assert_eq!(reduced[0].len(), 3);
    }

    #[test]
    fn umap_reducer_small_vectors_falls_back() {
        let vectors = vec![vec![1.0]];
        let reducer = UmapReducer::default();
        let reduced = reducer.reduce(&vectors);
        assert_eq!(reduced.len(), 1);
    }

    #[test]
    fn coarse_clustering_assigns_multiple_clusters_with_scaling() {
        let vectors = vec![
            vec![0.03, 0.015, -0.012, 0.1],
            vec![0.03, 0.015, -0.012, 0.2],
            vec![0.03, 0.015, -0.012, 0.3],
            vec![0.03, 0.015, -0.012, 0.4],
            vec![-0.03, -0.015, 0.012, 0.1],
            vec![-0.03, -0.015, 0.012, 0.2],
            vec![-0.03, -0.015, 0.012, 0.3],
            vec![-0.03, -0.015, 0.012, 0.4],
        ];
        let chunk_indexes = vec![0, 1, 2, 3, 4, 5, 6, 7];
        let layout = super::build_coarse_semantic_cluster_layout(&chunk_indexes, &vectors);
        let cluster_ids = layout.points.iter().map(|p| p.cluster_id).collect::<std::collections::HashSet<_>>();
        assert!(cluster_ids.len() >= 2);
    }
}
