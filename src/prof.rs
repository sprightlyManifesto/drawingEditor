//! Optional timing and call counts (STEPDRAW_PROFILE=1): where a render spends its time.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static ON: OnceLock<bool> = OnceLock::new();
static T: Mutex<BTreeMap<String, (Duration, u64)>> = Mutex::new(BTreeMap::new());

pub fn on() -> bool {
    *ON.get_or_init(|| std::env::var("STEPDRAW_PROFILE").is_ok())
}

/// Run `f`, adding its time and one call to `label` (times add up over threads).
pub fn time<R>(label: &str, f: impl FnOnce() -> R) -> R {
    if !on() {
        return f();
    }
    let t0 = Instant::now();
    let r = f();
    let dt = t0.elapsed();
    if let Ok(mut t) = T.lock() {
        let e = t.entry(label.to_string()).or_default();
        e.0 += dt;
        e.1 += 1;
    }
    r
}

/// Count one event of `label`.
pub fn count(label: &str) {
    if on()
        && let Ok(mut t) = T.lock()
    {
        t.entry(label.to_string()).or_default().1 += 1;
    }
}

/// Print the table (and clear it).
pub fn report() {
    if !on() {
        return;
    }
    if let Ok(mut t) = T.lock() {
        eprintln!("{:<44} {:>10} {:>12}", "profile (ms summed over threads)", "ms", "calls");
        for (k, (d, n)) in t.iter() {
            eprintln!("{:<44} {:>10.1} {:>12}", k, d.as_secs_f64() * 1000.0, n);
        }
        t.clear();
    }
}
