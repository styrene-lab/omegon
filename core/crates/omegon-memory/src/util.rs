//! Shared utilities for memory backends — ID generation, timestamps.

use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic counter to avoid ID collisions within the same process.
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique ID. Uses nanosecond timestamp + atomic counter + PID
/// to avoid collisions even under concurrent access.
pub fn gen_id() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let seq = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() as u64;
    // Mix time, sequence, and pid for uniqueness
    let hash = t.wrapping_mul(6364136223846793005).wrapping_add(seq ^ pid);
    format!("{:012x}", hash & 0xFFFF_FFFF_FFFF) // 12 hex chars
}

/// ISO 8601 UTC timestamp from the system clock.
pub fn now_iso() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    epoch_to_iso(d.as_secs(), d.subsec_millis())
}

fn epoch_to_iso(secs: u64, ms: u32) -> String {
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    let mut y = 1970i64;
    let mut rem = days as i64;
    loop {
        let yd = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if rem < yd { break; }
        rem -= yd;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let md: [i64; 12] = [31, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 0usize;
    for (i, &days_in_month) in md.iter().enumerate() {
        if rem < days_in_month { mo = i; break; }
        rem -= days_in_month;
    }

    format!("{y}-{:02}-{:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z", mo + 1, rem + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_id_is_12_hex_chars() {
        let id = gen_id();
        assert_eq!(id.len(), 12, "id={id}");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "not hex: {id}");
    }

    #[test]
    fn gen_id_no_collisions_sequential() {
        let ids: Vec<String> = (0..1000).map(|_| gen_id()).collect();
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), 1000, "expected 1000 unique IDs, got {}", unique.len());
    }

    #[test]
    fn now_iso_format() {
        let ts = now_iso();
        assert!(ts.ends_with('Z'), "should end with Z: {ts}");
        assert!(ts.contains('T'), "should contain T: {ts}");
        assert_eq!(ts.len(), 24, "YYYY-MM-DDTHH:MM:SS.mmmZ = 24 chars: {ts}");
    }
}
