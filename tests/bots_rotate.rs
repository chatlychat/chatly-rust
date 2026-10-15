//! `bots().rotate_external_secret` and `rotate_external_secret_at` — what
//! each one PUTS ON THE WIRE.
//!
//! The route took no body until `activateAt` arrived, and its schema is
//! `.strict().default({})`: no body is the hard cut, `{"activateAt": …}` is
//! the overlap window, and any other key — `activate_at` included — is a 400.
//! So the two things worth proving are that the old call still sends
//! *nothing*, and that the new one sends exactly one camelCase key. A fake
//! that echoes the request would agree with either bug, which is why every
//! assertion here is on the recorded request rather than the canned answer.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chatly::HttpRequest;
use common::{client, MockTransport};
use serde_json::{json, Value};

const URL: &str = "https://api.chatlychat.com/v1/bots/b_1/external/rotate-secret";
const ANSWER: &str = r#"{"signingSecret":"whsec_x","activatesAt":null}"#;

fn sent(call: impl FnOnce(&chatly::ChatlyClient)) -> HttpRequest {
    let mock = MockTransport::new();
    mock.push_json(201, ANSWER);
    call(&client(&mock));
    mock.only_request()
}

#[test]
fn with_activate_at_the_body_is_exactly_one_camel_case_key() {
    let at = "2026-10-16T11:00:00+02:00";
    let request = sent(|c| {
        c.bots().rotate_external_secret_at("b_1", at).unwrap();
    });

    assert_eq!(request.method, "POST");
    assert_eq!(request.url, URL);
    assert!(
        request.header("idempotency-key").is_some(),
        "a rotation is a write and carries an Idempotency-Key"
    );
    assert_eq!(request.header("content-type"), Some("application/json"));

    let bytes = request.body.as_deref().expect("a body was sent");
    let body: Value = serde_json::from_slice(bytes).expect("the body is JSON");
    // Exact equality: no other key, and the value passed through verbatim
    // (the offset is NOT normalised to Z — the server validates it).
    assert_eq!(body, json!({ "activateAt": at }));
    let object = body.as_object().unwrap();
    assert_eq!(object.len(), 1);
    assert!(
        !object.contains_key("activate_at"),
        "snake_case is a 400 on a strict schema"
    );
}

#[test]
fn without_activate_at_nothing_changed_on_the_wire() {
    let request = sent(|c| {
        c.bots().rotate_external_secret("b_1").unwrap();
    });

    assert_eq!(request.method, "POST");
    assert_eq!(request.url, URL);
    // NO body — not `{}`. Both mean the hard cut server-side, but this call
    // has never sent a body and must not start.
    assert_eq!(request.body, None, "the hard cut sends no body at all");
    // The full header set, so a stray content-type would show up too.
    let names: Vec<&str> = request.headers.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        names,
        ["authorization", "accept", "user-agent", "idempotency-key"]
    );
}

#[test]
fn the_caller_can_read_activates_at_as_the_server_sends_it() {
    let mock = MockTransport::new();
    mock.push_json(
        201,
        r#"{"signingSecret":"whsec_x","activatesAt":"2026-10-16T09:00:00.000Z"}"#,
    );
    let out = client(&mock)
        .bots()
        .rotate_external_secret_at("b_1", "2026-10-16T09:00:00.000Z")
        .unwrap();
    assert_eq!(out["signingSecret"], "whsec_x");
    assert_eq!(out["activatesAt"], "2026-10-16T09:00:00.000Z");

    let mock = MockTransport::new();
    mock.push_json(201, ANSWER);
    let out = client(&mock).bots().rotate_external_secret("b_1").unwrap();
    assert_eq!(out["signingSecret"], "whsec_x");
    assert!(
        out.get("activatesAt").is_some_and(Value::is_null),
        "null = already signing, and it is present rather than missing"
    );
}
