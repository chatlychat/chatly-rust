//! Per-route response shapes — the hand-written column, and nothing else.
//!
//! Split out of `tests/common/mod.rs` so neither file approaches the
//! contract's 650-line ceiling (§9), and the seam is the obvious one: that
//! file is machinery (a fake transport, a client builder, the manifest
//! loader) and this one is data. The other three SDKs that finished this
//! surface split the same way, for the same reason.
//!
//! ═══ WHY THERE IS A TABLE HERE AT ALL ═══════════════════════════════════
//!
//! A fake transport that answers every route with `{"items":[],"nextCursor":
//! null}` agrees with any mistake about which routes are paginated. That is
//! not hypothetical: the shared manifest briefly marked `GET /v1/macros`
//! `paged: true`, this SDK's `macros().list` deserialised into `Page`, and
//! real callers got `ChatlyError::Decode` while the suite stayed green —
//! because the page envelope the fake returned was the one shape the route
//! never sends. The .NET SDK had the identical pair of bugs.
//!
//! So the fake answers each route the shape that route really returns, and
//! `tests/surface_lock.rs` cross-checks this table against the manifest's
//! `paged` flags in both directions.

#![allow(dead_code)]

/// The top-level JSON shape a route answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyShape {
    /// `{ "items": [...], "nextCursor": … }` — a cursor page. Several routes
    /// add siblings (`hasMore`, `total`, `tier`/`facets`); those are the same
    /// shape as far as a client is concerned.
    Page,
    /// A bare top-level JSON array. Thirty-three routes do this.
    Array,
    /// A single top-level JSON object that is not a page.
    Object,
    /// `204 No Content`, which this crate decodes to `Value::Null`.
    Empty,
}

impl BodyShape {
    /// The HTTP status to answer with.
    pub fn status(self) -> u16 {
        match self {
            Self::Empty => 204,
            _ => 200,
        }
    }

    /// A body of this shape.
    ///
    /// **Non-empty on purpose.** The empty forms are not distinguishable by
    /// a lenient deserialiser — `[]`, `{}` and
    /// `{"items":[],"nextCursor":null}` were all accepted by a derived
    /// `Page`, so a fake that answered `[]` could not tell a correct
    /// `macros().list` from one that still deserialised into a `Page`. Each
    /// of these carries a row, so the shapes are actually distinct.
    pub fn body(self) -> &'static str {
        match self {
            Self::Page => r#"{"items":[{"id":"row_1"}],"nextCursor":null}"#,
            Self::Array => r#"[{"id":"row_1"}]"#,
            Self::Object => r#"{"id":"row_1"}"#,
            Self::Empty => "",
        }
    }

    /// The name this shape has in `surface.json` and in every lock file.
    ///
    /// The lock's `shape` column is this string, so the four spellings are
    /// cross-language wire values and not local cosmetics.
    pub fn name(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Array => "array",
            Self::Object => "object",
            Self::Empty => "empty",
        }
    }
}

/// What `ns.name` answers with, for all 163 manifest methods.
///
/// ═══ READ THIS BEFORE CHANGING A VALUE ══════════════════════════════════
///
/// Hand-written, from each handler's `@ZodResponse(...)` schema — or from the
/// service return type it forwards where the handler has no such decorator.
/// **Deliberately NOT read from `surface.json`**, even though this module's
/// sibling loads that manifest for paths and `paged` flags. The 104 routes
/// added on 2026-10-14 were each read off the handler before they were
/// entered here; where the schema lives in a `*-response-schemas.ts` beside
/// the controller, that file was opened too, because `z.array(...)` versus
/// `z.object({ items })` is the whole question and the controller does not
/// show it. The three added later the same day — `root.me`,
/// `webhooks.delete` and `workspace.mine` — were read the same way.
///
/// A fixture derived from the manifest agrees with whatever the manifest
/// says, including when it is wrong, and that is not hypothetical:
/// `GET /v1/macros` was marked `paged: true` for a day while its handler is
/// `@PaginationExempt` and answers a bare array. This crate's `macros().list`
/// deserialised that into a `Page`; worse than the .NET client's decode
/// error, a derived `Page` read the bare array as an **empty page** and
/// handed it back with a 200 — a wrong answer rather than a missing one. The
/// suite stayed green because the fake answered every route with
/// `{ items, nextCursor }`, the one shape that route never sends.
///
/// So this table is a second opinion. `tests/surface_lock.rs` feeds each
/// method the shape declared HERE, asserts the value it hands back really
/// has that shape, and writes this column into `surface.lock.json`;
/// `scripts/check-sdk-surface-parity.mjs` then diffs eight independently
/// written columns against the manifest. A wrong manifest row needs eight
/// authors to misread the same handler.
///
/// Returns `None` for anything not listed, and `tests/surface_lock.rs`
/// treats that as a failure — there is deliberately **no catch-all arm**, so
/// a method added to the manifest cannot quietly inherit a default shape.
/// That matters because the default used to be `Page`, for every route.
///
/// Five things in here are worth knowing before you trust a shape:
///
/// - **Thirty-three routes return a bare array**, not an envelope. All of them
///   are `@PaginationExempt`, so there is no cursor to ask for more. One of
///   them is a POST (`docs_sources.discover`) and one is a PUT
///   (`ai.set_tone`), so the verb is no guide.
/// - **Several routes answer an object that CARRIES an `items` array** and no
///   cursor — `conversations.linked`, `conversations.outcomes`,
///   `tickets.time`, `macros.templates`, `workspace.secrets`,
///   `billing.usage_monthly`. They are not pages, and the only thing
///   distinguishing them on the wire is the absence of `nextCursor`, which
///   is why `Returned::shape` requires BOTH keys before it says `Page`.
/// - **`billing.invoices` really is cursor-paginated and is still `object`**,
///   because its envelope key is `data`. The decoder cannot read it as a
///   page, so no walker is offered.
/// - **Several rows look wrong and are not.** `contacts.remove_tag` is a
///   DELETE answering with the surviving contact, and
///   `conversations.cancel_scheduled` a DELETE answering with the cancelled
///   row; four of the twelve report routes are bare arrays.
/// - **`macros.delete` is the only 204.** Seven other DELETEs on this
///   surface answer 200 with a body.
pub fn shape_for(ns: &str, name: &str) -> Option<BodyShape> {
    use BodyShape::{Array, Empty, Object, Page};
    let shape = match (ns, name) {
        ("root", "whoami") => Object,
        // A discriminated union on `principal`; for a key, the same object
        // `whoami` returns.
        ("root", "me") => Object,

        ("ai", "add_provider") => Object,
        ("ai", "create_tool") => Object,
        // `{ deleted: true }` with a 200, not a 204.
        ("ai", "delete_tool") => Object,
        ("ai", "generate_tone") => Object,
        // `{ providers: [...] }`, not a bare array — unlike `ai.tools`.
        ("ai", "providers") => Object,
        ("ai", "quota") => Object,
        // A PUT answering the bare array the GET returns, not the
        // `{ rules }` envelope it was sent.
        ("ai", "set_tone") => Array,
        ("ai", "test_provider") => Object,
        // z.array(toneRuleResponseSchema) — a bare array.
        ("ai", "tone") => Array,
        ("ai", "tone_preview") => Object,
        ("ai", "tools") => Array,
        ("ai", "update_tool") => Object,

        // Cursor-paginated, and still an OBJECT: its envelope key is
        // `data`, not `items`, so it is not the page shape.
        ("billing", "invoices") => Object,
        ("billing", "plans") => Array,
        ("billing", "summary") => Object,
        ("billing", "usage") => Object,
        ("billing", "usage_daily") => Object,
        // `items` plus a `total`, and no cursor.
        ("billing", "usage_monthly") => Object,

        ("bot_routes", "create") => Object,
        ("bot_routes", "delete") => Object,
        ("bot_routes", "fields") => Object,
        ("bot_routes", "list") => Array,
        ("bot_routes", "resolve") => Object,
        ("bot_routes", "update") => Object,

        ("bots", "analytics") => Object,
        ("bots", "create") => Object,
        // The second of the three pages the manifest once missed.
        ("bots", "deliveries") => Page,
        ("bots", "delivery") => Object,
        ("bots", "get") => Object,
        // buildPaginatedResponse — `items`, `nextCursor`, `hasMore`.
        ("bots", "list") => Page,
        ("bots", "redeliver") => Object,
        ("bots", "rotate_external_secret") => Object,
        ("bots", "test_delivery") => Object,
        ("bots", "update") => Object,
        ("bots", "update_external") => Object,

        ("brands", "create") => Object,
        ("brands", "delete") => Object,
        ("brands", "list") => Array,
        ("brands", "update") => Object,

        ("canvas_steps", "get") => Object,
        ("canvas_steps", "resolve") => Object,

        // z.array(z.string()) — the only array of plain strings here.
        ("channels", "adapters") => Array,
        ("channels", "create") => Object,
        ("channels", "get") => Object,
        ("channels", "list") => Array,
        ("channels", "rotate_secret") => Object,
        ("channels", "update") => Object,

        ("contacts", "add_tags") => Object,
        ("contacts", "bulk_upsert") => Object,
        ("contacts", "cards") => Object,
        ("contacts", "get") => Object,
        ("contacts", "journey") => Page,
        ("contacts", "list") => Page,
        ("contacts", "merge") => Object,
        ("contacts", "mood") => Object,
        // Answers the surviving contact, not a 204.
        ("contacts", "remove_tag") => Object,
        ("contacts", "unsubscribe") => Object,
        ("contacts", "upsert") => Object,

        ("conversations", "agent_handling") => Object,
        // Answers the scheduled message as it ended up, not a 204.
        ("conversations", "cancel_scheduled") => Object,
        ("conversations", "generate_reply") => Object,
        ("conversations", "generated_replies") => Array,
        ("conversations", "get") => Object,
        // `{ items }` with NO cursor — an object, not a page.
        ("conversations", "linked") => Object,
        ("conversations", "list") => Page,
        ("conversations", "messages") => Page,
        ("conversations", "outcomes") => Object,
        ("conversations", "record_outcome") => Object,
        ("conversations", "return_to_bot") => Object,
        ("conversations", "schedule_message") => Object,
        ("conversations", "scheduled_messages") => Array,
        ("conversations", "send") => Object,
        ("conversations", "set_agent_handling") => Object,
        // Page plus a `total` that is not `items.len()`.
        ("conversations", "unattended") => Page,
        // Four scalars plus three bucket arrays.
        ("conversations", "unread_counts") => Object,
        ("conversations", "update_blocks") => Object,

        ("customer_card_endpoints", "activate_secret") => Object,
        ("customer_card_endpoints", "create") => Object,
        ("customer_card_endpoints", "delete") => Object,
        ("customer_card_endpoints", "list") => Array,
        ("customer_card_endpoints", "rotate_secret") => Object,

        ("customer_cards", "create") => Object,
        ("customer_cards", "delete") => Object,
        ("customer_cards", "list") => Array,
        ("customer_cards", "rotate_secret") => Object,
        ("customer_cards", "update") => Object,

        ("docs_sources", "create") => Object,
        ("docs_sources", "delete") => Object,
        // A POST that answers a BARE ARRAY.
        ("docs_sources", "discover") => Array,
        ("docs_sources", "list") => Array,
        ("docs_sources", "providers") => Array,
        ("docs_sources", "sync") => Object,
        ("docs_sources", "update") => Object,
        ("docs_sources", "webhook") => Object,

        ("email_templates", "delete") => Object,
        ("email_templates", "list") => Array,
        ("email_templates", "preview") => Object,
        ("email_templates", "update") => Object,

        ("events", "list") => Page,
        ("events", "track") => Object,

        ("kb", "article") => Object,
        ("kb", "articles") => Page,
        ("kb", "collections") => Array,

        ("knowledge_gaps", "draft") => Object,
        ("knowledge_gaps", "list") => Array,
        ("knowledge_gaps", "update") => Object,

        // `{ limit, actions }` — the envelope carries the ceiling.
        ("macros", "actions") => Object,
        ("macros", "create") => Object,
        // The ONLY real 204 on the surface.
        ("macros", "delete") => Empty,
        ("macros", "get") => Object,
        // The bare array this whole table exists for.
        ("macros", "list") => Array,
        ("macros", "template") => Object,
        // `{ items, total }` over a frozen catalogue — no cursor.
        ("macros", "templates") => Object,
        ("macros", "update") => Object,

        // Two of the report routes are bare arrays, and these are
        ("reports", "agents") => Array,
        ("reports", "attribution") => Array,
        ("reports", "cohorts") => Object,
        // the other one — plus `sources` and `attribution` below.
        ("reports", "conversations_by_day") => Array,
        ("reports", "labels") => Object,
        ("reports", "overview") => Object,
        ("reports", "response_times") => Object,
        ("reports", "sla") => Object,
        ("reports", "sources") => Array,
        ("reports", "teams") => Object,
        ("reports", "theme_conversations") => Object,
        ("reports", "themes") => Object,
        ("reports", "voice") => Object,

        // Page plus `tier` and `facets`.
        ("search", "query") => Page,

        ("tickets", "bulk") => Object,
        ("tickets", "comments") => Array,
        ("tickets", "conversations") => Array,
        ("tickets", "for_conversation") => Array,
        ("tickets", "lineage") => Object,
        // Page plus `hasMore`.
        ("tickets", "list") => Page,
        ("tickets", "merge") => Object,
        ("tickets", "open_count") => Object,
        ("tickets", "queues") => Array,
        ("tickets", "split") => Object,
        ("tickets", "status_history") => Object,
        // `{ items, totalMinutes, billableMinutes }` — no cursor.
        ("tickets", "time") => Object,

        ("triggers", "create") => Object,
        ("triggers", "delete") => Object,
        // The third page the manifest once missed.
        ("triggers", "list") => Page,

        // `{ conversationId }`, read off the service return type —
        // `VisitorsService.startProactiveConversation(): Promise<{
        // conversationId: string }>` — because this handler carries no
        // `@ZodResponse` at all. An object, and emphatically not a page:
        // a derived `Page` here would read a one-key object as an empty
        // page and hand it back with a 200, which is this crate's own
        // historical bug rather than a hypothetical one.
        ("visitors", "start_conversation") => Object,

        ("webhooks", "create") => Object,
        ("webhooks", "deliveries") => Page,
        ("webhooks", "delivery") => Object,
        // `{ eventTypes: [...] }`, not a bare array.
        ("webhooks", "events") => Object,
        ("webhooks", "list") => Array,
        ("webhooks", "redeliver") => Object,
        ("webhooks", "schema_versions") => Object,
        ("webhooks", "set_schema_version") => Object,
        // `{ ok: true }` with a 200, like the other six non-204 DELETEs.
        ("webhooks", "delete") => Object,

        // `{ "skills": [...] }`, not a bare array.
        ("workspace", "agent_skills") => Object,
        ("workspace", "inboxes") => Array,
        // z.array({ value, group }) — a bare array.
        ("workspace", "key_scopes") => Array,
        // z.array(workspaceListItemSchema) — exactly one entry for a key.
        ("workspace", "mine") => Array,
        ("workspace", "labels") => Array,
        // `items` plus `total` and `truncated`, and no cursor.
        ("workspace", "secrets") => Object,
        ("workspace", "settings") => Object,
        ("workspace", "team_members") => Array,
        _ => return None,
    };
    Some(shape)
}
