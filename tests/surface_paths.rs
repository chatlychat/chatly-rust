//! The routes added on 2026-10-14 whose paths invite a specific mistake.
//!
//! `tests/surface_lock.rs` already drives all 159 methods and compares every
//! verb and path against the manifest, which is strictly stronger than
//! restating a path by hand. So this file is not the other 104 — it is the
//! handful where the path is NOT what the method name suggests, or where two
//! sibling resources spell the same operation differently, written out
//! because those are the ones a reasonable author gets wrong and a reviewer
//! does not notice:
//!
//! - `/v1/ai-providers/{provider}/test` — a hyphenated SIBLING of `/v1/ai`,
//!   not a child of it, and `/v1/ai/providers` is unmapped.
//! - `/v1/channels/{id}/secret/rotate` versus
//!   `/v1/customer-cards/{id}/rotate-secret` — one operation, two spellings,
//!   on two resources in the same SDK.
//! - `/v1/tickets/open-count/{conversationId}` — the literal segment comes
//!   FIRST, and the id is a conversation's rather than a ticket's.
//! - `/v1/macros/templates/{slug}` sitting beside `/v1/macros/{id}`.
//! - `PUT /v1/ai/tone` and `PUT /v1/email-templates/{kind}` — two writes
//!   whose verb is the thing to get right, because neither has merge
//!   semantics.
//!
//! Split from `tests/surface.rs` for the contract's 650-line ceiling (§9).
//! The shared `expect` helper lives in `tests/common/mod.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chatly::Query;
use common::{client, expect, BodyShape, MockTransport};
use serde_json::json;

/// A bare JSON array.
const ARRAY: BodyShape = BodyShape::Array;
/// One entity, a report, a settings blob, or a write's result.
const OBJ: BodyShape = BodyShape::Object;

/// `/v1/ai-providers` is a SIBLING of `/v1/ai`, not a child of it.
///
/// Four routes under one namespace, two prefixes, and the hyphen is the only
/// thing telling them apart. `ai().providers()` must not reach
/// `/v1/ai/providers`, which is unmapped and would 404.
#[test]
fn ai_provider_routes_are_hyphenated_siblings_of_the_ai_prefix() {
    expect(
        OBJ,
        |c| c.ai().providers(&Query::new()),
        "GET",
        "/v1/ai-providers",
    );
    expect(
        OBJ,
        |c| c.ai().test_provider("openai"),
        "POST",
        "/v1/ai-providers/openai/test",
    );
    // And these really are under `/v1/ai`.
    expect(OBJ, |c| c.ai().quota(&Query::new()), "GET", "/v1/ai/quota");
    expect(
        ARRAY,
        |c| c.ai().tools(&Query::new()),
        "GET",
        "/v1/ai/tools",
    );
}

/// Tone is a PUT that replaces the whole set and answers a bare array.
///
/// The verb is the mistake worth catching: a PATCH would be a merge, and this
/// route has no merge semantics at all.
#[test]
fn setting_the_tone_rules_is_a_put_on_the_collection() {
    expect(
        ARRAY,
        |c| c.ai().set_tone(&json!({ "rules": [] })),
        "PUT",
        "/v1/ai/tone",
    );
    expect(
        OBJ,
        |c| {
            c.ai()
                .generate_tone(&json!({ "styleGuide": "Write plainly." }))
        },
        "POST",
        "/v1/ai/tone/generate",
    );
}

/// A delivery is addressed by TWO ids, bot then delivery, with the literal
/// tail last.
#[test]
fn bot_delivery_paths_nest_both_ids() {
    expect(
        OBJ,
        |c| c.bots().delivery("b_1", "d_1"),
        "GET",
        "/v1/bots/b_1/deliveries/d_1",
    );
    expect(
        OBJ,
        |c| c.bots().redeliver("b_1", "d_1"),
        "POST",
        "/v1/bots/b_1/deliveries/d_1/redeliver",
    );
    expect(
        OBJ,
        |c| c.bots().rotate_external_secret("b_1"),
        "POST",
        "/v1/bots/b_1/external/rotate-secret",
    );
}

/// Blocks hang off a MESSAGE inside a conversation — two ids in the middle of
/// the path, with a literal segment on either side.
#[test]
fn patching_message_blocks_carries_both_ids() {
    expect(
        OBJ,
        |c| {
            c.conversations()
                .update_blocks("v_1", "m_1", &json!({ "patches": [] }))
        },
        "PATCH",
        "/v1/conversations/v_1/messages/m_1/blocks",
    );
}

/// Two resources, one operation, two spellings — and the two are not
/// interchangeable.
///
/// A channel rotates at `{id}/secret/rotate`; a customer card at
/// `{id}/rotate-secret`. Nothing but a test keeps them straight.
#[test]
fn secret_rotation_is_spelled_differently_on_each_resource() {
    let mock = expect(
        OBJ,
        |c| c.channels().rotate_secret("ch_1"),
        "POST",
        "/v1/channels/ch_1/secret/rotate",
    );
    // No body, and still a write: the key goes out even though this route
    // declares `@NoIdempotency` and will ignore it.
    let request = mock.only_request();
    assert_eq!(request.body, None);
    assert!(request.header("idempotency-key").is_some());

    expect(
        OBJ,
        |c| c.customer_cards().rotate_secret("cc_1"),
        "POST",
        "/v1/customer-cards/cc_1/rotate-secret",
    );
    expect(
        OBJ,
        |c| c.customer_card_endpoints().rotate_secret("ce_1"),
        "POST",
        "/v1/customer-card-endpoints/ce_1/rotate-secret",
    );
    expect(
        OBJ,
        |c| c.customer_card_endpoints().activate_secret("ce_1"),
        "POST",
        "/v1/customer-card-endpoints/ce_1/activate-secret",
    );
}

/// Starting a proactive conversation sends the id in the path and nothing
/// else — no body, no query — and still carries an idempotency key.
///
/// Asserted on what the SDK SENT rather than on what the fake answered. A
/// transport that echoes the request back agrees with any bug in it, which is
/// how two sibling SDKs shipped a paginator over a bare-array route with
/// green suites.
///
/// The key assertion is not decoration here, and it is the opposite case to
/// `rotate_secret` above: that handler declares `@NoIdempotency` and ignores
/// the key, while this one sits under a class-level
/// `@RequireIdempotencyKey()` with no opt-out — so a request without the
/// header is a **428** in production, and a visitor gets no greeting at all.
///
/// `start-conversation` is hyphenated and the id comes FIRST, unlike
/// `tickets().open_count` below.
#[test]
fn starting_a_conversation_sends_only_the_contact_id_in_the_path() {
    let mock = expect(
        OBJ,
        |c| c.visitors().start_conversation("cont_1"),
        "POST",
        "/v1/visitors/cont_1/start-conversation",
    );
    let request = mock.only_request();
    assert_eq!(
        request.body, None,
        "the handler has no @Body(); an empty JSON object is not the same as no body"
    );
    assert!(
        !request.url.contains('?'),
        "the handler has no @Query() either: {}",
        request.url
    );
    assert!(
        request.header("idempotency-key").is_some(),
        "@RequireIdempotencyKey() is on the class with no opt-out here — \
         without the header this route answers 428"
    );
}

/// `open-count` puts the literal segment BEFORE the id, and the id is a
/// conversation's.
#[test]
fn the_open_ticket_count_is_keyed_on_a_conversation_after_a_literal_segment() {
    expect(
        OBJ,
        |c| c.tickets().open_count("v_1"),
        "GET",
        "/v1/tickets/open-count/v_1",
    );
}

/// A template is addressed by slug under a literal `templates` segment, which
/// sits beside `GET /v1/macros/{id}`.
#[test]
fn macro_templates_sit_under_a_literal_segment_beside_the_id_route() {
    expect(
        OBJ,
        |c| c.macros().template("first-reply"),
        "GET",
        "/v1/macros/templates/first-reply",
    );
    expect(
        OBJ,
        |c| c.macros().actions(&Query::new()),
        "GET",
        "/v1/macros/actions",
    );
    expect(OBJ, |c| c.macros().get("m_1"), "GET", "/v1/macros/m_1");
}

/// An email template is addressed by KIND, and the kind is percent-encoded.
///
/// The resource is the kind rather than a row id, so these are a PUT and a
/// DELETE on a path segment a caller supplies — and an unknown kind must
/// reach the handler (which answers 400 naming it) rather than being mangled
/// into a different route.
#[test]
fn email_templates_are_addressed_by_kind_and_the_kind_is_encoded() {
    expect(
        OBJ,
        |c| {
            c.email_templates()
                .update("workspace_invite", &json!({ "subject": "s", "body": "b" }))
        },
        "PUT",
        "/v1/email-templates/workspace_invite",
    );
    expect(
        OBJ,
        |c| c.email_templates().delete("workspace_invite"),
        "DELETE",
        "/v1/email-templates/workspace_invite",
    );
    expect(
        OBJ,
        |c| c.email_templates().delete("not a kind/../x"),
        "DELETE",
        "/v1/email-templates/not%20a%20kind%2F..%2Fx",
    );
}

/// `/v1/workspace` singular and `/v1/workspaces` plural are different
/// resources, and the `workspace()` namespace spans both plus `/v1/api-keys`.
///
/// `secrets()` is this workspace's credential census under the singular
/// prefix; `mine()` is the collection of workspaces the caller can reach
/// under the plural one. Swapping them is a 404 against a path that looks
/// right.
#[test]
fn the_workspace_namespace_spans_a_singular_and_a_plural_prefix() {
    expect(
        ARRAY,
        |c| c.workspace().key_scopes(&Query::new()),
        "GET",
        "/v1/api-keys/scopes",
    );
    expect(
        OBJ,
        |c| c.workspace().secrets(&Query::new()),
        "GET",
        "/v1/workspace/secrets",
    );
    expect(
        ARRAY,
        |c| c.workspace().mine(&Query::new()),
        "GET",
        "/v1/workspaces/me",
    );
}

/// Deleting a subscription is `/v1/webhooks/{id}` — the bare endpoint path,
/// not anything under `/deliveries`.
///
/// Worth one assertion because this namespace holds two resources on
/// overlapping prefixes: the endpoints at `/v1/webhooks/{id}` and the
/// delivery log at `/v1/webhooks/deliveries/{id}`. A delete aimed at the
/// wrong one would hit a route that exists.
#[test]
fn deleting_a_subscription_does_not_touch_the_delivery_log() {
    let mock = expect(
        OBJ,
        |c| c.webhooks().delete("wh_1"),
        "DELETE",
        "/v1/webhooks/wh_1",
    );
    assert!(mock.only_request().header("idempotency-key").is_some());
}

/// `usage/daily` needs an `eventType`, and an empty one is refused before a
/// request is made.
///
/// Same rule as `billing().usage()`, and worth asserting separately: the two
/// routes are different handlers and only one of them was implemented first.
#[test]
fn daily_usage_requires_an_event_type_and_refuses_an_empty_one_locally() {
    expect(
        OBJ,
        |c| c.billing().usage_daily("ai.tokens", &Query::new()),
        "GET",
        "/v1/billing/usage/daily?eventType=ai.tokens",
    );

    let mock = MockTransport::new();
    let err = client(&mock)
        .billing()
        .usage_daily("  ", &Query::new())
        .expect_err("an empty event type must not be sent");
    assert!(matches!(err, chatly::ChatlyError::Config(_)), "{err:?}");
    assert_eq!(
        mock.request_count(),
        0,
        "it must refuse locally rather than spending a round trip on a 400"
    );
}

/// `sync` sends the body that decides delta-versus-full, and `full` is the
/// field that matters after a reader-side fix.
#[test]
fn a_docs_source_sync_says_whether_it_is_full() {
    let mock = expect(
        OBJ,
        |c| c.docs_sources().sync_now("ds_1", true),
        "POST",
        "/v1/docs-sources/ds_1/sync",
    );
    let body = mock.only_request().body.expect("a body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("json"),
        json!({ "full": true })
    );
}
