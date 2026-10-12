//! CONTRACT.md §10 items **7, 8 and 9** — the two helpers that take no
//! client, and the two pinned cross-language vectors.
//!
//! Split out of `tests/contract.rs`, which covers §10.1–§10.6. Nothing here
//! touches a transport: `verify_webhook_signature` and `identity_hash` are
//! free functions, which is the whole point of them — a webhook handler
//! should not need a configured client to tell a genuine delivery from a
//! forged one.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chatly::{
    identity_hash, verify_webhook_signature, verify_webhook_signature_with, VerifyOptions,
};

// ---------------------------------------------------------------------------
// §10.7 — Webhook verification accepts a genuine signature and rejects the
//          four failure shapes, without panicking on any of them.
// ---------------------------------------------------------------------------

const WEBHOOK_SECRET: &str = "whsec_test";

/// Build a genuine header for `body` at `t`, the way the platform does.
fn sign(body: &str, t: u64, secret: &str) -> String {
    // Independent of the crate's own HMAC: computed here from the public
    // identity_hash, which is the same primitive keyed the same way. If both
    // this helper and the verifier were wrong in the same direction the
    // pinned vectors in s9 would still catch it.
    let digest = identity_hash(secret, &format!("{t}.{body}")).unwrap();
    format!("t={t},v1={digest}")
}

#[test]
fn s7_accepts_a_genuine_signature_built_over_t_dot_body() {
    let body = r#"{"id":"evt_9","type":"conversation.created"}"#;
    let t = 1_900_000_000u64;
    let header = sign(body, t, WEBHOOK_SECRET);

    assert!(verify_webhook_signature_with(
        body,
        &header,
        WEBHOOK_SECRET,
        &VerifyOptions::new().now_seconds(t),
    ));
}

#[test]
fn s7_rejects_a_digest_over_the_body_alone() {
    let body = r#"{"id":"evt_9"}"#;
    let t = 1_900_000_000u64;
    // The Node bug: HMAC the body with no `<t>.` prefix.
    let body_only = identity_hash(WEBHOOK_SECRET, body).unwrap();
    let header = format!("t={t},v1={body_only}");

    assert!(!verify_webhook_signature_with(
        body,
        &header,
        WEBHOOK_SECRET,
        &VerifyOptions::new().now_seconds(t),
    ));
}

#[test]
fn s7_rejects_a_stale_timestamp() {
    let body = r#"{"id":"evt_9"}"#;
    let t = 1_900_000_000u64;
    let header = sign(body, t, WEBHOOK_SECRET);

    assert!(!verify_webhook_signature_with(
        body,
        &header,
        WEBHOOK_SECRET,
        &VerifyOptions::new().now_seconds(t + 301),
    ));
    assert!(verify_webhook_signature_with(
        body,
        &header,
        WEBHOOK_SECRET,
        &VerifyOptions::new().now_seconds(t + 299),
    ));
}

#[test]
fn s7_rejects_a_malformed_header_and_never_panics() {
    let body = r#"{"id":"evt_9"}"#;
    for header in [
        "",
        "garbage",
        "=",
        ",,,",
        "t=1900000000",
        "v1=deadbeef",
        "t=abc,v1=deadbeef",
        "t=1900000000,v1=nothex!!",
        "t=1900000000,v1=odd",
        "t=-1,v1=deadbeef",
    ] {
        // The assertion is twofold: false, and no unwind. A verifier that
        // panicked would be an accepted webhook in any handler without a
        // catch_unwind, which is all of them.
        let verdict = std::panic::catch_unwind(|| {
            verify_webhook_signature_with(
                body,
                header,
                WEBHOOK_SECRET,
                &VerifyOptions::new().now_seconds(1_900_000_000),
            )
        });
        assert_eq!(verdict.ok(), Some(false), "header {header:?}");
    }
}

#[test]
fn s7_the_default_tolerance_is_300_seconds() {
    assert_eq!(chatly::WEBHOOK_SIGNATURE_TOLERANCE_SECONDS, 300);
    assert_eq!(chatly::WEBHOOK_SIGNATURE_HEADER, "x-livechat-signature");
}

// ---------------------------------------------------------------------------
// §10.8 — identity_hash matches the pinned cross-language vector.
// ---------------------------------------------------------------------------

#[test]
fn s8_identity_hash_matches_the_pinned_vector() {
    assert_eq!(
        identity_hash("sek_test_secret", "user_42").unwrap(),
        "5e26772dce97d5ba995796e621f83f2e97e6f8c3f9386dbf153a98ab24c10271"
    );
}

#[test]
fn s8_identity_hash_rejects_empty_input() {
    assert!(identity_hash("", "user_42").is_err());
    assert!(identity_hash("sek_test_secret", "").is_err());
}

// ---------------------------------------------------------------------------
// §10.9 — The pinned webhook delivery, verbatim.
// ---------------------------------------------------------------------------

/// Secret `whsec_test`, body `{"id":"evt_1"}` (those 14 bytes, no trailing
/// newline), header as published in CONTRACT.md §10.9.
const PINNED_BODY: &[u8] = br#"{"id":"evt_1"}"#;
const PINNED_HEADER: &str =
    "t=1700000000,v1=c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925";

#[test]
fn s9_accepts_the_pinned_delivery_with_a_fixed_now() {
    assert_eq!(PINNED_BODY.len(), 14);
    assert!(verify_webhook_signature_with(
        PINNED_BODY,
        PINNED_HEADER,
        "whsec_test",
        &VerifyOptions::new().now_seconds(1_700_000_000),
    ));
}

#[test]
fn s9_accepts_the_pinned_delivery_with_an_unbounded_tolerance() {
    assert!(verify_webhook_signature_with(
        PINNED_BODY,
        PINNED_HEADER,
        "whsec_test",
        &VerifyOptions::new().without_tolerance(),
    ));
}

#[test]
fn s9_rejects_the_same_delivery_signed_over_the_body_alone() {
    let body_only =
        "t=1700000000,v1=030fa3b2413d1993c551364bd53bb9b3edb5c0c34d55dba6ada6041245632811";
    assert!(!verify_webhook_signature_with(
        PINNED_BODY,
        body_only,
        "whsec_test",
        &VerifyOptions::new().without_tolerance(),
    ));
    // And with the default (live) window, which rejects it for two reasons.
    assert!(!verify_webhook_signature(
        PINNED_BODY,
        body_only,
        "whsec_test"
    ));
}

/// Prints the digests this implementation produces, so they can be compared
/// against the other language SDKs. Run with
/// `cargo test -- --nocapture cross_language_digests`.
#[test]
fn cross_language_digests() {
    let identity = identity_hash("sek_test_secret", "user_42").unwrap();
    let webhook_t_dot_body = identity_hash("whsec_test", "1700000000.{\"id\":\"evt_1\"}").unwrap();
    let webhook_body_only = identity_hash("whsec_test", "{\"id\":\"evt_1\"}").unwrap();
    println!("identity_hash(\"sek_test_secret\", \"user_42\") = {identity}");
    println!("hmac(whsec_test, '1700000000.{{\"id\":\"evt_1\"}}') = {webhook_t_dot_body}");
    println!("hmac(whsec_test, '{{\"id\":\"evt_1\"}}')            = {webhook_body_only}");
    assert_eq!(
        identity,
        "5e26772dce97d5ba995796e621f83f2e97e6f8c3f9386dbf153a98ab24c10271"
    );
    assert_eq!(
        webhook_t_dot_body,
        "c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925"
    );
    assert_eq!(
        webhook_body_only,
        "030fa3b2413d1993c551364bd53bb9b3edb5c0c34d55dba6ada6041245632811"
    );
}
