//! The 107 methods added on 2026-10-14, when this crate went from 55 routes
//! to the whole manifest.
//!
//! A continuation of `invocations.rs` and nothing more: same type, same
//! rules, split only for the contract's 650-line ceiling (§9). The seam is
//! the one this package draws everywhere else — what shipped in 0.1.0 versus
//! what was added when the surface was finished — rather than an arbitrary
//! halfway point through a namespace.
//!
//! Three of these (`root.me`, `webhooks.delete`, `workspace.mine`) joined the
//! shared manifest later the same day, after the other 104 were written.

#![allow(dead_code)]

use chatly::Query;
use serde_json::json;

use super::invocations::Invocation;
use super::Returned;

/// Every method added on 2026-10-14, as `(ns, name, invocation)`.
#[allow(clippy::type_complexity)]
pub fn added_2026_10_14() -> Vec<(&'static str, &'static str, Invocation)> {
    vec![
        // -- ai -----------------------------------------------------------
        ("ai", "providers", |c| {
            (c.ai().providers(&Query::new())).map(Returned::value)
        }),
        ("ai", "add_provider", |c| {
            (c.ai()
                .add_provider(&json!({ "provider": "openai", "apiKey": "sk_test" })))
            .map(Returned::value)
        }),
        ("ai", "test_provider", |c| {
            (c.ai().test_provider("openai")).map(Returned::value)
        }),
        ("ai", "quota", |c| {
            (c.ai().quota(&Query::new())).map(Returned::value)
        }),
        ("ai", "tone", |c| {
            (c.ai().tone(&Query::new())).map(Returned::value)
        }),
        ("ai", "set_tone", |c| {
            (c.ai().set_tone(&json!({ "rules": [] }))).map(Returned::value)
        }),
        ("ai", "tone_preview", |c| {
            (c.ai().tone_preview(&Query::new())).map(Returned::value)
        }),
        ("ai", "generate_tone", |c| {
            (c.ai()
                .generate_tone(&json!({ "styleGuide": "Write plainly." })))
            .map(Returned::value)
        }),
        ("ai", "tools", |c| {
            (c.ai().tools(&Query::new())).map(Returned::value)
        }),
        ("ai", "create_tool", |c| {
            (c.ai().create_tool(&json!({ "name": "lookup_order" }))).map(Returned::value)
        }),
        ("ai", "update_tool", |c| {
            (c.ai().update_tool("tl_1", &json!({ "name": "renamed" }))).map(Returned::value)
        }),
        ("ai", "delete_tool", |c| {
            (c.ai().delete_tool("tl_1")).map(Returned::value)
        }),
        // -- billing ------------------------------------------------------
        ("billing", "summary", |c| {
            (c.billing().summary(&Query::new())).map(Returned::value)
        }),
        ("billing", "invoices", |c| {
            (c.billing().invoices(&Query::new())).map(Returned::value)
        }),
        ("billing", "usage_monthly", |c| {
            (c.billing().usage_monthly(&Query::new())).map(Returned::value)
        }),
        ("billing", "usage_daily", |c| {
            (c.billing().usage_daily("ai.tokens", &Query::new())).map(Returned::value)
        }),
        // -- bot_routes ---------------------------------------------------
        ("bot_routes", "list", |c| {
            (c.bot_routes().list(&Query::new())).map(Returned::value)
        }),
        ("bot_routes", "fields", |c| {
            (c.bot_routes().fields(&Query::new())).map(Returned::value)
        }),
        ("bot_routes", "create", |c| {
            (c.bot_routes()
                .create(&json!({ "botId": "b_1", "name": "vip" })))
            .map(Returned::value)
        }),
        ("bot_routes", "update", |c| {
            (c.bot_routes().update("br_1", &json!({ "enabled": false }))).map(Returned::value)
        }),
        ("bot_routes", "delete", |c| {
            (c.bot_routes().delete("br_1")).map(Returned::value)
        }),
        ("bot_routes", "resolve", |c| {
            (c.bot_routes()
                .resolve(&json!({ "body": "hi", "channelType": "web" })))
            .map(Returned::value)
        }),
        // -- bots ---------------------------------------------------------
        ("bots", "list", |c| {
            (c.bots().list(&Query::new())).map(Returned::page)
        }),
        ("bots", "get", |c| {
            (c.bots().get("b_1")).map(Returned::value)
        }),
        ("bots", "create", |c| {
            (c.bots().create(&json!({ "name": "Support" }))).map(Returned::value)
        }),
        ("bots", "update", |c| {
            (c.bots().update("b_1", &json!({ "status": "active" }))).map(Returned::value)
        }),
        ("bots", "update_external", |c| {
            (c.bots()
                .update_external("b_1", &json!({ "timeoutSeconds": 20 })))
            .map(Returned::value)
        }),
        ("bots", "rotate_external_secret", |c| {
            (c.bots().rotate_external_secret("b_1")).map(Returned::value)
        }),
        ("bots", "analytics", |c| {
            (c.bots().analytics("b_1", &Query::new())).map(Returned::value)
        }),
        ("bots", "deliveries", |c| {
            (c.bots().deliveries("b_1", &Query::new())).map(Returned::page)
        }),
        ("bots", "delivery", |c| {
            (c.bots().delivery("b_1", "d_1")).map(Returned::value)
        }),
        ("bots", "redeliver", |c| {
            (c.bots().redeliver("b_1", "d_1")).map(Returned::value)
        }),
        ("bots", "test_delivery", |c| {
            (c.bots().test_delivery("b_1")).map(Returned::value)
        }),
        // -- brands -------------------------------------------------------
        ("brands", "list", |c| {
            (c.brands().list(&Query::new())).map(Returned::value)
        }),
        ("brands", "create", |c| {
            (c.brands()
                .create(&json!({ "name": "Acme", "slug": "acme" })))
            .map(Returned::value)
        }),
        ("brands", "update", |c| {
            (c.brands().update("br_1", &json!({ "name": "Acme Ltd" }))).map(Returned::value)
        }),
        ("brands", "delete", |c| {
            (c.brands().delete("br_1")).map(Returned::value)
        }),
        // -- canvas_steps -------------------------------------------------
        ("canvas_steps", "get", |c| {
            (c.canvas_steps().get("cs_1")).map(Returned::value)
        }),
        ("canvas_steps", "resolve", |c| {
            (c.canvas_steps()
                .resolve("cs_1", &json!({ "canvas": { "blocks": [] } })))
            .map(Returned::value)
        }),
        // -- channels -----------------------------------------------------
        ("channels", "adapters", |c| {
            (c.channels().adapters(&Query::new())).map(Returned::value)
        }),
        ("channels", "create", |c| {
            (c.channels()
                .create(&json!({ "type": "api", "name": "Custom", "publicId": "custom" })))
            .map(Returned::value)
        }),
        ("channels", "update", |c| {
            (c.channels().update("ch_1", &json!({ "enabled": false }))).map(Returned::value)
        }),
        ("channels", "rotate_secret", |c| {
            (c.channels().rotate_secret("ch_1")).map(Returned::value)
        }),
        // -- contacts (added 2026-10-14) ----------------------------------
        ("contacts", "mood", |c| {
            (c.contacts().mood("c_1", &Query::new())).map(Returned::value)
        }),
        ("contacts", "cards", |c| {
            (c.contacts().cards("c_1", &json!({}))).map(Returned::value)
        }),
        // -- conversations (added 2026-10-14) -----------------------------
        ("conversations", "linked", |c| {
            (c.conversations().linked("v_1", &Query::new())).map(Returned::value)
        }),
        ("conversations", "outcomes", |c| {
            (c.conversations().outcomes("v_1", &Query::new())).map(Returned::value)
        }),
        ("conversations", "record_outcome", |c| {
            (c.conversations()
                .record_outcome("v_1", &json!({ "outcome": "resolved" })))
            .map(Returned::value)
        }),
        ("conversations", "return_to_bot", |c| {
            (c.conversations().return_to_bot_id("v_1", "b_1")).map(Returned::value)
        }),
        ("conversations", "update_blocks", |c| {
            (c.conversations()
                .update_blocks("v_1", "m_1", &json!({ "patches": [] })))
            .map(Returned::value)
        }),
        ("conversations", "unread_counts", |c| {
            (c.conversations().unread_counts(&Query::new())).map(Returned::value)
        }),
        // -- customer_cards -----------------------------------------------
        ("customer_cards", "list", |c| {
            (c.customer_cards().list(&Query::new())).map(Returned::value)
        }),
        ("customer_cards", "create", |c| {
            (c.customer_cards().create(
                &json!({ "key": "orders", "title": "Orders", "url": "https://acme.test/cards" }),
            ))
            .map(Returned::value)
        }),
        ("customer_cards", "update", |c| {
            (c.customer_cards()
                .update("cc_1", &json!({ "title": "Recent orders" })))
            .map(Returned::value)
        }),
        ("customer_cards", "rotate_secret", |c| {
            (c.customer_cards().rotate_secret("cc_1")).map(Returned::value)
        }),
        ("customer_cards", "delete", |c| {
            (c.customer_cards().delete("cc_1")).map(Returned::value)
        }),
        // -- customer_card_endpoints --------------------------------------
        ("customer_card_endpoints", "list", |c| {
            (c.customer_card_endpoints().list(&Query::new())).map(Returned::value)
        }),
        ("customer_card_endpoints", "create", |c| {
            (c.customer_card_endpoints()
                .create(&json!({ "url": "https://acme.test/cards" })))
            .map(Returned::value)
        }),
        ("customer_card_endpoints", "rotate_secret", |c| {
            (c.customer_card_endpoints().rotate_secret("ce_1")).map(Returned::value)
        }),
        ("customer_card_endpoints", "activate_secret", |c| {
            (c.customer_card_endpoints().activate_secret("ce_1")).map(Returned::value)
        }),
        ("customer_card_endpoints", "delete", |c| {
            (c.customer_card_endpoints().delete("ce_1")).map(Returned::value)
        }),
        // -- docs_sources -------------------------------------------------
        ("docs_sources", "providers", |c| {
            (c.docs_sources().providers(&Query::new())).map(Returned::value)
        }),
        ("docs_sources", "list", |c| {
            (c.docs_sources().list(&Query::new())).map(Returned::value)
        }),
        ("docs_sources", "discover", |c| {
            (c.docs_sources()
                .discover(&json!({ "provider": "gitbook", "token": "t_1" })))
            .map(Returned::value)
        }),
        ("docs_sources", "create", |c| {
            (c.docs_sources().create(&json!({
                "provider": "gitbook",
                "token": "t_1",
                "organizationId": "o_1",
                "spaceId": "s_1"
            })))
            .map(Returned::value)
        }),
        ("docs_sources", "update", |c| {
            (c.docs_sources()
                .update("ds_1", &json!({ "status": "paused" })))
            .map(Returned::value)
        }),
        ("docs_sources", "sync", |c| {
            (c.docs_sources().sync_now("ds_1", true)).map(Returned::value)
        }),
        ("docs_sources", "webhook", |c| {
            (c.docs_sources().webhook("ds_1")).map(Returned::value)
        }),
        ("docs_sources", "delete", |c| {
            (c.docs_sources().delete("ds_1")).map(Returned::value)
        }),
        // -- email_templates ----------------------------------------------
        ("email_templates", "list", |c| {
            (c.email_templates().list(&Query::new())).map(Returned::value)
        }),
        ("email_templates", "preview", |c| {
            (c.email_templates().preview(&json!({
                "kind": "workspace_invite",
                "subject": "Join us",
                "body": "Hello"
            })))
            .map(Returned::value)
        }),
        ("email_templates", "update", |c| {
            (c.email_templates().update(
                "workspace_invite",
                &json!({ "subject": "Join us", "body": "Hello" }),
            ))
            .map(Returned::value)
        }),
        ("email_templates", "delete", |c| {
            (c.email_templates().delete("workspace_invite")).map(Returned::value)
        }),
        // -- knowledge_gaps -----------------------------------------------
        ("knowledge_gaps", "list", |c| {
            (c.knowledge_gaps().list(&Query::new())).map(Returned::value)
        }),
        ("knowledge_gaps", "update", |c| {
            (c.knowledge_gaps().set_status("kg_1", "dismissed")).map(Returned::value)
        }),
        ("knowledge_gaps", "draft", |c| {
            (c.knowledge_gaps().draft("kg_1")).map(Returned::value)
        }),
        // -- macros (added 2026-10-14) ------------------------------------
        ("macros", "get", |c| {
            (c.macros().get("m_1")).map(Returned::value)
        }),
        ("macros", "actions", |c| {
            (c.macros().actions(&Query::new())).map(Returned::value)
        }),
        ("macros", "templates", |c| {
            (c.macros().templates(&Query::new())).map(Returned::value)
        }),
        ("macros", "template", |c| {
            (c.macros().template("first-reply")).map(Returned::value)
        }),
        // -- reports (added 2026-10-14) -----------------------------------
        ("reports", "sources", |c| {
            (c.reports().sources(&Query::new())).map(Returned::value)
        }),
        ("reports", "attribution", |c| {
            (c.reports().attribution(&Query::new())).map(Returned::value)
        }),
        ("reports", "cohorts", |c| {
            (c.reports().cohorts(&Query::new())).map(Returned::value)
        }),
        ("reports", "response_times", |c| {
            (c.reports().response_times(&Query::new())).map(Returned::value)
        }),
        ("reports", "teams", |c| {
            (c.reports().teams(&Query::new())).map(Returned::value)
        }),
        ("reports", "labels", |c| {
            (c.reports().labels(&Query::new())).map(Returned::value)
        }),
        ("reports", "voice", |c| {
            (c.reports().voice(&Query::new())).map(Returned::value)
        }),
        ("reports", "themes", |c| {
            (c.reports().themes(&Query::new())).map(Returned::value)
        }),
        ("reports", "theme_conversations", |c| {
            (c.reports().theme_conversations("th_1", &Query::new())).map(Returned::value)
        }),
        // -- tickets (added 2026-10-14) -----------------------------------
        ("tickets", "conversations", |c| {
            (c.tickets().conversations("t_1", &Query::new())).map(Returned::value)
        }),
        ("tickets", "status_history", |c| {
            (c.tickets().status_history("t_1", &Query::new())).map(Returned::value)
        }),
        ("tickets", "lineage", |c| {
            (c.tickets().lineage("t_1", &Query::new())).map(Returned::value)
        }),
        ("tickets", "open_count", |c| {
            (c.tickets().open_count("v_1")).map(Returned::value)
        }),
        ("tickets", "time", |c| {
            (c.tickets().time("t_1", &Query::new())).map(Returned::value)
        }),
        // -- triggers -----------------------------------------------------
        ("triggers", "list", |c| {
            (c.triggers().list(&Query::new())).map(Returned::page)
        }),
        ("triggers", "create", |c| {
            (c.triggers().create(&json!({
                "name": "welcome",
                "eventType": "conversation.created",
                "enabled": true,
                "conditions": [],
                "actions": [{ "type": "send_message", "params": {} }]
            })))
            .map(Returned::value)
        }),
        ("triggers", "delete", |c| {
            (c.triggers().delete("tg_1")).map(Returned::value)
        }),
        // -- webhooks (added 2026-10-14) ----------------------------------
        ("webhooks", "list", |c| {
            (c.webhooks().list(&Query::new())).map(Returned::value)
        }),
        ("webhooks", "events", |c| {
            (c.webhooks().events(&Query::new())).map(Returned::value)
        }),
        ("webhooks", "create", |c| {
            (c.webhooks().create(&json!({
                "url": "https://acme.test/hook",
                "eventTypes": ["conversation.created"]
            })))
            .map(Returned::value)
        }),
        ("webhooks", "schema_versions", |c| {
            (c.webhooks().schema_versions(&Query::new())).map(Returned::value)
        }),
        ("webhooks", "set_schema_version", |c| {
            (c.webhooks()
                .set_schema_version("wh_1", &json!({ "schemaVersion": "2026-01-01" })))
            .map(Returned::value)
        }),
        ("webhooks", "delete", |c| {
            (c.webhooks().delete("wh_1")).map(Returned::value)
        }),
        // -- workspace (added 2026-10-14) ---------------------------------
        ("workspace", "key_scopes", |c| {
            (c.workspace().key_scopes(&Query::new())).map(Returned::value)
        }),
        ("workspace", "secrets", |c| {
            (c.workspace().secrets(&Query::new())).map(Returned::value)
        }),
        ("workspace", "mine", |c| {
            (c.workspace().mine(&Query::new())).map(Returned::value)
        }),
    ]
}
