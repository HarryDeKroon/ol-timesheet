use regex::Regex;
use std::sync::LazyLock;

/// Format hours as a short decimal string.
///
/// Rules:
/// - total >= 100  → rounded integer (e.g. "126")
/// - 10 <= total < 100 → one decimal (e.g. "32.6")
/// - total < 10   → two decimals (e.g. "2.50")
/// - total == 0.0 → empty string (blank cell)
///
/// The decimal separator is locale-dependent.
pub fn format_hours_short(hours: f64, decimal_sep: char) -> String {
    if hours == 0.0 {
        return String::new();
    }
    let abs = hours.abs();
    let formatted = if abs >= 100.0 {
        format!("{:.0}", hours)
    } else if abs >= 10.0 {
        format!("{:.1}", hours)
    } else {
        format!("{:.2}", hours)
    };
    if decimal_sep != '.' {
        formatted.replace('.', &decimal_sep.to_string())
    } else {
        formatted
    }
}

/// Format hours in long format: "1w 3d 7h 15m".
///
/// Calculated from `hours_per_day` and `hours_per_week` user preferences.
/// Zero components are omitted. Returns empty string for 0.0 hours.
pub fn format_hours_long(
    hours: f64,
    hours_per_day: f64,
    hours_per_week: f64,
    w_label: &str,
    d_label: &str,
    h_label: &str,
    m_label: &str,
) -> String {
    if hours <= 0.0 {
        return String::new();
    }

    let mut remaining = hours;
    let mut parts = Vec::new();

    if hours_per_week > 0.0 {
        let weeks = (remaining / hours_per_week).floor() as u32;
        if weeks > 0 {
            parts.push(format!("{}{}", weeks, w_label));
            remaining -= weeks as f64 * hours_per_week;
        }
    }

    if hours_per_day > 0.0 {
        let days = (remaining / hours_per_day).floor() as u32;
        if days > 0 {
            parts.push(format!("{}{}", days, d_label));
            remaining -= days as f64 * hours_per_day;
        }
    }

    let mut h = remaining.floor() as u32;
    let mut m = ((remaining - h as f64) * 60.0).round() as u32;

    // Carry over when rounding pushes minutes to 60.
    if m >= 60 {
        h += m / 60;
        m %= 60;
    }

    if h > 0 {
        parts.push(format!("{}{}", h, h_label));
    }
    if m > 0 {
        parts.push(format!("{}{}", m, m_label));
    }

    if parts.is_empty() {
        format!("0{}", m_label)
    } else {
        parts.join(" ")
    }
}

static LONG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:(\d+)\s*w)?\s*(?:(\d+)\s*d)?\s*(?:(\d+)\s*h)?\s*(?:(\d+)\s*m)?").unwrap()
});

/// Build a locale-aware regex for the long duration format.
///
/// The pattern matches `<digits><label>` pairs in the order
/// weeks / days / hours / minutes, where each label comes from i18n.
/// All groups are optional. The regex is case-insensitive.
fn build_long_re(w: &str, d: &str, h: &str, m: &str) -> Regex {
    Regex::new(&format!(
        r"(?i)(?:(\d+)\s*{w})?\s*(?:(\d+)\s*{d})?\s*(?:(\d+)\s*{h})?\s*(?:(\d+)\s*{m})?",
        w = regex::escape(w),
        d = regex::escape(d),
        h = regex::escape(h),
        m = regex::escape(m),
    ))
    .unwrap_or_else(|_| LONG_RE.clone())
}

/// Try to parse the long duration format against a given regex.
/// Returns `Some(hours)` when at least one group matched a non-zero value.
fn try_long_format(
    re: &Regex,
    input: &str,
    hours_per_day: f64,
    hours_per_week: f64,
) -> Option<f64> {
    let caps = re.captures(input)?;
    let weeks: f64 = caps
        .get(1)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    let days: f64 = caps
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    let hours: f64 = caps
        .get(3)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    let minutes: f64 = caps
        .get(4)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    let total = weeks * hours_per_week + days * hours_per_day + hours + minutes / 60.0;
    if total > 0.0 { Some(total) } else { None }
}

/// Parse a hours string in either short (decimal) or long ("1w 2d 3h 45m") format.
///
/// The long format uses the locale-specific unit labels `w_label`, `d_label`,
/// `h_label`, `m_label` (e.g. "w"/"d"/"h"/"m" for English, "w"/"d"/"u"/"m"
/// for Dutch). English labels are always accepted as a fallback.
///
/// Returns `None` if the string cannot be parsed.
/// The `decimal_sep` parameter is used to normalise the decimal separator for short format.
pub fn parse_hours(
    input: &str,
    hours_per_day: f64,
    hours_per_week: f64,
    decimal_sep: char,
    w_label: &str,
    d_label: &str,
    h_label: &str,
    m_label: &str,
) -> Option<f64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try short format (decimal number) first.
    // Replace locale decimal separator with '.' for parsing.
    let normalised = if decimal_sep != '.' {
        trimmed.replace(decimal_sep, ".")
    } else {
        trimmed.to_string()
    };
    if let Ok(v) = normalised.parse::<f64>() {
        if v >= 0.0 {
            return Some(v);
        }
    }

    // Try locale-specific long format first (e.g. "1u 5m" for Dutch).
    let locale_re = build_long_re(w_label, d_label, h_label, m_label);
    if let Some(total) = try_long_format(&locale_re, trimmed, hours_per_day, hours_per_week) {
        return Some(total);
    }

    // Fall back to English labels (w/d/h/m) so input like "1h 30m" always works.
    if let Some(total) = try_long_format(&LONG_RE, trimmed, hours_per_day, hours_per_week) {
        return Some(total);
    }

    None
}

/// Format a decimal number with the given number of decimal places and locale separator.
pub fn format_decimal(value: f64, decimals: usize, decimal_sep: char) -> String {
    let formatted = format!("{:.prec$}", value, prec = decimals);
    if decimal_sep != '.' {
        formatted.replace('.', &decimal_sep.to_string())
    } else {
        formatted
    }
}
