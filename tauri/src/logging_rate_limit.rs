//! Per-target rate limiting for tracing events.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use tracing::Level;
use tracing_subscriber::filter::FilterFn;

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

static LIMITS: LazyLock<HashMap<String, f64>> = LazyLock::new(|| {
    let raw = std::env::var("LIVTET_LOG_RATE_LIMIT").unwrap_or_default();
    parse_rate_limits_from(&raw)
});
static STATE: LazyLock<Mutex<HashMap<String, Bucket>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn parse_rate_limits_from(raw: &str) -> HashMap<String, f64> {
    let mut limits = HashMap::new();
    for spec in raw.split(',') {
        let spec = spec.trim();
        if spec.is_empty() {
            continue;
        }
        let Some((target, rate_str)) = spec.split_once('=') else {
            continue;
        };
        if let Ok(rate) = rate_str
            .trim()
            .strip_suffix("/s")
            .unwrap_or(rate_str.trim())
            .parse::<f64>()
        {
            limits.insert(target.trim().to_string(), rate);
        }
    }
    limits
}

fn rate_limit_enabled(metadata: &tracing::Metadata<'_>) -> bool {
    let Some(&rate) = LIMITS.get(metadata.target()) else {
        return true;
    };

    let mut state = STATE.lock().unwrap();
    let bucket = state
        .entry(metadata.target().to_string())
        .or_insert_with(|| Bucket {
            tokens: rate,
            last: Instant::now(),
        });

    let now = Instant::now();
    let elapsed = now.duration_since(bucket.last).as_secs_f64();
    bucket.last = now;
    bucket.tokens = (bucket.tokens + elapsed * rate).min(rate);
    if bucket.tokens >= 1.0 {
        bucket.tokens -= 1.0;
        true
    } else {
        *metadata.level() >= Level::WARN
    }
}

fn always_true(_metadata: &tracing::Metadata<'_>) -> bool {
    true
}

pub fn from_env() -> FilterFn {
    if LIMITS.is_empty() {
        return FilterFn::new(always_true);
    }
    FilterFn::new(rate_limit_enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_string() {
        assert!(parse_rate_limits_from("").is_empty());
    }

    #[test]
    fn parse_valid_spec() {
        let limits = parse_rate_limits_from("target_a=10/s,target_b=5/s");
        assert_eq!(limits.get("target_a"), Some(&10.0));
        assert_eq!(limits.get("target_b"), Some(&5.0));
    }

    #[test]
    fn strips_s_suffix() {
        let limits = parse_rate_limits_from("tgt=1/s");
        assert_eq!(limits.get("tgt"), Some(&1.0));
    }

    #[test]
    fn tolerates_no_suffix() {
        let limits = parse_rate_limits_from("tgt=2");
        assert_eq!(limits.get("tgt"), Some(&2.0));
    }

    #[test]
    fn skips_malformed_entries() {
        let limits = parse_rate_limits_from("a=10/s,b=broken,c=5/s");
        assert_eq!(limits.len(), 2);
        assert!(limits.contains_key("a"));
        assert!(limits.contains_key("c"));
    }

    #[test]
    fn trims_whitespace() {
        let limits = parse_rate_limits_from("  a = 10/s , b = 5 ");
        assert_eq!(limits.get("a"), Some(&10.0));
        assert_eq!(limits.get("b"), Some(&5.0));
    }

    #[test]
    fn bucket_refills_tokens_over_time() {
        let rate = 10.0;
        let mut bucket = Bucket {
            tokens: 0.0,
            last: Instant::now(),
        };

        // Simulate 1 second passing
        bucket.last = Instant::now() - std::time::Duration::from_secs_f64(1.0);
        let elapsed = Instant::now().duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * rate).min(rate);
        assert!(bucket.tokens >= 9.9); // ~10 tokens after 1 second
    }

    #[test]
    fn missing_equal_sign_is_skipped() {
        let limits = parse_rate_limits_from("no_equals");
        assert!(limits.is_empty());
    }

    #[test]
    fn bucket_starts_with_rate_tokens() {
        let bucket = Bucket {
            tokens: 5.0,
            last: Instant::now(),
        };
        assert_eq!(bucket.tokens, 5.0);
    }
}
