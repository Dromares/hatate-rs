//! The few numpy reductions `core/image_diff.py` and `core/upscale_detect.py` use, in numpy's own
//! `float32` arithmetic, so a result is equal to numpy's bit for bit: the pairwise summation of
//! `add.reduce`, `mean`, `median`, and `percentile` with a Python-float `q`.

/// numpy's `pairwise_sum` for `float32` (`loops_utils.h.src`): blocks of up to 128 are summed in
/// eight interleaved accumulators, longer runs are split in two at a multiple of eight. This is
/// what `ndarray.sum()` of a contiguous `float32` array computes.
pub(crate) fn sum_f32(a: &[f32]) -> f32 {
    let n = a.len();
    if n < 8 {
        let mut res = -0.0f32;
        for &x in a {
            res += x;
        }
        return res;
    }
    if n <= 128 {
        let mut r = [a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]];
        let mut i = 8;
        while i < n - (n % 8) {
            for (j, acc) in r.iter_mut().enumerate() {
                *acc += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        return res;
    }
    let mut n2 = n / 2;
    n2 -= n2 % 8;
    sum_f32(&a[..n2]) + sum_f32(&a[n2..])
}

/// `float(arr.mean())` for a `float32` array: numpy divides the `float32` sum by the item count
/// in `float64` (the count is an `intp`) and stores the result as `float32`.
pub(crate) fn mean_f32(a: &[f32]) -> f64 {
    f64::from((f64::from(sum_f32(a)) / a.len() as f64) as f32)
}

/// `float(np.median(a))` for a non-empty `float32` array (no NaNs): the middle value, or the
/// `float32` mean of the two middle ones. Reorders `a`.
pub(crate) fn median_f32(a: &mut [f32]) -> f64 {
    let n = a.len();
    let cmp = |x: &f32, y: &f32| x.total_cmp(y);
    if n % 2 == 1 {
        let (_, mid, _) = a.select_nth_unstable_by(n / 2, cmp);
        return f64::from(*mid);
    }
    let (lower, hi, _) = a.select_nth_unstable_by(n / 2, cmp);
    let lo = lower.iter().copied().max_by(cmp).expect("n >= 2");
    let sum = lo + *hi;
    f64::from((f64::from(sum) / 2.0) as f32)
}

/// `float(np.percentile(a, q))` for a non-empty `float32` array (no NaNs) with a Python-float `q`
/// (percent), method `linear`. numpy interpolates in `float32` with the weight as a weak Python
/// float. Reorders `a`.
pub(crate) fn percentile_f32(a: &mut [f32], q: f64) -> f64 {
    let n = a.len();
    let quantile = q / 100.0;
    let virtual_index = (n - 1) as f64 * quantile;
    let mut previous = virtual_index.floor() as i64;
    let mut next = previous + 1;
    if virtual_index >= (n - 1) as f64 {
        previous = -1;
        next = -1;
    }
    let gamma = virtual_index - previous as f64;
    let at = |i: i64| if i < 0 { n - 1 } else { i as usize };
    let cmp = |x: &f32, y: &f32| x.total_cmp(y);
    let (pi, ni) = (at(previous), at(next));
    a.select_nth_unstable_by(ni, cmp);
    let b = a[ni];
    let a_val = if pi == ni {
        b
    } else {
        // `pi` is the largest value below position `ni`.
        a[..ni].iter().copied().max_by(cmp).expect("ni >= 1")
    };
    let diff = b - a_val;
    let mut out = a_val + diff * (gamma as f32);
    if gamma >= 0.5 {
        out = b - diff * ((1.0 - gamma) as f32);
    }
    f64::from(out)
}

/// Python's `round()` on a float with no `ndigits`: half to even.
pub(crate) fn py_round(x: f64) -> i64 {
    x.round_ties_even() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_short_and_long_agree_with_a_plain_fold_on_exact_values() {
        for n in [0usize, 1, 7, 8, 9, 127, 128, 129, 1000, 5000] {
            let a: Vec<f32> = (0..n).map(|i| (i % 13) as f32).collect();
            let want: f32 = (0..n).map(|i| (i % 13) as f32).sum();
            assert_eq!(sum_f32(&a), want, "n = {n}");
        }
    }

    #[test]
    fn median_even_is_the_mean_of_the_two_middle_values() {
        let mut a = [4.0f32, 1.0, 3.0, 2.0];
        assert_eq!(median_f32(&mut a), 2.5);
        let mut b = [5.0f32, 1.0, 3.0];
        assert_eq!(median_f32(&mut b), 3.0);
    }

    #[test]
    fn percentile_interpolates_between_order_statistics() {
        let mut a: Vec<f32> = (0..=10).map(|i| i as f32).collect();
        assert_eq!(percentile_f32(&mut a, 70.0), 7.0);
        let mut one = [9.0f32];
        assert_eq!(percentile_f32(&mut one, 70.0), 9.0);
        let mut two = [1.0f32, 2.0];
        assert!((percentile_f32(&mut two, 70.0) - 1.7).abs() < 1e-6);
    }

    #[test]
    fn round_is_half_to_even() {
        assert_eq!(py_round(2.5), 2);
        assert_eq!(py_round(3.5), 4);
        assert_eq!(py_round(-0.5), 0);
        assert_eq!(py_round(2.4999), 2);
    }
}
