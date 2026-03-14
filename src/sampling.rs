//! Sampler and reducer — temporal observation primitives.
//!
//! A single `observe` call is a point-in-time snapshot. For ephemeral
//! quantities (CPU%, memory, network throughput), one sample is nearly
//! useless — you need temporal context.
//!
//! The **sampler** calls `observe` N times at a fixed interval.
//! The **reducer** collapses those samples into derived signals:
//!   - **mean** — average over the window (integral / duration)
//!   - **min / max** — extremes over the window
//!   - **delta** — last − first (net change)
//!   - **rate** — delta / duration (derivative)
//!
//! These are still observations — just higher-quality ones. Belief
//! state remains the observer's (agent's) concern, not ours.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─── Reducer types ──────────────────────────────────────────────────────────

/// Statistics computed over a series of numeric samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    pub first: f64,
    pub last: f64,
    /// last − first
    pub delta: f64,
    /// delta / duration_sec (only meaningful when duration > 0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_per_sec: Option<f64>,
    pub samples: usize,
}

/// The result of a sampling + reduction pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleResult {
    /// Sampling metadata.
    pub sampling: SamplingMeta,
    /// Reduced observation (same shape as original, but numeric leaves
    /// are replaced with Stats objects).
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingMeta {
    pub count: u32,
    pub interval_ms: u64,
    pub duration_ms: u64,
}

// ─── Reducer ────────────────────────────────────────────────────────────────

/// Reduce a series of observation `details` Values into a single Value
/// where numeric leaves become Stats objects.
///
/// `identity_keys` are field names used to group array elements across
/// samples (e.g. "pid" for processes, "id" for containers). If an array
/// element has one of these fields, elements with the same value are
/// grouped together and their other fields are reduced.
pub fn reduce(samples: &[Value], duration_sec: f64, identity_keys: &[&str]) -> Value {
    if samples.is_empty() {
        return Value::Null;
    }
    if samples.len() == 1 {
        return samples[0].clone();
    }

    reduce_value(samples, duration_sec, identity_keys)
}

fn reduce_value(samples: &[Value], duration_sec: f64, id_keys: &[&str]) -> Value {
    // Check what type the first sample is
    match &samples[0] {
        Value::Number(_) => {
            // All samples should be numbers — compute stats
            let nums: Vec<f64> = samples.iter().filter_map(as_f64).collect();
            if nums.is_empty() {
                return samples.last().cloned().unwrap_or(Value::Null);
            }
            // If all values are identical, return the scalar — no stats needed.
            // Reduces noise for fields like ppid, total_count, key_size that
            // are numeric but not measurements.
            if nums.iter().all(|&v| v == nums[0]) {
                return samples[0].clone();
            }
            serde_json::to_value(compute_stats(&nums, duration_sec)).unwrap_or(Value::Null)
        }

        Value::Object(_) => {
            // Collect all keys across all samples
            let mut all_keys = Vec::new();
            for s in samples {
                if let Value::Object(map) = s {
                    for k in map.keys() {
                        if !all_keys.contains(k) {
                            all_keys.push(k.clone());
                        }
                    }
                }
            }

            let mut result = serde_json::Map::new();
            for key in &all_keys {
                let field_samples: Vec<Value> = samples
                    .iter()
                    .filter_map(|s| s.get(key).cloned())
                    .collect();

                if field_samples.is_empty() {
                    continue;
                }

                // Identity keys (pid, id, name, ...) are kept as-is, not reduced
                if id_keys.contains(&key.as_str()) {
                    result.insert(
                        key.clone(),
                        field_samples.last().cloned().unwrap_or(Value::Null),
                    );
                } else if field_samples.iter().all(|v| v.is_number() || v.is_array() || v.is_object()) {
                    result.insert(key.clone(), reduce_value(&field_samples, duration_sec, id_keys));
                } else {
                    // Non-reducible (strings, bools, mixed) — take last
                    result.insert(
                        key.clone(),
                        field_samples.last().cloned().unwrap_or(Value::Null),
                    );
                }
            }

            Value::Object(result)
        }

        Value::Array(_) => {
            // Arrays of objects: try to group by identity key, then reduce each group.
            // Arrays of primitives: take last.
            let first_arr = samples[0].as_array().unwrap();
            if first_arr.is_empty() {
                return samples.last().cloned().unwrap_or(Value::Null);
            }

            // Check if elements are objects
            if !first_arr[0].is_object() {
                // Array of primitives — take last
                return samples.last().cloned().unwrap_or(Value::Null);
            }

            // Find which identity key is present
            let id_key = id_keys
                .iter()
                .find(|&&k| first_arr[0].get(k).is_some())
                .copied();

            match id_key {
                Some(key) => {
                    // Group elements across all samples by identity key value
                    let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();

                    for sample in samples {
                        if let Value::Array(arr) = sample {
                            for item in arr {
                                if let Some(id_val) = item.get(key) {
                                    let id_str = match id_val {
                                        Value::Number(n) => n.to_string(),
                                        Value::String(s) => s.clone(),
                                        other => other.to_string(),
                                    };
                                    groups.entry(id_str).or_default().push(item.clone());
                                }
                            }
                        }
                    }

                    // Reduce each group
                    let reduced: Vec<Value> = groups
                        .values()
                        .map(|group| reduce_value(group, duration_sec, id_keys))
                        .collect();

                    Value::Array(reduced)
                }
                None => {
                    // No identity key found — take last
                    samples.last().cloned().unwrap_or(Value::Null)
                }
            }
        }

        // Strings, bools, null — take last
        _ => samples.last().cloned().unwrap_or(Value::Null),
    }
}

fn compute_stats(nums: &[f64], duration_sec: f64) -> Stats {
    let n = nums.len();
    let first = nums[0];
    let last = nums[n - 1];
    let min = nums.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let sum: f64 = nums.iter().sum();
    let mean = sum / n as f64;
    let delta = last - first;
    let rate_per_sec = if duration_sec > 0.0 {
        Some(delta / duration_sec)
    } else {
        None
    };

    Stats {
        mean: round2(mean),
        min: round2(min),
        max: round2(max),
        first: round2(first),
        last: round2(last),
        delta: round2(delta),
        rate_per_sec: rate_per_sec.map(round2),
        samples: n,
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))
}

/// Well-known identity keys for grouping array elements across samples.
/// Order matters — first match wins.
pub const IDENTITY_KEYS: &[&str] = &["pid", "id", "name", "subject", "path", "port"];

// ─── Duration parsing ───────────────────────────────────────────────────────

/// Parse a duration string like "2s", "500ms", "1m" into milliseconds.
pub fn parse_duration_ms(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if let Some(rest) = s.strip_suffix("ms") {
        rest.trim().parse::<u64>().map_err(|e| e.to_string())
    } else if let Some(rest) = s.strip_suffix('s') {
        rest.trim()
            .parse::<f64>()
            .map(|v| (v * 1000.0) as u64)
            .map_err(|e| e.to_string())
    } else if let Some(rest) = s.strip_suffix('m') {
        rest.trim()
            .parse::<f64>()
            .map(|v| (v * 60_000.0) as u64)
            .map_err(|e| e.to_string())
    } else {
        // Default: treat bare number as seconds
        s.parse::<f64>()
            .map(|v| (v * 1000.0) as u64)
            .map_err(|_| format!("Invalid duration: '{s}'. Use e.g. 2s, 500ms, 1m"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_reduce_numbers() {
        let samples = vec![json!(10.0), json!(20.0), json!(30.0)];
        let result = reduce(&samples, 2.0, &[]);
        let stats: Stats = serde_json::from_value(result).unwrap();
        assert_eq!(stats.mean, 20.0);
        assert_eq!(stats.min, 10.0);
        assert_eq!(stats.max, 30.0);
        assert_eq!(stats.delta, 20.0);
        assert_eq!(stats.rate_per_sec, Some(10.0));
        assert_eq!(stats.samples, 3);
    }

    #[test]
    fn test_reduce_object_with_numeric_fields() {
        let samples = vec![
            json!({"cpu": 10.0, "name": "foo", "pid": 42}),
            json!({"cpu": 20.0, "name": "foo", "pid": 42}),
            json!({"cpu": 30.0, "name": "foo", "pid": 42}),
        ];
        let result = reduce(&samples, 2.0, &[]);
        assert_eq!(result["name"], "foo"); // non-numeric: last value
        assert_eq!(result["cpu"]["mean"], 20.0); // varying numeric: reduced
        assert_eq!(result["cpu"]["delta"], 20.0);
        assert_eq!(result["pid"], 42); // constant numeric: kept as scalar
    }

    #[test]
    fn test_reduce_array_grouped_by_pid() {
        let samples = vec![
            json!([
                {"pid": 1, "cpu": 10.0, "name": "init"},
                {"pid": 2, "cpu": 50.0, "name": "app"},
            ]),
            json!([
                {"pid": 1, "cpu": 12.0, "name": "init"},
                {"pid": 2, "cpu": 60.0, "name": "app"},
            ]),
            json!([
                {"pid": 1, "cpu": 14.0, "name": "init"},
                {"pid": 2, "cpu": 55.0, "name": "app"},
            ]),
        ];
        let result = reduce(&samples, 4.0, &["pid"]);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        // PID 1: cpu 10 → 12 → 14
        let p1 = &arr[0];
        assert_eq!(p1["pid"], 1);
        assert_eq!(p1["cpu"]["mean"], 12.0);
        assert_eq!(p1["cpu"]["delta"], 4.0);

        // PID 2: cpu 50 → 60 → 55
        let p2 = &arr[1];
        assert_eq!(p2["pid"], 2);
        assert_eq!(p2["cpu"]["mean"], 55.0);
        assert_eq!(p2["cpu"]["delta"], 5.0);
    }

    #[test]
    fn test_reduce_single_sample() {
        let samples = vec![json!({"cpu": 42.0})];
        let result = reduce(&samples, 0.0, &[]);
        // Single sample: returned as-is
        assert_eq!(result["cpu"], 42.0);
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration_ms("2s").unwrap(), 2000);
        assert_eq!(parse_duration_ms("500ms").unwrap(), 500);
        assert_eq!(parse_duration_ms("1m").unwrap(), 60000);
        assert_eq!(parse_duration_ms("0.5s").unwrap(), 500);
        assert_eq!(parse_duration_ms("3").unwrap(), 3000);
    }
}
