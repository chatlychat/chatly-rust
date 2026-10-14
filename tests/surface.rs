//! Hand-written verb-and-path assertions: every method of the original 55,
//! plus the routes among the other 104 whose paths invite a specific
//! mistake.
//!
//! ## Why this file is not all 159
//!
//! `tests/surface_lock.rs` drives **every** method in the manifest and
//! compares the verb and path it issued against the manifest's own, route by
//! route, failing on any difference. That is strictly stronger than
//! restating a path here, and restating one is how a hand-written expectation
//! drifts: a sweep of this suite once left `/v1/macros` claiming `Page` while
//! the shape table said `Array`, and only a one-off script caught it.
//!
//! So this file earns its place twice over and no more. The original 55 stay
//! because a second, independently typed copy of a path is worth having on
//! the surface people call most. The 104 added on 2026-10-14 appear here only
//! where the path is one a reasonable author gets wrong —
//! `/v1/ai-providers/{provider}/test` rather than `/v1/ai/providers/…`,
//! `/v1/channels/{id}/secret/rotate` rather than `/rotate-secret`,
//! `/v1/tickets/open-count/{conversationId}` with the literal segment
//! FIRST — and the lock covers the rest.
//!
//! Request *bodies*, query encoding and the escape hatch are in
//! `tests/bodies.rs` — split off to stay under the contract's 650-line
//! ceiling (§9), and a fair seam: this file is about where a call goes, that
//! one about what it carries.
//!
//! This is the behavioural half of what
//! `scripts/check-sdk-surface-parity.mjs` checks by name. The gate can see
//! that a method called `cancel_scheduled` exists; only a test can see that
//! it issues `DELETE /v1/conversations/scheduled-messages/{id}` and not
//! `DELETE /v1/conversations/{id}/scheduled-messages`, which is the mistake
//! that path invites.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chatly::Query;
use common::{client, expect, shape_for, BodyShape, MockTransport, BASE};
use serde_json::json;

// Response shapes, per route. `tests/common/shapes.rs` holds the
// authoritative table for all 159 and `tests/surface_lock.rs` enforces it;
// these aliases
// just keep the call sites below readable. This file's assertions are about
// the verb and the path, so the shape does not change their outcome — but a
// wrong one here would still be a false claim about the API, and that is how
// `macros.list` got mis-typed in the first place.
/// A cursor page. Includes three routes the manifest does not mark `paged`.
const PAGE: BodyShape = BodyShape::Page;
/// A bare JSON array. Thirty-two routes do this.
const ARRAY: BodyShape = BodyShape::Array;
/// One entity, a report, a settings blob, or a write's result.
const OBJ: BodyShape = BodyShape::Object;
/// `204 No Content` — only `DELETE /v1/macros/{id}`, out of seven DELETEs.
const EMPTY: BodyShape = BodyShape::Empty;

#[test]
fn root_whoami_and_me() {
    expect(OBJ, |c| c.whoami(), "GET", "/v1/api-keys/whoami");
    // Two routes, one answer for a key — and `/v1/me` is the looser of the
    // two (no `@RequirePermission` at all), so it is the liveness check.
    // Neither path contains the other, which is the mistake to rule out.
    expect(OBJ, |c| c.me(), "GET", "/v1/me");
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
        PAGE,
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
        PAGE,
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
        ARRAY,
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
        ARRAY,
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
        ARRAY,
        |c| c.tickets().comments("t_1", &Query::new()),
        "GET",
        "/v1/tickets/t_1/comments",
    );
    expect(
        ARRAY,
        |c| c.tickets().for_conversation("v_1"),
        "GET",
        "/v1/tickets/for-conversation/v_1",
    );
    expect(
        ARRAY,
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
        ARRAY,
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
        EMPTY,
        |c| c.macros().delete("m_1"),
        "DELETE",
        "/v1/macros/m_1",
    );
}

/// `macros().list` must read a bare array. It deserialised into `Page`
/// until 2026-10-12, so a real call raised `ChatlyError::Decode` — and this
/// suite did not notice, because the fake answered every route with a page
/// envelope. Both halves of that are now asserted.
#[test]
fn macros_list_reads_a_bare_array_and_refuses_a_page_envelope() {
    let mock = MockTransport::new();
    mock.push_json(200, r#"[{"id":"m_1"},{"id":"m_2"}]"#);
    let out = client(&mock).macros().list(&Query::new()).unwrap();
    assert!(out.is_array(), "expected a bare array, got {out}");
    assert_eq!(out.as_array().map(Vec::len), Some(2));
    assert_eq!(shape_for("macros", "list"), Some(BodyShape::Array));

    // And it is not one of the paginated namespaces: there is no
    // `macros().iter()` to call. That is enforced by the type system rather
    // than here, so what this asserts is the shape table's own claim.
    assert_ne!(shape_for("macros", "list"), Some(BodyShape::Page));
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
        ARRAY,
        |c| c.kb().collections(&Query::new()),
        "GET",
        "/v1/kb/collections",
    );
}

#[test]
fn search_surface() {
    expect(
        PAGE,
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
    mock.push_json(200, OBJ.body());
    client(&mock).search().kb("refund", &Query::new()).unwrap();
    let url = mock.only_request().url;
    assert_eq!(url, format!("{BASE}/v1/search?q=refund&kinds=kb"));
    assert!(!url.contains("kinds%5B%5D"), "must not send kinds[]: {url}");

    // Two kinds stay two parameters.
    let mock = MockTransport::new();
    mock.push_json(200, OBJ.body());
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
        ARRAY,
        |c| c.reports().agents(&Query::new()),
        "GET",
        "/v1/reports/agents",
    );
    expect(
        ARRAY,
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
        ARRAY,
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
        ARRAY,
        |c| c.billing().plans(&Query::new()),
        "GET",
        "/v1/billing/plans",
    );
    expect(
        OBJ,
        |c| c.billing().usage("ai.tokens", &Query::new()),
        "GET",
        "/v1/billing/usage?eventType=ai.tokens",
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
        ARRAY,
        |c| c.workspace().labels(&Query::new()),
        "GET",
        "/v1/labels",
    );
    expect(
        ARRAY,
        |c| c.workspace().inboxes(&Query::new()),
        "GET",
        "/v1/inboxes",
    );
    expect(
        ARRAY,
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
