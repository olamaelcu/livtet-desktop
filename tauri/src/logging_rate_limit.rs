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

static LIMITS: LazyLock<HashMap<String, f64>> = LazyLock::new(|| parse_rate_limits());
static STATE: LazyLock<Mutex<HashMap<String, Bucket>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn parse_rate_limits() -> HashMap<String, f64> {
    let raw = std::env::var("LIVTET_LOG_RATE_LIMIT").unwrap_or_default();
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
