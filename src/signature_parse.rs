//! The low-level, attacker-facing parsing behind
//! [`crate::verify_chatly_signature`]: header lookup, the `t=…,v1=…` value,
//! digest shape, and JavaScript-exact trimming.
//!
//! Every function here takes untrusted text and none of them can panic on
//! it: no byte indexing into a `&str`, no unwrap, and previews cut on `char`
//! boundaries. Split from `signatures.rs` for the 650-line ceiling.

use std::time::{SystemTime, UNIX_EPOCH};

/// Every value of `name` across every pair whose name matches it
/// case-insensitively. Full Unicode lowercasing, as the reference's
/// `toLowerCase()`; for the ASCII names Chatly sends that is plain ASCII.
pub(crate) fn header_values<'h>(headers: &[(&'h str, &'h str)], name: &str) -> Vec<&'h str> {
    let wanted = name.to_lowercase();
    headers
        .iter()
        .filter(|(k, _)| k.to_lowercase() == wanted)
        .map(|(_, v)| *v)
        .collect()
}

pub(crate) struct Tv1<'s> {
    pub(crate) t: &'s str,
    pub(crate) v1: Vec<&'s str>,
    pub(crate) b: Option<&'s str>,
}

/// Parse `t=…,v1=…[,b=…]`, collecting EVERY `v1` (a sender mid-rotation
/// signs with both secrets) and keeping the FIRST `t` and the FIRST `b`, so
/// a crafted second one cannot move the window or the base. Unknown fields
/// are ignored, so a future `v3=` cannot break a `v1` verifier.
pub(crate) fn parse_tv1(raw: &str) -> Option<Tv1<'_>> {
    let mut t = None;
    let mut b = None;
    let mut v1 = Vec::new();
    for part in raw.split(',') {
        // `eq <= 0` in the reference: no `=`, or a `=` in first position.
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        let value = js_trim(value);
        match js_trim(key) {
            "t" => t = t.or(Some(value)),
            "b" => b = b.or(Some(value)),
            "v1" => v1.push(value),
            _ => {}
        }
    }
    if v1.is_empty() {
        return None;
    }
    Some(Tv1 { t: t?, v1, b })
}

pub(crate) fn is_hex_sha256(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `String.prototype.trim`, exactly. Rust's `str::trim` uses Unicode
/// `White_Space`, which strips U+0085 and keeps U+FEFF — the opposite of
/// JavaScript on both. A header that verified in Node must verify here.
pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\u{0009}'..='\u{000D}'
                | '\u{0020}'
                | '\u{00A0}'
                | '\u{1680}'
                | '\u{2000}'..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
        )
    })
}

pub(crate) fn system_now_ms() -> i64 {
    // A clock before 1970 or past the year 292 million reads as 0 / i64::MAX
    // rather than panicking; either way the window check then refuses.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub(crate) fn capitalise(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Clip an untrusted value so an error message stays one line. By `char`,
/// never by byte, so a multi-byte character at the cut cannot panic.
pub(crate) fn preview(value: &str) -> String {
    if value.chars().count() > 48 {
        let mut out: String = value.chars().take(47).collect();
        out.push('…');
        out
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn js_trim_matches_javascript_not_rust() {
        assert_eq!(js_trim("\u{FEFF} a \u{3000}"), "a");
        // U+0085 is Rust whitespace and NOT JavaScript whitespace.
        assert_eq!(js_trim("\u{0085}a"), "\u{0085}a");
    }

    #[test]
    fn preview_cuts_on_chars() {
        let long = "é".repeat(100);
        assert_eq!(preview(&long).chars().count(), 48);
    }

    #[test]
    fn parse_tv1_skips_a_leading_equals_and_keeps_firsts() {
        let p = parse_tv1("=x, t=1 ,t=2,b=t,b=b,v1=a,v1=b").unwrap();
        assert_eq!((p.t, p.b, p.v1), ("1", Some("t"), vec!["a", "b"]));
        assert!(parse_tv1("t=1").is_none());
        assert!(parse_tv1("v1=a").is_none());
    }
}
