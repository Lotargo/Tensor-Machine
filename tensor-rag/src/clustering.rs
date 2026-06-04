use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ClusterAssignment {
    pub cluster_id: i32,
    pub distance_to_centroid: f32,
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
