//! What each call *carries*: request bodies, query encoding and the escape
//! hatch.
//!
//! Split out of `tests/surface.rs`, which asserts the verb and path of all
//! 55 methods. The field names asserted here were read off the API's own zod
//! schemas rather than guessed — a wrong one is a write that validates
//! server-side and does nothing, which no status code reveals.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chatly::{ChatlyClient, Query};
use common::{client, BodyShape, MockTransport};
use serde_json::json;

const BASE: &str = "https://api.chatlychat.com";
const OBJ: BodyShape = BodyShape::Object;
const PAGE: BodyShape = BodyShape::Page;

// ---------------------------------------------------------------------------
// Bodies the convenience methods build, asserted against the API's verified
// zod field names. A field name wrong here is a write that validates and
// does nothing.
// ---------------------------------------------------------------------------

fn sent_body(call: impl FnOnce(&ChatlyClient)) -> serde_json::Value {
    let mock = MockTransport::new();
    mock.push_json(200, OBJ.body());
    call(&client(&mock));
    let text = mock.only_request().body_text().expect("a body was sent");
    serde_json::from_str(&text).expect("the body is JSON")
}

#[test]
fn contact_merge_sends_merged_id_and_ticket_merge_sends_loser_id() {
    // Two routes that do the same thing and spell the argument differently.
    assert_eq!(
        sent_body(|c| {
            let _ = c.contacts().merge("c_1", "c_2");
        }),
        json!({ "mergedId": "c_2" })
    );
    assert_eq!(
        sent_body(|c| {
            let _ = c.tickets().merge_loser("t_1", "t_2");
        }),
        json!({ "loserId": "t_2" })
    );
}

#[test]
fn add_tag_sends_a_single_tag_key() {
    assert_eq!(
        sent_body(|c| {
            let _ = c.contacts().add_tag("c_1", "vip");
        }),
        json!({ "tag": "vip" })
    );
}

#[test]
fn unsubscribe_sends_a_boolean_and_omits_an_absent_reason() {
    assert_eq!(
        sent_body(|c| {
            let _ = c.contacts().unsubscribe("c_1", true, None);
        }),
        json!({ "unsubscribed": true })
    );
    assert_eq!(
        sent_body(|c| {
            let _ = c
                .contacts()
                .unsubscribe("c_1", false, Some("asked to return"));
        }),
        json!({ "unsubscribed": false, "reason": "asked to return" })
    );
}

#[test]
fn schedule_sends_body_and_send_at() {
    assert_eq!(
        sent_body(|c| {
            let _ = c
                .conversations()
                .schedule("v_1", "later", "2026-10-13T09:00:00Z");
        }),
        json!({ "body": "later", "sendAt": "2026-10-13T09:00:00Z" })
    );
}

#[test]
fn set_agent_handling_state_sends_state_and_refuses_an_unsettable_one() {
    assert_eq!(
        sent_body(|c| {
            let _ = c
                .conversations()
                .set_agent_handling_state("v_1", "handed_off", Some("why"));
        }),
        json!({ "state": "handed_off", "reason": "why" })
    );

    // `taken_over` is set by a database trigger, never by a caller.
    let mock = MockTransport::new();
    let err = client(&mock)
        .conversations()
        .set_agent_handling_state("v_1", "taken_over", None)
        .unwrap_err();
    assert!(matches!(err, chatly::ChatlyError::Config(_)), "{err:?}");
    assert_eq!(
        mock.request_count(),
        0,
        "refused before the request, not after a 500"
    );
}

#[test]
fn add_tags_issues_one_request_per_tag_and_refuses_an_empty_list() {
    let mock = MockTransport::new();
    mock.push_json(200, OBJ.body())
        .push_json(200, OBJ.body())
        .push_json(200, OBJ.body());
    client(&mock)
        .contacts()
        .add_tags("c_1", &["a", "b", "c"])
        .unwrap();
    assert_eq!(mock.request_count(), 3, "the route takes one tag per call");
    let bodies: Vec<String> = mock
        .requests()
        .iter()
        .filter_map(chatly::HttpRequest::body_text)
        .collect();
    assert_eq!(
        bodies,
        vec![r#"{"tag":"a"}"#, r#"{"tag":"b"}"#, r#"{"tag":"c"}"#]
    );

    let empty = MockTransport::new();
    assert!(client(&empty).contacts().add_tags("c_1", &[]).is_err());
    assert_eq!(empty.request_count(), 0);
}

// ---------------------------------------------------------------------------
// Query encoding and the escape hatch.
// ---------------------------------------------------------------------------

/// `eventType` is a required argument, not an optional query parameter: the
/// handler validates it against a closed list and throws first, so a call
/// without it is a 400 and never an all-types rollup.
#[test]
fn billing_usage_sends_the_event_type_and_refuses_an_empty_one() {
    let mock = MockTransport::new();
    mock.push_json(200, OBJ.body());
    client(&mock)
        .billing()
        .usage_range("ai.tokens", "2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z")
        .unwrap();
    assert_eq!(
        mock.only_request().url,
        format!("{BASE}/v1/billing/usage?from=2026-10-01T00%3A00%3A00Z&to=2026-11-01T00%3A00%3A00Z&eventType=ai.tokens")
    );

    let empty = MockTransport::new();
    let err = client(&empty)
        .billing()
        .usage("  ", &Query::new())
        .unwrap_err();
    assert!(matches!(err, chatly::ChatlyError::Config(_)), "{err:?}");
    assert_eq!(empty.request_count(), 0, "refused before the request");

    // An unrecognised but non-empty type IS sent: the server's own error
    // lists what it accepts, and that list is more current than ours.
    let unknown = MockTransport::new();
    unknown.push_json(400, r#"{"error":{"code":"validation_failed"}}"#);
    let err = client(&unknown)
        .billing()
        .usage("ai.credits", &Query::new())
        .unwrap_err();
    assert!(err.is_validation(), "{err:?}");
    assert_eq!(unknown.request_count(), 1);
}

#[test]
fn repeated_query_keys_stay_repeated_on_the_wire() {
    let mock = MockTransport::new();
    mock.push_json(200, PAGE.body());
    client(&mock)
        .contacts()
        .list(
            &Query::new()
                .filter("status:eq:active")
                .filter("plan:eq:pro")
                .limit(10),
        )
        .unwrap();
    assert_eq!(
        mock.only_request().url,
        format!(
            "{BASE}/v1/contacts?filter%5B%5D=status%3Aeq%3Aactive\
             &filter%5B%5D=plan%3Aeq%3Apro&limit=10"
        )
    );
}

#[test]
fn path_parameters_are_percent_encoded() {
    let mock = MockTransport::new();
    mock.push_json(200, OBJ.body());
    client(&mock)
        .contacts()
        .remove_tag("c_1", "vip customer/eu")
        .unwrap();
    assert_eq!(
        mock.only_request().url,
        format!("{BASE}/v1/contacts/c_1/tags/vip%20customer%2Feu")
    );
}

#[test]
fn the_escape_hatch_uses_the_same_auth_and_idempotency_machinery() {
    let mock = MockTransport::new();
    mock.push_json(200, r#"{"ok":true}"#);
    let out = client(&mock)
        .request(
            "post",
            "v1/some/unmapped/route",
            &Query::new().push("a", "1"),
            Some(&json!({ "hello": "world" })),
        )
        .unwrap();
    assert_eq!(out["ok"], true);

    let request = mock.only_request();
    assert_eq!(request.method, "POST", "the verb is upcased");
    assert_eq!(request.url, format!("{BASE}/v1/some/unmapped/route?a=1"));
    assert_eq!(request.header("authorization"), Some("Bearer ck_test_key"));
    assert!(
        request.header("idempotency-key").is_some(),
        "an escape-hatch write is still a write"
    );
    assert_eq!(request.body_text().unwrap(), r#"{"hello":"world"}"#);
}

#[test]
fn the_escape_hatch_can_also_read_and_paginate() {
    let mock = MockTransport::new();
    mock.push_json(200, r#"{"items":[{"n":1}],"nextCursor":"c2"}"#)
        .push_json(200, r#"{"items":[{"n":2}],"nextCursor":null}"#);
    let rows: Vec<serde_json::Value> = client(&mock)
        .paginate::<serde_json::Value>("/v1/anything", &Query::new())
        .map(Result::unwrap)
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["n"], 2);

    let mock = MockTransport::new();
    mock.push_json(200, r#"{"items":[],"nextCursor":null}"#);
    let page: chatly::Page<serde_json::Value> = client(&mock)
        .request_as("GET", "/v1/anything", &Query::new(), None)
        .unwrap();
    assert!(page.is_last());
    assert_eq!(mock.only_request().header("idempotency-key"), None);
}

#[test]
fn a_204_decodes_to_null_rather_than_failing() {
    let mock = MockTransport::new();
    mock.push(chatly::HttpResponse::new(204, vec![], Vec::new()));
    let out = client(&mock).macros().delete("m_1").unwrap();
    assert!(out.is_null());
}
