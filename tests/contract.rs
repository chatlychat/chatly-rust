//! CONTRACT.md §10 items **1 to 6** — everything that needs a client.
//!
//! The remaining three (§10.7 webhook verification, §10.8 the identity-hash
//! vector, §10.9 the pinned delivery) are in `tests/contract_crypto.rs`:
//! they exercise free functions that take no client and no transport, and
//! keeping them here pushed this file past the contract's own 650-line
//! ceiling (§9).
//!
//! Offline by construction: every test drives an injected
//! [`common::MockTransport`]. No network, no API key, no running API.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use chatly::{ChatlyClient, ChatlyError, Query, DEFAULT_BASE_URL};
use common::{client, retrying_client, MockTransport};

// ---------------------------------------------------------------------------
// §10.1 — The default base URL is the production host.
// ---------------------------------------------------------------------------

#[test]
fn s1_the_default_base_url_is_the_production_host() {
    assert_eq!(DEFAULT_BASE_URL, "https://api.chatlychat.com");
    let chatly = ChatlyClient::new("ck_test_key").unwrap();
    assert_eq!(chatly.base_url(), "https://api.chatlychat.com");
}

/// The form that would have caught the real bug: assert the URL the client
/// *built*, from a client constructed without an explicit base URL.
#[test]
fn s1_a_request_from_a_default_client_goes_to_production() {
    let mock = MockTransport::new();
    mock.push_json(200, r#"{"workspaceId":"ws_1","scopes":[]}"#);
    client(&mock).whoami().unwrap();

    let request = mock.only_request();
    assert_eq!(
        request.url, "https://api.chatlychat.com/v1/api-keys/whoami",
        "a client with no base_url override must address production"
    );
    assert!(request.url.starts_with("https://"), "and over TLS");
}

// ---------------------------------------------------------------------------
// §10.2 — Authorization carries the key.
// ---------------------------------------------------------------------------

#[test]
fn s2_authorization_carries_the_key() {
    let mock = MockTransport::new();
    mock.push_json(200, "{}");
    client(&mock).contacts().get("c_1").unwrap();

    let request = mock.only_request();
    assert_eq!(request.header("authorization"), Some("Bearer ck_test_key"));
    assert_eq!(request.header("accept"), Some("application/json"));
    assert_eq!(
        request.header("user-agent"),
        Some(chatly::USER_AGENT),
        "CONTRACT.md §2: chatly-sdk-<lang>/<version>"
    );
    assert!(chatly::USER_AGENT.starts_with("chatly-sdk-rust/"));
}

// ---------------------------------------------------------------------------
// §10.3 — Writes send an Idempotency-Key; two writes differ; a pinned key
//          is sent twice.
// ---------------------------------------------------------------------------

#[test]
fn s3_a_write_sends_an_idempotency_key() {
    let mock = MockTransport::new();
    mock.push_json(201, "{}");
    client(&mock)
        .contacts()
        .upsert(&serde_json::json!({ "externalId": "u_1" }))
        .unwrap();

    let request = mock.only_request();
    assert_eq!(request.method, "POST");
    let key = request
        .header("idempotency-key")
        .expect("a write must carry an Idempotency-Key");
    assert_eq!(key.len(), 36, "a UUID, not an arbitrary token: {key}");
    assert_eq!(request.header("content-type"), Some("application/json"));
}

#[test]
fn s3_two_writes_send_different_keys() {
    let mock = MockTransport::new();
    mock.push_json(201, "{}").push_json(201, "{}");
    let chatly = client(&mock);
    let body = serde_json::json!({ "externalId": "u_1" });
    chatly.contacts().upsert(&body).unwrap();
    chatly.contacts().upsert(&body).unwrap();

    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    let first = requests[0].header("idempotency-key").unwrap();
    let second = requests[1].header("idempotency-key").unwrap();
    assert_ne!(
        first, second,
        "a fresh key per call is what stops an SDK retry double-writing"
    );
}

#[test]
fn s3_a_pinned_key_is_sent_twice() {
    let mock = MockTransport::new();
    mock.push_json(201, "{}").push_json(201, "{}");
    let pinned = client(&mock).with_idempotency_key("order-4711").unwrap();
    let body = serde_json::json!({ "externalId": "u_1" });
    pinned.contacts().upsert(&body).unwrap();
    pinned.contacts().upsert(&body).unwrap();

    let requests = mock.requests();
    assert_eq!(requests[0].header("idempotency-key"), Some("order-4711"));
    assert_eq!(requests[1].header("idempotency-key"), Some("order-4711"));
}

#[test]
fn s3_pinning_does_not_affect_the_parent_client() {
    let mock = MockTransport::new();
    mock.push_json(201, "{}").push_json(201, "{}");
    let chatly = client(&mock);
    let pinned = chatly.with_idempotency_key("order-4711").unwrap();
    let body = serde_json::json!({});
    pinned.contacts().upsert(&body).unwrap();
    chatly.contacts().upsert(&body).unwrap();

    let requests = mock.requests();
    assert_eq!(requests[0].header("idempotency-key"), Some("order-4711"));
    assert_ne!(requests[1].header("idempotency-key"), Some("order-4711"));
}

/// A DELETE is a write too, and has no body. The split that matters is
/// method, not body: `content-type` must be absent, `idempotency-key` present.
#[test]
fn s3_a_delete_carries_a_key_and_no_content_type() {
    let mock = MockTransport::new();
    mock.push_json(200, "{}");
    client(&mock).macros().delete("m_1").unwrap();

    let request = mock.only_request();
    assert_eq!(request.method, "DELETE");
    assert!(request.header("idempotency-key").is_some());
    assert_eq!(request.header("content-type"), None);
    assert_eq!(request.body, None);
}

#[test]
fn s3_a_read_carries_no_idempotency_key() {
    let mock = MockTransport::new();
    mock.push_json(200, r#"{"items":[],"nextCursor":null}"#);
    client(&mock).contacts().list(&Query::new()).unwrap();
    assert_eq!(mock.only_request().header("idempotency-key"), None);
}

// ---------------------------------------------------------------------------
// §10.4 — Status codes map to types, and each carries request_id.
// ---------------------------------------------------------------------------

#[test]
fn s4_404_raises_not_found_with_a_request_id() {
    let mock = MockTransport::new();
    mock.push_with_headers(
        404,
        r#"{"error":{"code":"not_found","message":"no such contact"}}"#,
        &[("x-request-id", "req_404")],
    );
    let err = client(&mock).contacts().get("c_missing").unwrap_err();

    assert!(matches!(err, ChatlyError::NotFound(_)), "{err:?}");
    assert!(err.is_not_found());
    assert_eq!(err.status(), Some(404));
    assert_eq!(err.code(), Some("not_found"));
    assert_eq!(err.message(), "no such contact");
    assert_eq!(err.request_id(), Some("req_404"));
    assert!(err.body().unwrap().contains("no such contact"));
}

/// CONTRACT.md §10.4 asks for 422; §3 adds that validation is really **400**
/// on this API and that both must map. Asserted both ways.
#[test]
fn s4_400_raises_validation_with_a_request_id() {
    let mock = MockTransport::new();
    mock.push_with_headers(
        400,
        r#"{"error":{"code":"validation_failed","message":"email is invalid"}}"#,
        &[("x-request-id", "req_400")],
    );
    let err = client(&mock)
        .contacts()
        .upsert(&serde_json::json!({ "email": "nope" }))
        .unwrap_err();

    assert!(err.is_validation(), "{err:?}");
    assert_eq!(err.status(), Some(400));
    assert_eq!(err.code(), Some("validation_failed"));
    assert_eq!(err.request_id(), Some("req_400"));
}

#[test]
fn s4_422_raises_validation_with_a_request_id() {
    let mock = MockTransport::new();
    mock.push_with_headers(
        422,
        r#"{"error":{"code":"validation_failed","message":"email is invalid"}}"#,
        &[("x-request-id", "req_422")],
    );
    let err = client(&mock)
        .contacts()
        .upsert(&serde_json::json!({ "email": "nope" }))
        .unwrap_err();

    assert!(err.is_validation(), "{err:?}");
    assert_eq!(err.status(), Some(422));
    assert_eq!(err.request_id(), Some("req_422"));
}

#[test]
fn s4_429_raises_rate_limited_with_retry_after_populated() {
    let mock = MockTransport::new();
    mock.push_with_headers(
        429,
        r#"{"error":{"code":"rate_limited","message":"slow down"}}"#,
        &[("retry-after", "7"), ("x-request-id", "req_429")],
    );
    let err = client(&mock).contacts().get("c_1").unwrap_err();

    assert!(err.is_rate_limited(), "{err:?}");
    assert_eq!(err.status(), Some(429));
    assert_eq!(
        err.retry_after(),
        Some(Duration::from_secs(7)),
        "Retry-After must reach the caller"
    );
    assert_eq!(err.request_id(), Some("req_429"));
    match err {
        ChatlyError::RateLimited { retry_after, error } => {
            assert_eq!(retry_after, Some(Duration::from_secs(7)));
            assert_eq!(error.code.as_deref(), Some("rate_limited"));
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn s4_401_and_403_are_distinguishable_without_parsing_a_message() {
    let mock = MockTransport::new();
    mock.push_with_headers(
        401,
        r#"{"error":{"code":"unauthenticated","message":"bad key"}}"#,
        &[("x-request-id", "req_401")],
    )
    .push_with_headers(
        403,
        r#"{"error":{"code":"forbidden","message":"missing scope"}}"#,
        &[("x-request-id", "req_403")],
    );
    let chatly = client(&mock);

    let unauthenticated = chatly.contacts().get("c_1").unwrap_err();
    assert!(unauthenticated.is_authentication());
    assert!(!unauthenticated.is_permission());
    assert_eq!(unauthenticated.request_id(), Some("req_401"));

    let forbidden = chatly.contacts().get("c_1").unwrap_err();
    assert!(forbidden.is_permission());
    assert!(!forbidden.is_authentication());
    assert_eq!(forbidden.request_id(), Some("req_403"));
}

#[test]
fn s4_409_raises_conflict_and_5xx_raises_server() {
    let mock = MockTransport::new();
    mock.push_json(
        409,
        r#"{"error":{"code":"conflict","message":"key reused"}}"#,
    )
    .push_json(500, r#"{"error":{"code":"unknown","message":"boom"}}"#);
    let chatly = client(&mock);

    assert!(chatly
        .contacts()
        .upsert(&serde_json::json!({}))
        .unwrap_err()
        .is_conflict());
    assert!(matches!(
        chatly
            .contacts()
            .upsert(&serde_json::json!({}))
            .unwrap_err(),
        ChatlyError::Server(_)
    ));
}

#[test]
fn s4_a_transport_failure_is_its_own_type_not_a_status() {
    let mock = MockTransport::new();
    mock.push_transport_error("dns failure");
    let err = client(&mock).contacts().get("c_1").unwrap_err();

    assert!(matches!(err, ChatlyError::Transport { .. }), "{err:?}");
    assert_eq!(err.status(), None, "a transport error has no HTTP status");
    assert!(err.message().contains("dns failure"));
}

// ---------------------------------------------------------------------------
// §10.5 — A 429 is retried then succeeds; a 400 is not retried.
// ---------------------------------------------------------------------------

#[test]
fn s5_a_429_is_retried_and_then_succeeds() {
    let mock = MockTransport::new();
    mock.push_with_headers(
        429,
        r#"{"error":{"code":"rate_limited"}}"#,
        &[("retry-after", "0")],
    )
    .push_json(200, r#"{"id":"c_1"}"#);

    let out = retrying_client(&mock, 3).contacts().get("c_1").unwrap();
    assert_eq!(out["id"], "c_1");
    assert_eq!(mock.request_count(), 2, "one retry, then success");
}

/// A 400 is a validation refusal here, and still must not be retried.
#[test]
fn s5_a_400_is_not_retried() {
    let mock = MockTransport::new();
    mock.push_json(
        400,
        r#"{"error":{"code":"validation_failed","message":"bad"}}"#,
    );

    let err = retrying_client(&mock, 3).contacts().get("c_1").unwrap_err();
    assert_eq!(err.status(), Some(400));
    assert!(err.is_validation());
    assert_eq!(
        mock.request_count(),
        1,
        "CONTRACT.md §4: never retry a 4xx that is not 429"
    );
}

#[test]
fn s5_no_other_4xx_is_retried_either() {
    for status in [401u16, 403, 404, 409, 422] {
        let mock = MockTransport::new();
        mock.push_json(status, "{}");
        let _ = retrying_client(&mock, 3).contacts().get("c_1");
        assert_eq!(
            mock.request_count(),
            1,
            "status {status} must not be retried"
        );
    }
}

#[test]
fn s5_a_5xx_and_a_connection_error_are_retried_up_to_the_limit() {
    let mock = MockTransport::new();
    mock.push_json(503, "{}")
        .push_json(503, "{}")
        .push_json(503, "{}");
    let err = retrying_client(&mock, 3).contacts().get("c_1").unwrap_err();
    assert_eq!(err.status(), Some(503));
    assert_eq!(mock.request_count(), 3, "three attempts, then give up");

    let mock = MockTransport::new();
    mock.push_transport_error("refused")
        .push_transport_error("refused")
        .push_json(200, "{}");
    retrying_client(&mock, 3).contacts().get("c_1").unwrap();
    assert_eq!(mock.request_count(), 3);
}

/// The whole reason the header exists (CONTRACT.md §4): a retried write must
/// reuse the key of the attempt that failed, or the retry is a second write.
#[test]
fn s5_a_retried_write_reuses_the_same_idempotency_key() {
    let mock = MockTransport::new();
    mock.push_json(503, "{}")
        .push_json(503, "{}")
        .push_json(201, "{}");

    retrying_client(&mock, 3)
        .contacts()
        .upsert(&serde_json::json!({ "externalId": "u_1" }))
        .unwrap();

    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    let keys: Vec<&str> = requests
        .iter()
        .map(|r| r.header("idempotency-key").expect("every attempt has one"))
        .collect();
    assert_eq!(keys[0], keys[1]);
    assert_eq!(keys[1], keys[2]);
}

#[test]
fn s5_retries_can_be_disabled() {
    let mock = MockTransport::new();
    mock.push_json(503, "{}");
    let err = client(&mock).contacts().get("c_1").unwrap_err();
    assert_eq!(err.status(), Some(503));
    assert_eq!(mock.request_count(), 1);
}

// ---------------------------------------------------------------------------
// §10.6 — The auto-paginator walks two pages and stops on a null cursor.
// ---------------------------------------------------------------------------

#[test]
fn s6_the_auto_paginator_walks_two_pages_and_stops_on_a_null_cursor() {
    let mock = MockTransport::new();
    mock.push_json(
        200,
        r#"{"items":[{"id":"a"},{"id":"b"}],"nextCursor":"cur_2"}"#,
    )
    .push_json(200, r#"{"items":[{"id":"c"}],"nextCursor":null}"#);

    let ids: Vec<String> = client(&mock)
        .contacts()
        .iter(&Query::new().limit(2))
        .map(|row| row.unwrap()["id"].as_str().unwrap_or_default().to_owned())
        .collect();

    assert_eq!(ids, vec!["a", "b", "c"]);
    let requests = mock.requests();
    assert_eq!(requests.len(), 2, "exactly two pages fetched");
    assert_eq!(
        requests[0].url,
        "https://api.chatlychat.com/v1/contacts?limit=2"
    );
    assert_eq!(
        requests[1].url, "https://api.chatlychat.com/v1/contacts?limit=2&cursor=cur_2",
        "the second page must carry the first page's cursor"
    );
}

#[test]
fn s6_the_auto_paginator_does_not_spin_on_a_repeated_cursor() {
    let mock = MockTransport::new();
    // A server bug: the same cursor, forever. Only two responses are queued,
    // so a paginator that kept going would panic in the mock rather than
    // hang the suite.
    mock.push_json(200, r#"{"items":[{"id":"a"}],"nextCursor":"stuck"}"#)
        .push_json(200, r#"{"items":[{"id":"b"}],"nextCursor":"stuck"}"#);

    let rows: Vec<_> = client(&mock).contacts().iter(&Query::new()).collect();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(Result::is_ok));
    assert_eq!(mock.request_count(), 2);
}

#[test]
fn s6_a_single_page_is_also_available_as_returned() {
    let mock = MockTransport::new();
    mock.push_json(200, r#"{"items":[{"id":"a"}],"nextCursor":"cur_2"}"#);

    let page = client(&mock).contacts().list(&Query::new()).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page.next_cursor.as_deref(), Some("cur_2"));
    assert!(!page.is_last());
    assert_eq!(mock.request_count(), 1, "one page means one request");
}

#[test]
fn s6_a_paginator_error_ends_the_iteration() {
    let mock = MockTransport::new();
    mock.push_json(200, r#"{"items":[{"id":"a"}],"nextCursor":"cur_2"}"#)
        .push_json(500, r#"{"error":{"code":"unknown","message":"boom"}}"#);

    let rows: Vec<_> = client(&mock).contacts().iter(&Query::new()).collect();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].is_ok());
    assert!(matches!(
        rows[1].as_ref().unwrap_err(),
        ChatlyError::Server(_)
    ));
    assert_eq!(mock.request_count(), 2, "no further pages after an error");
}

// ---------------------------------------------------------------------------
// A route-specific addendum to §10.4, because this surface has a 429 that
// does NOT look like the one above.
// ---------------------------------------------------------------------------

/// `visitors().start_conversation`'s own limit sends no `Retry-After`.
///
/// `s4_429_raises_rate_limited_with_retry_after_populated` asserts the shape
/// the throttler produces, and it is right about that. It is not the only
/// 429 on this surface, and believing it is would be the trap that
/// `wireError`'s own docstring was written about: a fixture that invents the
/// server's shape agrees with the bug rather than finding it.
///
/// What the API really does for the per-contact outreach limit, read off the
/// code rather than assumed:
///
/// - `consumePerContactOutreachToken` throws `RateLimitedError(ttl)`, which
///   is an `AppError` with `httpStatus: 429` and
///   `details: { retryAfterSeconds }`.
/// - `app-exception.filter.ts` sets no headers at all; it sends
///   `wireError(err.toJSON())` and nothing else.
/// - The only thing that synthesises a plain `Retry-After` is a Fastify
///   `onSend` hook that derives it from the `Retry-After-<bucket>` headers
///   `@nestjs/throttler` emits. This refusal does not come from the
///   throttler, so there are no bucket headers and no `Retry-After`.
///
/// So `retry_after()` is `None` here, and a caller that treats `None` as "no
/// delay required" hammers a limit that is closed for the rest of the hour.
/// The TTL is still recoverable — from the body, which this crate preserves
/// verbatim on `ApiError::body` precisely so a detail it does not model is
/// not lost.
///
/// `None` is this crate's answer specifically, and it is worth being precise
/// about why: the missing header belongs to the ROUTE, but whether a client
/// surfaces the wait regardless belongs to that client's error construction.
/// `parse_retry_after` here takes a header and nothing else, and neither
/// `error.rs` nor `client.rs` mentions `details` — so there is no body
/// fallback to find. The Ruby and Python clients do have one and populate
/// `retry_after` on this same refusal; Node, Go and PHP do not. That makes
/// CONTRACT.md §10.4's "a 429 … with `retry_after` populated" true of the
/// throttler's 429 in every language and language-dependent on this one, and
/// this test is the record of which side Rust is on. Driven through a real
/// envelope rather than asserted from the source, because reading the code
/// is how the wrong half of that gets believed.
#[test]
fn the_per_contact_outreach_limit_is_a_429_with_no_retry_after_header() {
    let mock = MockTransport::new();
    mock.push_with_headers(
        429,
        r#"{"error":{"code":"rate_limited","message":"Too many requests","details":{"retryAfterSeconds":2700}}}"#,
        &[("x-request-id", "req_outreach")],
    );

    let err = client(&mock)
        .visitors()
        .start_conversation("cont_1")
        .unwrap_err();

    assert!(err.is_rate_limited(), "{err:?}");
    assert_eq!(err.status(), Some(429));
    assert_eq!(err.request_id(), Some("req_outreach"));
    assert_eq!(
        err.retry_after(),
        None,
        "the per-contact limit sends no Retry-After, and this crate reads the \
         header only. Two things make this go red and they need opposite \
         fixes: a fixture that invented a header the API does not set (fix \
         the fixture), or this crate gaining the Ruby/Python \
         `details.retryAfterSeconds` fallback (fix this expectation AND the \
         rustdoc on `Visitors::start_conversation`, which tells callers to \
         parse the body themselves)"
    );

    // And the TTL the API did send is not lost: it is in the raw body.
    let ChatlyError::RateLimited { error, .. } = &err else {
        panic!("wrong variant: {err:?}");
    };
    let body: serde_json::Value = serde_json::from_str(&error.body).expect("the body is JSON");
    assert_eq!(body["error"]["details"]["retryAfterSeconds"], 2700);
}
