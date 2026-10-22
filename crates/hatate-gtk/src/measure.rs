//! Measurement helpers: RSS from `/proc/self/status`, frame-time statistics.

/// Resident set size and its peak, in KiB, from `/proc/self/status`.
pub fn rss_kib() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
            .unwrap_or(0)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

/// Summary of a list of durations in milliseconds.
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

pub fn summarize(samples: &[f64]) -> Summary {
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let at = |q: f64| {
        if v.is_empty() {
            0.0
        } else {
            v[(((v.len() - 1) as f64) * q).round() as usize]
        }
    };
    Summary {
        n: v.len(),
        p50: at(0.50),
        p95: at(0.95),
        p99: at(0.99),
        max: v.last().copied().unwrap_or(0.0),
    }
}

pub fn count_over(samples: &[f64], ms: f64) -> usize {
    samples.iter().filter(|s| **s > ms).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_picks_nearest_rank() {
        let s = summarize(&(1..=100).map(f64::from).collect::<Vec<_>>());
        assert_eq!((s.n, s.p50, s.max), (100, 51.0, 100.0));
        assert!(s.p95 >= 95.0 && s.p99 >= 99.0);
    }

    #[test]
    fn empty_summary_is_zero() {
        assert_eq!(summarize(&[]).max, 0.0);
    }

    #[test]
    fn counts_samples_over_a_budget() {
        assert_eq!(count_over(&[10.0, 17.0, 40.0], 16.7), 2);
    }

    #[test]
    fn rss_reads_this_process() {
        let (rss, hwm) = rss_kib();
        assert!(rss > 0 && hwm >= rss);
    }
}
