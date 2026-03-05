use serde::{Deserialize, Serialize};

/// Distribution statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub median: f64,
    pub p90: f64,
    pub p99: f64,
    pub count: usize,
}

impl Distribution {
    /// Calculate distribution from a list of values
    pub fn from_values(mut values: Vec<f64>) -> Self {
        if values.is_empty() {
            return Self {
                min: 0.0,
                max: 0.0,
                mean: 0.0,
                median: 0.0,
                p90: 0.0,
                p99: 0.0,
                count: 0,
            };
        }

        values.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let count = values.len();
        let min = values[0];
        let max = values[count - 1];
        let mean = values.iter().sum::<f64>() / count as f64;
        let median = percentile(&values, 0.50);
        let p90 = percentile(&values, 0.90);
        let p99 = percentile(&values, 0.99);

        Self {
            min,
            max,
            mean,
            median,
            p90,
            p99,
            count,
        }
    }
}

/// Calculate percentile from sorted values
fn percentile(sorted_values: &[f64], p: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }

    let idx = (p * (sorted_values.len() - 1) as f64) as usize;
    sorted_values[idx]
}

/// Calculate convergence rate
pub fn convergence_rate(converged: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (converged as f64) / (total as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distribution() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let dist = Distribution::from_values(values);

        assert_eq!(dist.min, 1.0);
        assert_eq!(dist.max, 10.0);
        assert_eq!(dist.mean, 5.5);
        assert_eq!(dist.median, 5.0);
        assert_eq!(dist.count, 10);
    }

    #[test]
    fn test_empty_distribution() {
        let dist = Distribution::from_values(vec![]);
        assert_eq!(dist.count, 0);
        assert_eq!(dist.mean, 0.0);
    }

    #[test]
    fn test_convergence_rate() {
        assert_eq!(convergence_rate(7, 10), 0.7);
        assert_eq!(convergence_rate(0, 10), 0.0);
        assert_eq!(convergence_rate(10, 10), 1.0);
        assert_eq!(convergence_rate(0, 0), 0.0);
    }
}
