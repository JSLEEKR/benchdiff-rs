//! Statistical primitives: mean, variance, Welch's t-test, Cohen's d,
//! Benjamini-Hochberg FDR correction, and a regularized incomplete beta
//! for computing t-distribution p-values without pulling in `statrs`.

use crate::error::{Error, Result};

/// Kahan compensated sum — resistant to drift for large `n`.
#[must_use]
pub fn kahan_sum(xs: &[f64]) -> f64 {
    let mut sum = 0.0_f64;
    let mut comp = 0.0_f64;
    for &x in xs {
        let y = x - comp;
        let t = sum + y;
        comp = (t - sum) - y;
        sum = t;
    }
    sum
}

/// Sample mean.
#[must_use]
pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    kahan_sum(xs) / xs.len() as f64
}

/// Bessel-corrected sample variance (divisor `n - 1`).
///
/// Returns `0.0` for `n < 2` — caller is responsible for checking.
#[must_use]
pub fn variance(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let mut s = 0.0_f64;
    for &x in xs {
        let d = x - m;
        s += d * d;
    }
    s / (xs.len() - 1) as f64
}

/// Standard deviation (sqrt of [`variance`]).
#[must_use]
pub fn stddev(xs: &[f64]) -> f64 {
    variance(xs).sqrt()
}

/// Median of a copy-sorted slice.
#[must_use]
pub fn median(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut v: Vec<f64> = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Result of a Welch's two-sample t-test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WelchResult {
    pub mean_a: f64,
    pub mean_b: f64,
    pub var_a: f64,
    pub var_b: f64,
    pub n_a: usize,
    pub n_b: usize,
    /// t statistic. `NaN` when both variances are zero.
    pub t: f64,
    /// Welch-Satterthwaite degrees of freedom.
    pub df: f64,
    /// Two-sided p-value in `[0, 1]`.
    pub p: f64,
}

/// Welch's two-sample t-test (unequal variance), two-sided.
///
/// Requires `n_a >= 2` and `n_b >= 2`; returns [`Error::InsufficientSamples`]
/// otherwise. Handles the edge case where both variances are zero: if the
/// means are equal returns `p = 1.0` (unchanged), otherwise `p = 0.0`
/// (deterministic difference).
pub fn welch_t_test(a: &[f64], b: &[f64]) -> Result<WelchResult> {
    if a.len() < 2 {
        return Err(Error::InsufficientSamples {
            name: "sample A".into(),
            need: 2,
            got: a.len(),
        });
    }
    if b.len() < 2 {
        return Err(Error::InsufficientSamples {
            name: "sample B".into(),
            need: 2,
            got: b.len(),
        });
    }

    let n_a = a.len();
    let n_b = b.len();
    let mean_a = mean(a);
    let mean_b = mean(b);
    let var_a = variance(a);
    let var_b = variance(b);

    // Degenerate case: both variances zero.
    if var_a == 0.0 && var_b == 0.0 {
        let same = (mean_a - mean_b).abs() < f64::EPSILON;
        return Ok(WelchResult {
            mean_a,
            mean_b,
            var_a,
            var_b,
            n_a,
            n_b,
            t: if same { 0.0 } else { f64::INFINITY },
            df: (n_a + n_b - 2) as f64,
            p: if same { 1.0 } else { 0.0 },
        });
    }

    let se2 = var_a / n_a as f64 + var_b / n_b as f64;
    let se = se2.sqrt();
    let t = (mean_b - mean_a) / se;

    // Welch-Satterthwaite df.
    let df_num = se2 * se2;
    let df_den = (var_a / n_a as f64).powi(2) / (n_a - 1) as f64
        + (var_b / n_b as f64).powi(2) / (n_b - 1) as f64;
    let df = if df_den > 0.0 {
        df_num / df_den
    } else {
        (n_a + n_b - 2) as f64
    };

    let p = two_sided_p(t, df);

    Ok(WelchResult {
        mean_a,
        mean_b,
        var_a,
        var_b,
        n_a,
        n_b,
        t,
        df,
        p,
    })
}

/// Cohen's d effect size for two independent samples, using pooled SD.
///
/// Returns `0.0` when both samples have zero variance and equal means;
/// returns `f64::INFINITY` when both variances are zero and means differ.
#[must_use]
pub fn cohens_d(a: &[f64], b: &[f64]) -> f64 {
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let mean_a = mean(a);
    let mean_b = mean(b);
    let var_a = variance(a);
    let var_b = variance(b);
    let n_a = a.len();
    let n_b = b.len();
    let pooled_var =
        ((n_a - 1) as f64 * var_a + (n_b - 1) as f64 * var_b) / (n_a + n_b - 2) as f64;
    if pooled_var == 0.0 {
        if (mean_a - mean_b).abs() < f64::EPSILON {
            0.0
        } else {
            f64::INFINITY
        }
    } else {
        (mean_b - mean_a) / pooled_var.sqrt()
    }
}

/// Two-sided p-value from a t-statistic with `df` degrees of freedom.
///
/// Uses `p = I_x(df/2, 1/2)` where `x = df / (df + t²)` — the relation between
/// the t-distribution and the regularized incomplete beta function.
#[must_use]
pub fn two_sided_p(t: f64, df: f64) -> f64 {
    if !t.is_finite() {
        return 0.0;
    }
    if df <= 0.0 {
        return 1.0;
    }
    let x = df / (df + t * t);
    let p = reg_incomplete_beta(x, df / 2.0, 0.5);
    p.clamp(0.0, 1.0)
}

/// Regularized incomplete beta function `I_x(a, b)` via Lentz's continued
/// fraction — Numerical Recipes in C §6.4.
#[must_use]
pub fn reg_incomplete_beta(x: f64, a: f64, b: f64) -> f64 {
    if !(0.0..=1.0).contains(&x) {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x == 1.0 {
        return 1.0;
    }

    // Choose whichever side converges faster, using the symmetry
    // I_x(a, b) = 1 - I_{1-x}(b, a).
    if x < (a + 1.0) / (a + b + 2.0) {
        let lbeta = ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b);
        let front = (a * x.ln() + b * (1.0 - x).ln() - lbeta).exp() / a;
        front * beta_cf(x, a, b)
    } else {
        let lbeta = ln_gamma(b) + ln_gamma(a) - ln_gamma(a + b);
        let front = (b * (1.0 - x).ln() + a * x.ln() - lbeta).exp() / b;
        1.0 - front * beta_cf(1.0 - x, b, a)
    }
}

fn beta_cf(x: f64, a: f64, b: f64) -> f64 {
    // Lentz's method.
    const MAX_ITER: usize = 300;
    const EPS: f64 = 3.0e-12;
    const FPMIN: f64 = 1.0e-300;

    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FPMIN {
        d = FPMIN;
    }
    d = 1.0 / d;
    let mut h = d;

    for m in 1..=MAX_ITER {
        let mf = m as f64;
        let m2 = 2.0 * mf;

        // Even step.
        let aa = mf * (b - mf) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        h *= d * c;

        // Odd step.
        let aa = -(a + mf) * (qab + mf) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            return h;
        }
    }
    h
}

/// Log-gamma via Lanczos approximation (good to ~15 digits).
#[must_use]
pub fn ln_gamma(z: f64) -> f64 {
    // Coefficients from Lanczos g=7 approximation.
    const G: f64 = 7.0;
    const COEFFS: [f64; 9] = [
        0.999_999_999_999_809_93,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_13,
        -176.615_029_162_140_59,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_571_6e-6,
        1.505_632_735_149_311_6e-7,
    ];

    if z < 0.5 {
        // Reflection.
        let pi = std::f64::consts::PI;
        return (pi / (pi * z).sin()).ln() - ln_gamma(1.0 - z);
    }
    let z = z - 1.0;
    let mut x = COEFFS[0];
    for (i, c) in COEFFS.iter().enumerate().skip(1) {
        x += c / (z + i as f64);
    }
    let t = z + G + 0.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (z + 0.5) * t.ln() - t + x.ln()
}

/// Benjamini-Hochberg FDR correction.
///
/// Given a slice of p-values, returns an equally-sized `Vec<bool>` where
/// `true` means "reject H0 at FDR `q`".
#[must_use]
pub fn benjamini_hochberg(pvals: &[f64], q: f64) -> Vec<bool> {
    let n = pvals.len();
    if n == 0 {
        return Vec::new();
    }
    let mut indexed: Vec<(usize, f64)> = pvals.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut reject = vec![false; n];
    let mut max_k = None;
    for (rank, &(_, p)) in indexed.iter().enumerate() {
        let threshold = (rank + 1) as f64 * q / n as f64;
        if p <= threshold {
            max_k = Some(rank);
        }
    }
    if let Some(k) = max_k {
        for (_rank, &(orig, _)) in indexed.iter().enumerate().take(k + 1) {
            reject[orig] = true;
        }
    }
    reject
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn kahan_sum_basic() {
        assert!(approx(kahan_sum(&[1.0, 2.0, 3.0]), 6.0, 1e-12));
        assert!(approx(kahan_sum(&[]), 0.0, 1e-12));
    }

    #[test]
    fn kahan_sum_resists_drift() {
        // Classic example: 1e20 + 1 - 1e20 drifts under naive sum.
        let xs: Vec<f64> = std::iter::repeat(0.1).take(10_000).collect();
        let k = kahan_sum(&xs);
        assert!((k - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn mean_of_range() {
        assert!(approx(mean(&[1.0, 2.0, 3.0, 4.0, 5.0]), 3.0, 1e-12));
    }

    #[test]
    fn mean_empty() {
        assert_eq!(mean(&[]), 0.0);
    }

    #[test]
    fn variance_known_dataset() {
        // Bessel-corrected variance of [2,4,4,4,5,5,7,9] is 4.0.
        let v = variance(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]);
        assert!(approx(v, 4.571_428_571_428_571, 1e-9));
    }

    #[test]
    fn variance_single_sample_zero() {
        assert_eq!(variance(&[42.0]), 0.0);
    }

    #[test]
    fn stddev_known() {
        let s = stddev(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!(approx(s, 1.581_138_830_084_189_7, 1e-9));
    }

    #[test]
    fn median_odd() {
        assert!(approx(median(&[3.0, 1.0, 2.0]), 2.0, 1e-12));
    }

    #[test]
    fn median_even() {
        assert!(approx(median(&[1.0, 2.0, 3.0, 4.0]), 2.5, 1e-12));
    }

    #[test]
    fn median_empty() {
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn welch_identical_samples_unchanged() {
        let a = vec![10.0, 10.5, 9.5, 10.1];
        let r = welch_t_test(&a, &a).unwrap();
        assert!(r.p > 0.99);
    }

    #[test]
    fn welch_zero_variance_equal_means() {
        let a = vec![5.0, 5.0, 5.0];
        let b = vec![5.0, 5.0, 5.0];
        let r = welch_t_test(&a, &b).unwrap();
        assert_eq!(r.p, 1.0);
    }

    #[test]
    fn welch_zero_variance_diff_means() {
        let a = vec![5.0, 5.0, 5.0];
        let b = vec![10.0, 10.0, 10.0];
        let r = welch_t_test(&a, &b).unwrap();
        assert_eq!(r.p, 0.0);
        assert!(r.t.is_infinite());
    }

    #[test]
    fn welch_detects_clear_difference() {
        let a = vec![100.0, 101.0, 99.0, 100.5, 99.5, 100.2, 99.8];
        let b = vec![200.0, 201.0, 199.0, 200.5, 199.5, 200.2, 199.8];
        let r = welch_t_test(&a, &b).unwrap();
        assert!(r.p < 0.001);
    }

    #[test]
    fn welch_insufficient_samples_errors() {
        let a = vec![1.0];
        let b = vec![2.0, 3.0];
        assert!(welch_t_test(&a, &b).is_err());
        assert!(welch_t_test(&b, &a).is_err());
    }

    #[test]
    fn welch_tails_symmetric() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b = vec![10.0, 11.0, 12.0, 13.0, 14.0];
        let r1 = welch_t_test(&a, &b).unwrap();
        let r2 = welch_t_test(&b, &a).unwrap();
        assert!(approx(r1.t, -r2.t, 1e-9));
        assert!(approx(r1.p, r2.p, 1e-9));
    }

    #[test]
    fn cohens_d_identical_zero() {
        let a = vec![1.0, 2.0, 3.0];
        assert!(cohens_d(&a, &a).abs() < 1e-12);
    }

    #[test]
    fn cohens_d_positive_for_increase() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b = vec![6.0, 7.0, 8.0, 9.0, 10.0];
        let d = cohens_d(&a, &b);
        assert!(d > 0.0);
    }

    #[test]
    fn cohens_d_zero_var_different_mean_infinite() {
        let a = vec![1.0, 1.0, 1.0];
        let b = vec![2.0, 2.0, 2.0];
        let d = cohens_d(&a, &b);
        assert!(d.is_infinite());
    }

    #[test]
    fn cohens_d_small_sample_returns_zero() {
        assert_eq!(cohens_d(&[1.0], &[2.0, 3.0]), 0.0);
    }

    #[test]
    fn two_sided_p_bounds() {
        let p = two_sided_p(0.0, 10.0);
        assert!(approx(p, 1.0, 1e-9));
        let p = two_sided_p(10.0, 10.0);
        assert!(p < 0.01);
        let p = two_sided_p(-10.0, 10.0);
        assert!(p < 0.01);
    }

    #[test]
    fn two_sided_p_in_unit_interval() {
        for &t in &[-5.0, -1.0, 0.0, 0.5, 1.0, 2.5, 5.0] {
            let p = two_sided_p(t, 20.0);
            assert!((0.0..=1.0).contains(&p));
        }
    }

    #[test]
    fn ln_gamma_known_values() {
        assert!(approx(ln_gamma(1.0), 0.0, 1e-9));
        assert!(approx(ln_gamma(2.0), 0.0, 1e-9));
        assert!(approx(ln_gamma(3.0), 2.0_f64.ln(), 1e-9));
        assert!(approx(ln_gamma(4.0), 6.0_f64.ln(), 1e-9));
        assert!(approx(ln_gamma(5.0), 24.0_f64.ln(), 1e-9));
    }

    #[test]
    fn reg_incomplete_beta_endpoints() {
        assert_eq!(reg_incomplete_beta(0.0, 2.0, 2.0), 0.0);
        assert_eq!(reg_incomplete_beta(1.0, 2.0, 2.0), 1.0);
    }

    #[test]
    fn reg_incomplete_beta_symmetry() {
        // I_0.5(a,a) = 0.5
        let v = reg_incomplete_beta(0.5, 3.0, 3.0);
        assert!(approx(v, 0.5, 1e-6));
    }

    #[test]
    fn bh_empty() {
        assert_eq!(benjamini_hochberg(&[], 0.05).len(), 0);
    }

    #[test]
    fn bh_all_pass() {
        let r = benjamini_hochberg(&[0.001, 0.002, 0.003], 0.05);
        assert_eq!(r, vec![true, true, true]);
    }

    #[test]
    fn bh_all_fail() {
        let r = benjamini_hochberg(&[0.9, 0.8, 0.7], 0.05);
        assert_eq!(r, vec![false, false, false]);
    }

    #[test]
    fn bh_mixed() {
        let r = benjamini_hochberg(&[0.01, 0.04, 0.03, 0.9], 0.05);
        // Sorted: 0.01, 0.03, 0.04, 0.9
        // Thresholds: 0.0125, 0.025, 0.0375, 0.05
        // 0.01 <= 0.0125 ok, 0.03 <= 0.025 no, 0.04 <= 0.0375 no, 0.9 <= 0.05 no.
        // max k satisfying at rank 0 → only first is rejected.
        assert_eq!(r[0], true);
    }
}
