use chrono::NaiveDateTime;

/// Convert integer to zero-padded 16-char hex string (span ID format).
pub fn pad16(n: usize) -> String {
    format!("{:016x}", n)
}

/// Parsed timestamp parts: epoch seconds + fractional nanoseconds.
pub struct TsParts {
    pub s: i64,
    pub ns: u64,
}

/// Parse ISO 8601 timestamp into epoch seconds + fractional nanos.
pub fn ts_parts(ts: &str) -> TsParts {
    if ts.is_empty() {
        return TsParts { s: 0, ns: 0 };
    }

    // Split at "." to separate seconds from fractional part
    let (datetime_str, frac_str) = match ts.split_once('.') {
        Some((dt, f)) => (dt, f.trim_end_matches('Z')),
        None => (ts.trim_end_matches('Z'), "0"),
    };

    let epoch = NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%dT%H:%M:%S")
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0);

    // Pad or truncate fractional part to 9 digits
    let padded = format!("{:0<9}", frac_str);
    let ns: u64 = padded[..9].parse().unwrap_or(0);

    TsParts { s: epoch, ns }
}

/// Convert TsParts to nanosecond string.
pub fn parts_to_ns(p: &TsParts) -> String {
    format!("{}{:09}", p.s, p.ns)
}

/// Parse ISO 8601 timestamp to nanosecond string.
pub fn ts_to_ns(ts: &str) -> String {
    parts_to_ns(&ts_parts(ts))
}

/// Subtract milliseconds from a TsParts.
pub fn subtract_ms(p: &TsParts, ms: i64) -> TsParts {
    let s_off = ms / 1000;
    let ns_off = (ms % 1000) * 1_000_000;
    let new_ns = p.ns as i64 - ns_off;
    if new_ns >= 0 {
        TsParts {
            s: p.s - s_off,
            ns: new_ns as u64,
        }
    } else {
        TsParts {
            s: p.s - s_off - 1,
            ns: (new_ns + 1_000_000_000) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad16_formats_correctly() {
        assert_eq!(pad16(0), "0000000000000000");
        assert_eq!(pad16(1), "0000000000000001");
        assert_eq!(pad16(16), "0000000000000010");
        assert_eq!(pad16(30016), "0000000000007540");
    }

    #[test]
    fn ts_to_ns_parses_iso8601() {
        let ns = ts_to_ns("2026-03-05T10:30:00.123456789Z");
        assert_eq!(ns, "1772706600123456789");
    }

    #[test]
    fn ts_to_ns_handles_empty() {
        assert_eq!(ts_to_ns(""), "0000000000");
    }

    #[test]
    fn subtract_ms_works() {
        let p = TsParts {
            s: 100,
            ns: 500_000_000,
        };
        let r = subtract_ms(&p, 1500);
        assert_eq!(r.s, 99);
        assert_eq!(r.ns, 0);
    }

    #[test]
    fn subtract_ms_borrows() {
        let p = TsParts {
            s: 100,
            ns: 200_000_000,
        };
        let r = subtract_ms(&p, 500);
        assert_eq!(r.s, 99);
        assert_eq!(r.ns, 700_000_000);
    }
}
