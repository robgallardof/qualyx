//! Statistics for regression decisions: percentiles, coefficient of variation and the
//! one-sided Mann–Whitney U test (normal approximation with tie correction).

/// Nearest-rank percentile of an unsorted sample (`q` in 0..=100).
pub fn percentile(sample: &[f64], q: f64) -> Option<f64> {
    if sample.is_empty() {
        return None;
    }
    let mut v = sample.to_vec();
    v.sort_by(f64::total_cmp);
    let rank = ((q / 100.0) * v.len() as f64).ceil().max(1.0) as usize;
    Some(v[rank.min(v.len()) - 1])
}

pub fn median(sample: &[f64]) -> Option<f64> {
    percentile(sample, 50.0)
}

/// Standard deviation / mean. `None` for fewer than two values or a zero mean.
pub fn coefficient_of_variation(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    if mean == 0.0 {
        return None;
    }
    let var = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Some(var.sqrt() / mean)
}

/// Standard normal CDF (Abramowitz & Stegun 7.1.26 erf, |error| < 1.5e-7).
pub fn normal_cdf(z: f64) -> f64 {
    let x = z / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    let erf = if x >= 0.0 { erf } else { -erf };
    0.5 * (1.0 + erf)
}

/// One-sided p-value for "candidate tends to be larger than baseline".
pub fn mann_whitney_greater(baseline: &[f64], candidate: &[f64]) -> Option<f64> {
    let (n1, n2) = (candidate.len() as f64, baseline.len() as f64);
    if candidate.is_empty() || baseline.is_empty() {
        return None;
    }
    // (value, is_candidate)
    let mut all: Vec<(f64, bool)> = candidate
        .iter()
        .map(|&v| (v, true))
        .chain(baseline.iter().map(|&v| (v, false)))
        .collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let n = all.len();
    let mut rank_sum_candidate = 0.0;
    let mut tie_term = 0.0;
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && all[j + 1].0 == all[i].0 {
            j += 1;
        }
        let avg_rank = (i + j) as f64 / 2.0 + 1.0;
        let t = (j - i + 1) as f64;
        tie_term += t * t * t - t;
        for item in &all[i..=j] {
            if item.1 {
                rank_sum_candidate += avg_rank;
            }
        }
        i = j + 1;
    }
    let u = rank_sum_candidate - n1 * (n1 + 1.0) / 2.0;
    let mean = n1 * n2 / 2.0;
    let nn = n1 + n2;
    let var = n1 * n2 / 12.0 * ((nn + 1.0) - tie_term / (nn * (nn - 1.0)));
    if var <= 0.0 {
        return Some(0.5);
    }
    let z = (u - mean - 0.5) / var.sqrt(); // continuity correction
    Some(1.0 - normal_cdf(z))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 50.0), Some(50.0));
        assert_eq!(percentile(&v, 95.0), Some(95.0));
        assert_eq!(percentile(&v, 100.0), Some(100.0));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn normal_cdf_known_points() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-7);
        assert!((normal_cdf(1.96) - 0.975).abs() < 1e-4);
        assert!((normal_cdf(-1.96) - 0.025).abs() < 1e-4);
    }

    /// Deterministic pseudo-random sample (no RNG dependency).
    fn sample(n: usize, base: f64, seed: u64) -> Vec<f64> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                base + (x >> 33) as f64 / (1u64 << 31) as f64 * 4.0
            })
            .collect()
    }

    #[test]
    fn same_distribution_is_not_significant() {
        let p = mann_whitney_greater(&sample(400, 10.0, 1), &sample(400, 10.0, 2)).unwrap();
        assert!(p > 0.05, "p = {p}");
    }

    #[test]
    fn shifted_distribution_is_significant_one_sided() {
        let base = sample(400, 10.0, 1);
        let slower = sample(400, 12.0, 2);
        assert!(mann_whitney_greater(&base, &slower).unwrap() < 1e-6);
        // Faster candidate is not a regression.
        assert!(mann_whitney_greater(&slower, &base).unwrap() > 0.99);
    }

    #[test]
    fn all_ties_are_handled() {
        assert_eq!(mann_whitney_greater(&[5.0; 10], &[5.0; 10]), Some(0.5));
    }

    #[test]
    fn cv() {
        assert_eq!(coefficient_of_variation(&[1.0]), None);
        assert!(coefficient_of_variation(&[10.0, 10.0, 10.0]).unwrap() < 1e-12);
        assert!(coefficient_of_variation(&[5.0, 15.0]).unwrap() > 0.5);
    }
}
