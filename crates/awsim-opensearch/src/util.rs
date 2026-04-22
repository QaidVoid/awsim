use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let time = secs % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;
    let mut y = 1970i64;
    let mut rem = days as i64;
    loop {
        let yd = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if rem < yd { break; }
        rem -= yd;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let md = [31, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 0;
    for &d in &md { if rem < d as i64 { break; } rem -= d as i64; mo += 1; }
    format!("{y:04}-{:02}-{:02}T{h:02}:{m:02}:{s:02}Z", mo+1, rem+1)
}
