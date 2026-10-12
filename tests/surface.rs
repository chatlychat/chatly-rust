//! Every one of the 55 methods in `packages/sdk-spec/surface.json`, called
//! once against a mock, asserting the HTTP verb and the path it produced.
//!
//! This is the behavioural half of what
//! `scripts/check-sdk-surface-parity.mjs` checks by name. The gate can see
//! that a method called `cancel_scheduled` exists; only a test can see that
//! it issues `DELETE /v1/conversations/scheduled-messages/{id}` and not
//! `DELETE /v1/conversations/{id}/scheduled-messages`, which is the mistake
//! that path invites.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chatly::{ChatlyClient, Query};
use common::{client, MockTransport};
use serde_json::json;
use std::sync::Arc;

const BASE: &str = "https://api.chatlychat.com";

/// Run `call`, then assert the single request it made.
fn expect<T>(
    body: &str,
    call: impl FnOnce(&ChatlyClient) -> T,
    method: &str,
    path: &str,
) -> Arc<MockTransport> {
    let mock = MockTransport::new();
    mock.push_json(200, body);
    let chatly = client(&mock);
    let _ = call(&chatly);
    let request = mock.only_request();
    assert_eq!(request.method, method, "verb for {path}");
    assert_eq!(request.url, format!("{BASE}{path}"), "path for {path}");
    mock
}

/// For the paginated list endpoints, whose bodies must be a page.
const PAGE: &str = r#"{"items":[],"nextCursor":null}"#;
const OBJ: &str = "{}";

#[test]
fn root_whoami() {
    expect(OBJ, |c| c.whoami(), "GET", "/v1/api-keys/whoami");
}

#[test]
fn contacts_surface() {
    expect(
        PAGE,
        |c| c.contacts().list(&Query::new()),
        "GET",
        "/v1/contacts",
    );
    expect(OBJ, |c| c.contacts().get("c_1"), "GET", "/v1/contacts/c_1");
    expect(
        OBJ,
        |c| c.contacts().upsert(&json!({})),
        "POST",
        "/v1/contacts",
    );
    expect(
        OBJ,
        |c| c.contacts().bulk_upsert(&json!({ "contacts": [] })),
        "POST",
        "/v1/contacts/bulk",
    );
    expect(
        OBJ,
        |c| c.contacts().merge("c_1", "c_2"),
        "POST",
        "/v1/contacts/c_1/merge",
    );
    expect(
        OBJ,
        |c| c.contacts().add_tags("c_1", &["vip"]),
        "POST",
        "/v1/contacts/c_1/tags",
    );
    expect(
        OBJ,
        |c| c.contacts().remove_tag("c_1", "vip"),
        "DELETE",
        "/v1/contacts/c_1/tags/vip",
    );
    expect(
        OBJ,
        |c| c.contacts().unsubscribe("c_1", true, None),
        "POST",
        "/v1/contacts/c_1/unsubscribe",
    );
    expect(
        OBJ,
        |c| c.contacts().journey("c_1", &Query::new()),
        "GET",
        "/v1/contacts/c_1/journey",
    );
}

#[test]
fn conversations_surface() {
    expect(
        PAGE,
        |c| c.conversations().list(&Query::new()),
        "GET",
        "/v1/conversations",
    );
    expect(
        OBJ,
        |c| c.conversations().get("v_1"),
        "GET",
        "/v1/conversations/v_1",
    );
    expect(
        PAGE,
        |c| c.conversations().messages("v_1", &Query::new()),
        "GET",
        "/v1/conversations/v_1/messages",
    );
    expect(
        OBJ,
        |c| c.conversations().send("v_1", &json!({ "body": "hi" })),
        "POST",
        "/v1/conversations/v_1/messages",
    );
    expect(
        OBJ,
        |c| c.conversations().unattended(&Query::new()),
        "GET",
        "/v1/conversations/unattended",
    );
    expect(
        OBJ,
        |c| c.conversations().agent_handling("v_1"),
        "GET",
        "/v1/conversations/v_1/agent-handling",
    );
    expect(
        OBJ,
        |c| {
            c.conversations()
                .set_agent_handling("v_1", &json!({ "state": "handled" }))
        },
        "PUT",
        "/v1/conversations/v_1/agent-handling",
    );
    expect(
        OBJ,
        |c| c.conversations().generated_replies("v_1", &Query::new()),
        "GET",
        "/v1/conversations/v_1/generated-replies",
    );
    expect(
        OBJ,
        |c| {
            c.conversations()
                .generate_reply("v_1", &json!({ "body": "draft" }))
        },
        "POST",
        "/v1/conversations/v_1/generated-replies",
    );
    expect(
        OBJ,
        |c| c.conversations().scheduled_messages("v_1", &Query::new()),
        "GET",
        "/v1/conversations/v_1/scheduled-messages",
    );
    expect(
        OBJ,
        |c| {
            c.conversations()
                .schedule("v_1", "later", "2026-10-13T09:00:00Z")
        },
        "POST",
        "/v1/conversations/v_1/scheduled-messages",
    );
    // The path is keyed on the scheduled id ALONE — no conversation id.
    expect(
        OBJ,
        |c| c.conversations().cancel_scheduled("s_1"),
        "DELETE",
        "/v1/conversations/scheduled-messages/s_1",
    );
}

#[test]
fn tickets_surface() {
    expect(
        PAGE,
        |c| c.tickets().list(&Query::new()),
        "GET",
        "/v1/tickets",
    );
    expect(
        OBJ,
        |c| c.tickets().comments("t_1", &Query::new()),
        "GET",
        "/v1/tickets/t_1/comments",
    );
    expect(
        OBJ,
        |c| c.tickets().for_conversation("v_1"),
        "GET",
        "/v1/tickets/for-conversation/v_1",
    );
    expect(
        OBJ,
        |c| c.tickets().queues(&Query::new()),
        "GET",
        "/v1/tickets/queues",
    );
    expect(
        OBJ,
        |c| c.tickets().merge_loser("t_1", "t_2"),
        "POST",
        "/v1/tickets/t_1/merge",
    );
    expect(
        OBJ,
        |c| c.tickets().split("t_1", &json!({ "title": "split" })),
        "POST",
        "/v1/tickets/t_1/split",
    );
    expect(
        OBJ,
        |c| {
            c.tickets()
                .bulk(&json!({ "ids": ["t_1"], "action": "status" }))
        },
        "POST",
        "/v1/tickets/bulk",
    );
}

#[test]
fn macros_surface() {
    expect(
        PAGE,
        |c| c.macros().list(&Query::new()),
        "GET",
        "/v1/macros",
    );
    expect(OBJ, |c| c.macros().create(&json!({})), "POST", "/v1/macros");
    expect(
        OBJ,
        |c| c.macros().update("m_1", &json!({})),
        "PATCH",
        "/v1/macros/m_1",
    );
    expect(
        OBJ,
        |c| c.macros().delete("m_1"),
        "DELETE",
        "/v1/macros/m_1",
    );
}

#[test]
fn kb_surface() {
    expect(
        PAGE,
        |c| c.kb().articles(&Query::new()),
        "GET",
        "/v1/kb/articles",
    );
    expect(
        OBJ,
        |c| c.kb().article("how-to"),
        "GET",
        "/v1/kb/articles/how-to",
    );
    expect(
        OBJ,
        |c| c.kb().collections(&Query::new()),
        "GET",
        "/v1/kb/collections",
    );
}

#[test]
fn search_surface() {
    expect(
        OBJ,
        |c| c.search().query("refund", &Query::new()),
        "GET",
        "/v1/search?q=refund",
    );
}

/// Not a manifest entry — a convenience over the same route. Asserted
/// because the bracketed spelling it must NOT use was silently stripped by
/// the API and ran the search unnarrowed while returning 200.
#[test]
fn search_kb_narrows_with_a_plain_kinds_key() {
    let mock = MockTransport::new();
    mock.push_json(200, OBJ);
    client(&mock).search().kb("refund", &Query::new()).unwrap();
    let url = mock.only_request().url;
    assert_eq!(url, format!("{BASE}/v1/search?q=refund&kinds=kb"));
    assert!(!url.contains("kinds%5B%5D"), "must not send kinds[]: {url}");

    // Two kinds stay two parameters.
    let mock = MockTransport::new();
    mock.push_json(200, OBJ);
    client(&mock)
        .search()
        .query(
            "refund",
            &Query::new().push("kinds", "kb").push("kinds", "ticket"),
        )
        .unwrap();
    assert_eq!(
        mock.only_request().url,
        format!("{BASE}/v1/search?kinds=kb&kinds=ticket&q=refund")
    );
}

#[test]
fn events_surface() {
    expect(
        OBJ,
        |c| {
            c.events()
                .track(&json!({ "contactId": "c_1", "name": "x" }))
        },
        "POST",
        "/v1/cdp/events",
    );
    expect(
        PAGE,
        |c| c.events().list(&Query::new()),
        "GET",
        "/v1/cdp/events",
    );
}

#[test]
fn webhooks_surface() {
    expect(
        PAGE,
        |c| c.webhooks().deliveries(&Query::new()),
        "GET",
        "/v1/webhooks/deliveries",
    );
    expect(
        OBJ,
        |c| c.webhooks().delivery("d_1"),
        "GET",
        "/v1/webhooks/deliveries/d_1",
    );
    let mock = expect(
        OBJ,
        |c| c.webhooks().redeliver("d_1"),
        "POST",
        "/v1/webhooks/deliveries/d_1/redeliver",
    );
    // The route takes no body, so none is sent — but it is still a write.
    let request = mock.only_request();
    assert_eq!(request.body, None);
    assert_eq!(request.header("content-type"), None);
    assert!(request.header("idempotency-key").is_some());
}

#[test]
fn reports_surface() {
    expect(
        OBJ,
        |c| c.reports().overview(&Query::new()),
        "GET",
        "/v1/reports/overview",
    );
    expect(
        OBJ,
        |c| c.reports().agents(&Query::new()),
        "GET",
        "/v1/reports/agents",
    );
    expect(
        OBJ,
        |c| c.reports().conversations_by_day(&Query::new()),
        "GET",
        "/v1/reports/conversations-by-day",
    );
    expect(
        OBJ,
        |c| c.reports().sla(&Query::new()),
        "GET",
        "/v1/reports/sla",
    );
}

#[test]
fn channels_surface() {
    expect(
        OBJ,
        |c| c.channels().list(&Query::new()),
        "GET",
        "/v1/channels",
    );
    expect(
        OBJ,
        |c| c.channels().get("ch_1"),
        "GET",
        "/v1/channels/ch_1",
    );
}

#[test]
fn billing_surface() {
    expect(
        OBJ,
        |c| c.billing().plans(&Query::new()),
        "GET",
        "/v1/billing/plans",
    );
    expect(
        OBJ,
        |c| c.billing().usage(&Query::new()),
        "GET",
        "/v1/billing/usage",
    );
}

#[test]
fn workspace_surface() {
    expect(
        OBJ,
        |c| c.workspace().settings(),
        "GET",
        "/v1/workspace/settings",
    );
    expect(
        OBJ,
        |c| c.workspace().labels(&Query::new()),
        "GET",
        "/v1/labels",
    );
    expect(
        OBJ,
        |c| c.workspace().inboxes(&Query::new()),
        "GET",
        "/v1/inboxes",
    );
    expect(
        OBJ,
        |c| c.workspace().team_members("tm_1", &Query::new()),
        "GET",
        "/v1/teams/tm_1/members",
    );
    expect(
        OBJ,
        |c| c.workspace().agent_skills(&Query::new()),
        "GET",
        "/v1/agents/skills",
    );
}

// ---------------------------------------------------------------------------
// Bodies the convenience methods build, asserted against the API's verified
// zod field names. A field name wrong here is a write that validates and
// does nothing.
// ---------------------------------------------------------------------------

fn sent_body(call: impl FnOnce(&ChatlyClient)) -> serde_json::Value {
    let mock = MockTransport::new();
    mock.push_json(200, OBJ);
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
    mock.push_json(200, OBJ)
        .push_json(200, OBJ)
        .push_json(200, OBJ);
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

#[test]
fn repeated_query_keys_stay_repeated_on_the_wire() {
    let mock = MockTransport::new();
    mock.push_json(200, PAGE);
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
    mock.push_json(200, OBJ);
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
