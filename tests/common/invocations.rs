//! Every method in `packages/sdk-spec/surface.json`, as a closure that calls
//! it.
//!
//! Split out of `tests/surface_lock.rs` so neither file approaches the
//! contract's 650-line ceiling (§9): 162 closures do not fit beside the
//! assertions that drive them, and the seam is data versus logic. The other
//! three SDKs that finished this surface split the same way.
//!
//! Split again, for the same ceiling, along the line this package already
//! draws everywhere else: the 55 routes that shipped in 0.1.0 are here, and
//! the 107 added on 2026-10-14 are in `invocations_added.rs`. `invocations()`
//! concatenates them and `surface_lock.rs` checks the result against the
//! manifest in both directions, so neither half can quietly shrink.
//!
//! Nothing in here can be satisfied by a comment. If a method is missing,
//! the closure does not compile; if it builds the wrong URL, the recorded
//! path differs from the manifest and `surface_lock.rs` fails. That is the
//! whole reason the parity evidence is executed rather than grepped — the
//! first version of the shared gate counted a doc comment explaining a
//! broken route as an implementation of it.

#![allow(dead_code)]

use chatly::{ChatlyClient, Query};
use serde_json::json;

use super::Returned;

/// One method call, returning **both** the decoded value and the error.
///
/// Returning a `Result` rather than `()` is load-bearing: these closures once
/// wrote `let _ = …`, which threw the error away, so a route answered with a
/// shape its own return type could not decode still looked fine here. That is
/// half of why `macros().list` could raise `ChatlyError::Decode` for real
/// callers with this suite green.
///
/// Carrying the VALUE rather than `()` is the other half. A decoder can
/// succeed and still be wrong — unwrap a page into its `items`, wrap an array
/// into an envelope, or decode into a default that drops the payload — and
/// the request it issued is perfect in every one of those cases.
/// [`Returned`] exists so `observe` can classify what came back.
pub type Invocation = fn(&ChatlyClient) -> Result<Returned, chatly::ChatlyError>;

/// Every method in the manifest, as `(ns, name, invocation)`.
///
/// The closures capture nothing, so they coerce to `fn` pointers and this is
/// a plain table. Arguments are throwaway; only the request that comes out,
/// and whether the call succeeded, matter. Each closure must issue
/// **exactly one** request — `add_tags` is given a single tag for that
/// reason, since the underlying route takes one tag per call, and
/// `docs_sources.sync_now` is not used here for the same reason it would be
/// fine if it were: it is one call either way.
///
/// `Returned::page` is used for exactly the thirteen routes whose declared
/// shape is `page`; everything else is `Returned::value`. Getting that wrong
/// is caught rather than hidden — `surface_lock.rs` compares the classified
/// shape against the declared one, and `Returned::Page` can only ever
/// classify as `page`.
#[allow(clippy::type_complexity)]
pub fn invocations() -> Vec<(&'static str, &'static str, Invocation)> {
    let mut all = released_in_0_1_0();
    all.extend(super::invocations_added::added_2026_10_14());
    all
}

/// The 55 that shipped in 0.1.0.
#[allow(clippy::type_complexity)]
fn released_in_0_1_0() -> Vec<(&'static str, &'static str, Invocation)> {
    vec![
        ("root", "whoami", |c| (c.whoami()).map(Returned::value)),
        ("root", "me", |c| (c.me()).map(Returned::value)),
        // -- contacts -----------------------------------------------------
        ("contacts", "list", |c| {
            (c.contacts().list(&Query::new())).map(Returned::page)
        }),
        ("contacts", "get", |c| {
            (c.contacts().get("c_1")).map(Returned::value)
        }),
        ("contacts", "upsert", |c| {
            (c.contacts().upsert(&json!({}))).map(Returned::value)
        }),
        ("contacts", "bulk_upsert", |c| {
            (c.contacts().bulk_upsert(&json!({ "contacts": [] }))).map(Returned::value)
        }),
        ("contacts", "merge", |c| {
            (c.contacts().merge("c_1", "c_2")).map(Returned::value)
        }),
        ("contacts", "add_tags", |c| {
            (c.contacts().add_tags("c_1", &["vip"])).map(Returned::value)
        }),
        ("contacts", "remove_tag", |c| {
            (c.contacts().remove_tag("c_1", "vip")).map(Returned::value)
        }),
        ("contacts", "unsubscribe", |c| {
            (c.contacts().unsubscribe("c_1", true, None)).map(Returned::value)
        }),
        ("contacts", "journey", |c| {
            (c.contacts().journey("c_1", &Query::new())).map(Returned::page)
        }),
        // -- conversations ------------------------------------------------
        ("conversations", "list", |c| {
            (c.conversations().list(&Query::new())).map(Returned::page)
        }),
        ("conversations", "get", |c| {
            (c.conversations().get("v_1")).map(Returned::value)
        }),
        ("conversations", "messages", |c| {
            (c.conversations().messages("v_1", &Query::new())).map(Returned::page)
        }),
        ("conversations", "send", |c| {
            (c.conversations().send("v_1", &json!({ "body": "hi" }))).map(Returned::value)
        }),
        ("conversations", "unattended", |c| {
            (c.conversations().unattended(&Query::new())).map(Returned::value)
        }),
        ("conversations", "agent_handling", |c| {
            (c.conversations().agent_handling("v_1")).map(Returned::value)
        }),
        ("conversations", "set_agent_handling", |c| {
            (c.conversations()
                .set_agent_handling("v_1", &json!({ "state": "handled" })))
            .map(Returned::value)
        }),
        ("conversations", "generated_replies", |c| {
            (c.conversations().generated_replies("v_1", &Query::new())).map(Returned::value)
        }),
        ("conversations", "generate_reply", |c| {
            (c.conversations()
                .generate_reply("v_1", &json!({ "body": "draft" })))
            .map(Returned::value)
        }),
        ("conversations", "scheduled_messages", |c| {
            (c.conversations().scheduled_messages("v_1", &Query::new())).map(Returned::value)
        }),
        ("conversations", "schedule_message", |c| {
            (c.conversations().schedule_message(
                "v_1",
                &json!({ "body": "later", "sendAt": "2026-10-13T09:00:00Z" }),
            ))
            .map(Returned::value)
        }),
        ("conversations", "cancel_scheduled", |c| {
            (c.conversations().cancel_scheduled("s_1")).map(Returned::value)
        }),
        // -- tickets ------------------------------------------------------
        ("tickets", "list", |c| {
            (c.tickets().list(&Query::new())).map(Returned::page)
        }),
        ("tickets", "comments", |c| {
            (c.tickets().comments("t_1", &Query::new())).map(Returned::value)
        }),
        ("tickets", "for_conversation", |c| {
            (c.tickets().for_conversation("v_1")).map(Returned::value)
        }),
        ("tickets", "queues", |c| {
            (c.tickets().queues(&Query::new())).map(Returned::value)
        }),
        ("tickets", "merge", |c| {
            (c.tickets().merge("t_1", &json!({ "loserId": "t_2" }))).map(Returned::value)
        }),
        ("tickets", "split", |c| {
            (c.tickets().split("t_1", &json!({ "title": "split" }))).map(Returned::value)
        }),
        ("tickets", "bulk", |c| {
            (c.tickets()
                .bulk(&json!({ "ids": ["t_1"], "action": "status" })))
            .map(Returned::value)
        }),
        // -- macros -------------------------------------------------------
        ("macros", "list", |c| {
            (c.macros().list(&Query::new())).map(Returned::value)
        }),
        ("macros", "create", |c| {
            (c.macros()
                .create(&json!({ "name": "m", "scope": "workspace" })))
            .map(Returned::value)
        }),
        ("macros", "update", |c| {
            (c.macros().update("m_1", &json!({ "name": "m2" }))).map(Returned::value)
        }),
        ("macros", "delete", |c| {
            (c.macros().delete("m_1")).map(Returned::value)
        }),
        // -- kb -----------------------------------------------------------
        ("kb", "articles", |c| {
            (c.kb().articles(&Query::new())).map(Returned::page)
        }),
        ("kb", "article", |c| {
            (c.kb().article("how-to")).map(Returned::value)
        }),
        ("kb", "collections", |c| {
            (c.kb().collections(&Query::new())).map(Returned::value)
        }),
        // -- search -------------------------------------------------------
        ("search", "query", |c| {
            (c.search().query("refund", &Query::new())).map(Returned::value)
        }),
        // -- events -------------------------------------------------------
        ("events", "track", |c| {
            (c.events()
                .track(&json!({ "contactId": "c_1", "name": "evt" })))
            .map(Returned::value)
        }),
        ("events", "list", |c| {
            (c.events().list(&Query::new())).map(Returned::page)
        }),
        // -- webhooks -----------------------------------------------------
        ("webhooks", "deliveries", |c| {
            (c.webhooks().deliveries(&Query::new())).map(Returned::page)
        }),
        ("webhooks", "delivery", |c| {
            (c.webhooks().delivery("d_1")).map(Returned::value)
        }),
        ("webhooks", "redeliver", |c| {
            (c.webhooks().redeliver("d_1")).map(Returned::value)
        }),
        // -- reports ------------------------------------------------------
        ("reports", "overview", |c| {
            (c.reports().overview(&Query::new())).map(Returned::value)
        }),
        ("reports", "agents", |c| {
            (c.reports().agents(&Query::new())).map(Returned::value)
        }),
        ("reports", "conversations_by_day", |c| {
            (c.reports().conversations_by_day(&Query::new())).map(Returned::value)
        }),
        ("reports", "sla", |c| {
            (c.reports().sla(&Query::new())).map(Returned::value)
        }),
        // -- channels -----------------------------------------------------
        ("channels", "list", |c| {
            (c.channels().list(&Query::new())).map(Returned::value)
        }),
        ("channels", "get", |c| {
            (c.channels().get("ch_1")).map(Returned::value)
        }),
        // -- billing ------------------------------------------------------
        ("billing", "plans", |c| {
            (c.billing().plans(&Query::new())).map(Returned::value)
        }),
        ("billing", "usage", |c| {
            (c.billing().usage("ai.tokens", &Query::new())).map(Returned::value)
        }),
        // -- workspace ----------------------------------------------------
        ("workspace", "settings", |c| {
            (c.workspace().settings()).map(Returned::value)
        }),
        ("workspace", "labels", |c| {
            (c.workspace().labels(&Query::new())).map(Returned::value)
        }),
        ("workspace", "inboxes", |c| {
            (c.workspace().inboxes(&Query::new())).map(Returned::value)
        }),
        ("workspace", "team_members", |c| {
            (c.workspace().team_members("tm_1", &Query::new())).map(Returned::value)
        }),
        ("workspace", "agent_skills", |c| {
            (c.workspace().agent_skills(&Query::new())).map(Returned::value)
        }),
    ]
}
