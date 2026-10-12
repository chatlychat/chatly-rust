//! Verifying the signature on an inbound webhook delivery.
//!
//! Chatly signs each delivery with the endpoint's `whsec_…` secret and sends
//! the result in `x-livechat-signature`:
//!
//! ```text
//! x-livechat-signature: t=1700000000,v1=c89214b5…d632925
//! ```
//!
//! `v1` is HMAC-SHA256 over the bytes `` `<t>.<raw body>` ``. The timestamp
//! is bound **into** the digest, which is what makes the replay window
//! enforceable: an attacker who replays a captured delivery cannot move `t`
//! forward without invalidating `v1`.
//!
//! An earlier Node implementation HMAC'd the body alone and therefore
//! rejected 100% of genuine deliveries. [`verify_webhook_signature`] is
//! pinned against both the correct construction and that exact mistake —
//! see this module's tests and CONTRACT.md §10.9.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::crypto;

/// The default freshness window for `t`, in seconds.
///
/// A delivery whose timestamp is more than this far from now — in either
/// direction, since a clock can be behind as well as ahead — is treated as a
/// replay.
pub const WEBHOOK_SIGNATURE_TOLERANCE_SECONDS: u64 = 300;

/// The header Chatly puts the signature in.
pub const WEBHOOK_SIGNATURE_HEADER: &str = "x-livechat-signature";

/// Knobs for [`verify_webhook_signature_with`].
///
/// `Default` is the production setting: a 300-second window against the
/// system clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyOptions {
    /// The freshness window in seconds.
    ///
    /// `None` disables the window check entirely. Use that **only** when
    /// re-verifying an archived delivery, for instance from
    /// `GET /v1/webhooks/deliveries/{id}`: a live endpoint with no window
    /// will accept a delivery captured from the wire a year ago.
    pub tolerance_seconds: Option<u64>,
    /// Pin "now", in Unix seconds. `None` reads the system clock.
    ///
    /// Exists so a test can assert against a fixed timestamp — including the
    /// 2023 vector in CONTRACT.md §10.9 — without disabling the window.
    pub now_seconds: Option<u64>,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            tolerance_seconds: Some(WEBHOOK_SIGNATURE_TOLERANCE_SECONDS),
            now_seconds: None,
        }
    }
}

impl VerifyOptions {
    /// Default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the freshness window, in seconds.
    pub fn tolerance_seconds(mut self, seconds: u64) -> Self {
        self.tolerance_seconds = Some(seconds);
        self
    }

    /// Disable the freshness window. See [`VerifyOptions::tolerance_seconds`]
    /// for why you almost certainly do not want this on a live endpoint.
    pub fn without_tolerance(mut self) -> Self {
        self.tolerance_seconds = None;
        self
    }

    /// Pin "now" to a fixed Unix-seconds value.
    pub fn now_seconds(mut self, now: u64) -> Self {
        self.now_seconds = Some(now);
        self
    }
}

/// Verify a webhook delivery against its signing secret.
///
/// Pass the **raw bytes you received**. Re-serialising parsed JSON reorders
/// keys and changes whitespace, and the digest will not match — so in a web
/// framework you need the body-bytes extractor, not the JSON one.
///
/// Returns `false` and never panics for a malformed header, a missing `t` or
/// `v1`, a non-numeric `t`, a non-hex `v1`, a stale timestamp or a bad
/// digest. A verifier that panicked would be an *accepted* webhook in any
/// handler that did not catch the unwind.
///
/// Uses the default 300-second window; see
/// [`verify_webhook_signature_with`] to change it.
///
/// ```
/// use chatly::verify_webhook_signature_with;
/// use chatly::VerifyOptions;
///
/// let body = br#"{"id":"evt_1"}"#;
/// let header = "t=1700000000,v1=c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925";
/// assert!(verify_webhook_signature_with(
///     body,
///     header,
///     "whsec_test",
///     &VerifyOptions::new().now_seconds(1700000000),
/// ));
/// ```
pub fn verify_webhook_signature(
    body: impl AsRef<[u8]>,
    signature_header: &str,
    secret: &str,
) -> bool {
    verify_webhook_signature_with(body, signature_header, secret, &VerifyOptions::default())
}

/// [`verify_webhook_signature`] with an explicit window and clock.
pub fn verify_webhook_signature_with(
    body: impl AsRef<[u8]>,
    signature_header: &str,
    secret: &str,
    options: &VerifyOptions,
) -> bool {
    if secret.is_empty() {
        return false;
    }

    let Some((timestamp_text, tag_hex)) = parse_signature_header(signature_header) else {
        return false;
    };
    let Ok(issued_at) = timestamp_text.parse::<i64>() else {
        return false;
    };
    let Some(presented) = crypto::hex_decode(tag_hex) else {
        return false;
    };

    if let Some(tolerance) = options.tolerance_seconds {
        let now = match options.now_seconds {
            Some(now) => now,
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };
        // Both sides widened to i128 before subtracting: an i64 timestamp and
        // a u64 clock cannot be compared by casting one to the other without
        // a wrap, and a wrapped difference is a delivery accepted outside the
        // window.
        let skew = (i128::from(now) - i128::from(issued_at)).abs();
        if skew > i128::from(tolerance) {
            return false;
        }
    }

    // Signed over `<t>.<raw body>`, using the timestamp text EXACTLY as it
    // arrived. Re-rendering the parsed integer would change the digest for a
    // zero-padded or `+`-prefixed `t`, which is a rejection where the real
    // producer's bytes would have verified.
    let mut prefix = String::with_capacity(timestamp_text.len() + 1);
    prefix.push_str(timestamp_text);
    prefix.push('.');
    let expected = crypto::hmac_sha256(secret.as_bytes(), &[prefix.as_bytes(), body.as_ref()]);

    crypto::constant_time_eq(&presented, &expected)
}

/// Pull `t` and `v1` out of `t=…,v1=…`.
///
/// Tolerant of surrounding whitespace and of extra comma-separated fields —
/// a future `v2=` must not break a `v1` verifier — but requires both `t` and
/// `v1` to be present and non-empty.
fn parse_signature_header(header: &str) -> Option<(&str, &str)> {
    let mut timestamp = None;
    let mut tag = None;
    for field in header.split(',') {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key {
            "t" if timestamp.is_none() => timestamp = Some(value),
            "v1" if tag.is_none() => tag = Some(value),
            _ => {}
        }
    }
    Some((timestamp?, tag?))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// CONTRACT.md §10.9, verbatim. Also cross-checked with
    /// `printf '1700000000.{"id":"evt_1"}' | openssl dgst -sha256 -hmac whsec_test`.
    const PINNED_BODY: &[u8] = br#"{"id":"evt_1"}"#;
    const PINNED_SECRET: &str = "whsec_test";
    const PINNED_T: u64 = 1_700_000_000;
    const PINNED_HEADER: &str =
        "t=1700000000,v1=c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925";
    /// The same body HMAC'd WITHOUT the `<t>.` prefix — the exact bug that
    /// shipped in Node and rejected every genuine delivery.
    const BODY_ONLY_HEADER: &str =
        "t=1700000000,v1=030fa3b2413d1993c551364bd53bb9b3edb5c0c34d55dba6ada6041245632811";

    fn pinned() -> VerifyOptions {
        VerifyOptions::new().now_seconds(PINNED_T)
    }

    #[test]
    fn accepts_the_pinned_delivery() {
        assert_eq!(PINNED_BODY.len(), 14, "the vector is those 14 bytes");
        assert!(verify_webhook_signature_with(
            PINNED_BODY,
            PINNED_HEADER,
            PINNED_SECRET,
            &pinned()
        ));
    }

    #[test]
    fn accepts_the_pinned_delivery_with_the_window_disabled() {
        // The other way CONTRACT.md §10.9 allows admitting a 2023 timestamp.
        assert!(verify_webhook_signature_with(
            PINNED_BODY,
            PINNED_HEADER,
            PINNED_SECRET,
            &VerifyOptions::new().without_tolerance()
        ));
    }

    #[test]
    fn rejects_a_digest_over_the_body_alone() {
        assert!(!verify_webhook_signature_with(
            PINNED_BODY,
            BODY_ONLY_HEADER,
            PINNED_SECRET,
            &pinned()
        ));
        assert!(!verify_webhook_signature_with(
            PINNED_BODY,
            BODY_ONLY_HEADER,
            PINNED_SECRET,
            &VerifyOptions::new().without_tolerance()
        ));
    }

    #[test]
    fn rejects_a_stale_timestamp() {
        // Genuine digest, clock moved 301s past the 300s window.
        assert!(!verify_webhook_signature_with(
            PINNED_BODY,
            PINNED_HEADER,
            PINNED_SECRET,
            &VerifyOptions::new().now_seconds(PINNED_T + 301)
        ));
        // And 301s before it: a delivery from the future is a replay too.
        assert!(!verify_webhook_signature_with(
            PINNED_BODY,
            PINNED_HEADER,
            PINNED_SECRET,
            &VerifyOptions::new().now_seconds(PINNED_T - 301)
        ));
        // The boundary itself is inside the window.
        assert!(verify_webhook_signature_with(
            PINNED_BODY,
            PINNED_HEADER,
            PINNED_SECRET,
            &VerifyOptions::new().now_seconds(PINNED_T + 300)
        ));
    }

    #[test]
    fn the_real_clock_rejects_the_2023_vector() {
        // The default window must not admit it; if this ever passes, either
        // the window stopped being enforced or the machine clock is wrong.
        assert!(!verify_webhook_signature(
            PINNED_BODY,
            PINNED_HEADER,
            PINNED_SECRET
        ));
    }

    #[test]
    fn rejects_malformed_headers_without_panicking() {
        for header in [
            "",
            "garbage",
            "t=1700000000",
            "v1=c89214b5",
            "t=,v1=c89214b5",
            "t=1700000000,v1=",
            "t=not-a-number,v1=c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925",
            "t=1700000000,v1=zzzz",
            "t=1700000000,v1=c89",
            "t=1700000000;v1=c89214b5",
            "t=99999999999999999999999999,v1=c89214b5",
        ] {
            assert!(
                !verify_webhook_signature_with(PINNED_BODY, header, PINNED_SECRET, &pinned()),
                "{header:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_a_truncated_digest_rather_than_comparing_a_prefix() {
        let short = "t=1700000000,v1=c89214b5b5da833daed6f0b8c5bb6bd5";
        assert!(!verify_webhook_signature_with(
            PINNED_BODY,
            short,
            PINNED_SECRET,
            &pinned()
        ));
    }

    #[test]
    fn rejects_a_wrong_secret_and_an_empty_secret() {
        assert!(!verify_webhook_signature_with(
            PINNED_BODY,
            PINNED_HEADER,
            "whsec_other",
            &pinned()
        ));
        assert!(!verify_webhook_signature_with(
            PINNED_BODY,
            PINNED_HEADER,
            "",
            &pinned()
        ));
    }

    #[test]
    fn a_reserialised_body_does_not_verify() {
        // Why the API takes bytes and not a decoded object: same JSON,
        // different whitespace.
        assert!(!verify_webhook_signature_with(
            br#"{ "id": "evt_1" }"#,
            PINNED_HEADER,
            PINNED_SECRET,
            &pinned()
        ));
        // Same bytes with a trailing newline, as a logger might add.
        assert!(!verify_webhook_signature_with(
            b"{\"id\":\"evt_1\"}\n",
            PINNED_HEADER,
            PINNED_SECRET,
            &pinned()
        ));
    }

    // The allocations clippy wants removed are the point: this asserts the
    // `impl AsRef<[u8]>` parameter accepts an owned `String` and `Vec<u8>`
    // as well as a borrow, so a handler that owns its body does not have to
    // re-borrow it.
    #[allow(clippy::unnecessary_to_owned)]
    #[test]
    fn accepts_a_str_a_string_and_bytes_alike() {
        let as_str = r#"{"id":"evt_1"}"#;
        assert!(verify_webhook_signature_with(
            as_str,
            PINNED_HEADER,
            PINNED_SECRET,
            &pinned()
        ));
        assert!(verify_webhook_signature_with(
            as_str.to_owned(),
            PINNED_HEADER,
            PINNED_SECRET,
            &pinned()
        ));
        assert!(verify_webhook_signature_with(
            as_str.as_bytes().to_vec(),
            PINNED_HEADER,
            PINNED_SECRET,
            &pinned()
        ));
    }

    #[test]
    fn tolerates_extra_fields_and_whitespace() {
        let header = " t=1700000000 , v1=c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925 , v2=future ";
        assert!(verify_webhook_signature_with(
            PINNED_BODY,
            header,
            PINNED_SECRET,
            &pinned()
        ));
    }

    #[test]
    fn uppercase_hex_verifies() {
        let header = format!(
            "t=1700000000,v1={}",
            "c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925".to_uppercase()
        );
        assert!(verify_webhook_signature_with(
            PINNED_BODY,
            &header,
            PINNED_SECRET,
            &pinned()
        ));
    }

    #[test]
    fn an_empty_body_is_signed_as_the_prefix_alone() {
        // Not a special case in the code; asserted so it stays that way.
        let digest = crypto::hmac_sha256(b"whsec_test", &[b"1700000000."]);
        let header = format!("t=1700000000,v1={}", crypto::hex_encode(&digest));
        assert!(verify_webhook_signature_with(
            b"",
            &header,
            PINNED_SECRET,
            &pinned()
        ));
    }

    #[test]
    fn parse_header_requires_both_fields() {
        assert_eq!(parse_signature_header("t=1,v1=ab"), Some(("1", "ab")));
        assert_eq!(parse_signature_header("v1=ab"), None);
        assert_eq!(parse_signature_header("t=1"), None);
        // First occurrence wins, so an appended duplicate cannot override.
        assert_eq!(parse_signature_header("t=1,v1=ab,v1=cd"), Some(("1", "ab")));
    }
}
