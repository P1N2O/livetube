//! Output formatting matching the original JS libraries:
//! `@std/fmt/bytes` (pretty-bytes), `@std/fmt/duration` (full style),
//! hono's logger elapsed-time and JS `new Date().toString()`.

use chrono::Local;

/// pretty-bytes: 3 significant figures, decimal units. 1337 => "1.34 kB".
pub fn format_bytes(n: f64) -> String {
    const UNITS: [&str; 9] = ["B", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    if n < 1.0 || !n.is_finite() {
        return "0 B".to_string();
    }
    let exp = ((n.log10() / 3.0).floor() as usize).min(UNITS.len() - 1);
    let scaled = n / 1000f64.powi(exp as i32);
    let digits = if scaled >= 100.0 {
        0
    } else if scaled >= 10.0 {
        1
    } else {
        2
    };
    let rendered = format!("{scaled:.digits$}");
    let rendered = if digits > 0 {
        // JS Number(toPrecision(3)) drops trailing zeros and the decimal point.
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        rendered
    };
    format!("{rendered} {}", UNITS[exp])
}

/// `@std/fmt/duration` with style "full" and ignoreZero (days..milliseconds).
pub fn format_duration_full(ms: u64) -> String {
    let days = ms / 86_400_000;
    let hours = ms / 3_600_000 % 24;
    let minutes = ms / 60_000 % 60;
    let seconds = ms / 1_000 % 60;
    let millis = ms % 1_000;

    let mut parts: Vec<String> = Vec::with_capacity(5);
    for (value, unit) in [
        (days, "day"),
        (hours, "hour"),
        (minutes, "minute"),
        (seconds, "second"),
        (millis, "millisecond"),
    ] {
        if value > 0 {
            parts.push(format!(
                "{value} {unit}{}",
                if value == 1 { "" } else { "s" }
            ));
        }
    }
    parts.join(", ")
}

/// hono logger: `1234ms` => `1,234ms`; `>= 1s` => rounded seconds.
pub fn elapsed_str(millis: u64) -> String {
    if millis < 1000 {
        format!("{}ms", with_thousands(millis))
    } else {
        let secs = (millis as f64 / 1000.0).round() as u64;
        format!("{secs}s")
    }
}

/// Thousands separators, matching hono's humanize().
pub fn with_thousands(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Approximation of JS `new Date().toString()`: e.g.
/// "Tue Aug 04 2026 17:08:02 GMT+0530 (India Standard Time)".
pub fn js_date_string() -> String {
    let now = Local::now();
    let abbrev = now.format("%Z").to_string();
    let name = match abbrev.as_str() {
        "UTC" | "GMT" | "Z" => "Coordinated Universal Time".to_string(),
        "IST" => "India Standard Time".to_string(),
        "CET" => "Central European Time".to_string(),
        "CEST" => "Central European Summer Time".to_string(),
        "EST" => "Eastern Standard Time".to_string(),
        "EDT" => "Eastern Daylight Time".to_string(),
        "CST" => "Central Standard Time".to_string(),
        "CDT" => "Central Daylight Time".to_string(),
        "MST" => "Mountain Standard Time".to_string(),
        "MDT" => "Mountain Daylight Time".to_string(),
        "PST" => "Pacific Standard Time".to_string(),
        "PDT" => "Pacific Daylight Time".to_string(),
        "JST" => "Japan Standard Time".to_string(),
        "KST" => "Korea Standard Time".to_string(),
        "AEST" => "Australian Eastern Standard Time".to_string(),
        "AEDT" => "Australian Eastern Daylight Time".to_string(),
        // Some platforms return a numeric zone; map the unambiguous offsets.
        "+0000" | "-0000" => "Coordinated Universal Time".to_string(),
        "+0530" => "India Standard Time".to_string(),
        "+0900" => "Japan Standard Time".to_string(),
        "+0900JST" => "Japan Standard Time".to_string(),
        other if other.starts_with('+') || other.starts_with('-') => match other {
            "+05:30" => "India Standard Time".to_string(),
            "+09:00" => "Japan Standard Time".to_string(),
            "+00:00" => "Coordinated Universal Time".to_string(),
            _ => other.to_string(),
        },
        other => other.to_string(),
    };
    format!(
        "{} GMT{} ({})",
        now.format("%a %b %d %Y %H:%M:%S"),
        now.format("%z"),
        name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_formatting() {
        assert_eq!(format_bytes(0.0), "0 B");
        assert_eq!(format_bytes(100.0), "100 B");
        assert_eq!(format_bytes(999.0), "999 B");
        assert_eq!(format_bytes(1337.0), "1.34 kB");
        assert_eq!(format_bytes(1_860.0), "1.86 kB");
        assert_eq!(format_bytes(3_950_000.0), "3.95 MB");
        assert_eq!(format_bytes(5_820_000.0), "5.82 MB");
        assert_eq!(format_bytes(62_200_000.0), "62.2 MB");
        assert_eq!(format_bytes(100_000_000.0), "100 MB");
        assert_eq!(format_bytes(1_000_000.0), "1 MB");
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration_full(30 * 60_000), "30 minutes");
        assert_eq!(format_duration_full(60 * 60_000), "1 hour");
        assert_eq!(format_duration_full(90 * 60_000), "1 hour, 30 minutes");
        assert_eq!(
            format_duration_full(99_674),
            "1 minute, 39 seconds, 674 milliseconds"
        );
    }

    #[test]
    fn elapsed_formatting() {
        assert_eq!(with_thousands(1234), "1,234");
        assert_eq!(with_thousands(12), "12");
        assert_eq!(elapsed_str(35), "35ms");
        // JS: >= 1000ms is rounded to seconds (Math.round), so 1234ms => "1s".
        assert_eq!(elapsed_str(999), "999ms");
        assert_eq!(elapsed_str(1234), "1s");
        assert_eq!(elapsed_str(1500), "2s");
    }
}
