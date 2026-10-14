//! More conversation routes — a second `impl` block on
//! [`Conversations`](crate::Conversations).
//!
//! Split out of `conversations.rs` for the contract's 650-line ceiling (§9),
//! and it mirrors the API's own arrangement: `linked` apart, these six routes
//! live on five separate controllers under the `/v1/conversations` prefix for
//! exactly the same reason — `conversations.controller.ts` reached 633 of 650
//! lines, and a file that is full is one where the next feature displaces
//! something else.
//!
//! ## One scope trap covers three of these
//!
//! [`record_outcome`](Conversations::record_outcome),
//! [`outcomes`](Conversations::outcomes) and
//! [`update_blocks`](Conversations::update_blocks) each declare one
//! permission — `conversations:write`, `conversations:read` and
//! `messages:write` — and each then narrows its query by the caller's
//! *conversation read scope*. That scope is `conversations:read`, or
//! `conversations:read:assigned`, and **nothing else**: a key holding
//! neither resolves to the closed-door sentinel, every conversation lookup
//! matches no rows, and you get **404 on a conversation that plainly
//! exists** rather than the 403 the missing scope deserves.
//!
//! So a key for these routes wants `conversations:read` *in addition to* the
//! scope the route advertises. `conversations:read:assigned` is no use to a
//! key either — it narrows to the caller's own `userId`, and a key is not a
//! person who gets assigned threads. If a conversation you can read through
//! [`get`](Conversations::get) 404s here, the scope list is the first thing
//! to check, not the id.

use serde::Serialize;
use serde_json::Value;

use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};
use crate::resources::Conversations;

impl Conversations<'_> {
    /// `GET /v1/conversations/{id}/linked` — the linked and merged threads
    /// for this one.
    ///
    /// `{ "items": [ … ] }` — an object with an `items` array and **no
    /// cursor**, so it is not a page and there is nothing to walk. A thread
    /// has a handful of these at most.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if the conversation is
    /// not visible to this credential.
    pub fn linked(&self, id: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client().get(
            &format!("/v1/conversations/{}/linked", encode_path_segment(id)),
            query,
        )
    }

    /// `GET /v1/conversations/{id}/outcomes` — what is already on record for
    /// this conversation.
    ///
    /// `{ conversationId, items }`, merged from three tables —
    /// `ai_resolutions`, `conversation_escalations` and
    /// `conversation.purchased` events — newest first, each arm capped at 20
    /// so a pathological thread is bounded rather than slow. An object, not
    /// a page.
    ///
    /// Each row's `source` is the field that earns the route: it tells your
    /// own `agent_report` apart from the visitor's `visitor_chip` and from an
    /// escalation a human asked for, which is what makes a number in the
    /// funnel auditable.
    ///
    /// See the module docs above: this needs `conversations:read` and a key
    /// without it gets a 404 rather than a 403.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if `id` is not a
    /// UUID v7.
    pub fn outcomes(&self, id: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client().get(
            &format!("/v1/conversations/{}/outcomes", encode_path_segment(id)),
            query,
        )
    }

    /// `POST /v1/conversations/{id}/outcomes` — report what happened:
    /// resolved, escalated or purchased.
    ///
    /// The write that makes the AI funnels count your agent's work.
    ///
    /// **`created: false` is a success and means nothing changed** — an
    /// equal outcome was already on record: a replayed resolution inside 24
    /// hours, an escalation inside 60 seconds, or a purchase whose
    /// `outcomeRef` we already hold. Treat it as idempotency working, not as
    /// a refusal.
    ///
    /// `valueCents` does **not** feed a revenue report. Chatly has no
    /// revenue report; the response's `feeds` field says in words what the
    /// outcome you sent actually populates, which is there because an
    /// integrator cannot see the dashboard on the other end.
    ///
    /// Answers **201**. Needs `conversations:write` *and*, in practice,
    /// `conversations:read` — see the module docs.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] for a conversation
    /// outside the credential's read scope.
    pub fn record_outcome(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client().write(
            "POST",
            &format!("/v1/conversations/{}/outcomes", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `POST /v1/conversations/{id}/return-to-bot` — hand an open thread
    /// back to an assistant after a human took it over.
    ///
    /// Body: `{ "botId": "<uuid>" }`, `.strict()`, and **`botId` is required
    /// rather than defaulting to whoever owned it before**. The previous
    /// owner is recoverable, but a thread three assistants have touched has
    /// no obvious "the" bot, and guessing wrong means the visitor talks to
    /// the returns agent about a size exchange. [`crate::Bots::list`] lists
    /// the choices.
    ///
    /// Refused on a **pending, snoozed, closed or merged** thread, and on a
    /// bot that is not `active`. Needs `conversations:assign` — a higher bar
    /// than replying, because this moves ownership to somebody else, which
    /// is the one thing
    /// [`set_agent_handling`](Conversations::set_agent_handling) cannot do.
    ///
    /// The internal note it leaves is authored by `api` for a key rather
    /// than by an invented person.
    ///
    /// Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Conflict`] for a thread or bot in
    /// the wrong state.
    pub fn return_to_bot(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client().write(
            "POST",
            &format!(
                "/v1/conversations/{}/return-to-bot",
                encode_path_segment(id)
            ),
            &Query::new(),
            Some(body),
        )
    }

    /// [`Conversations::return_to_bot`] with the one-field body built for
    /// you.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn return_to_bot_id(&self, id: &str, bot_id: &str) -> Result<Value, ChatlyError> {
        self.return_to_bot(id, &serde_json::json!({ "botId": bot_id }))
    }

    /// `PATCH /v1/conversations/{id}/messages/{message_id}/blocks` — edit
    /// the rich blocks on a message **already on the visitor's screen**.
    ///
    /// Body: `{ "patches": [ … ] }`, 1 to 50 — one agent turn's worth of
    /// revisions, not a bulk rewrite. Blocks are addressable by `block.id`
    /// after the fact, which is what lets an agent stream option cards and
    /// then fill in their reasons, or stop a `status` block spinning.
    ///
    /// Two failure modes worth knowing: **an unknown block id is a 404**
    /// rather than a no-op success, and **an invalid merged result refuses
    /// the whole batch** — a half-applied patch would leave a shape no
    /// author ever wrote.
    ///
    /// Needs `messages:write` *and*, in practice, `conversations:read` — see
    /// the module docs. `messages:write` alone would also let a
    /// scope-narrowed member reach into any thread in the workspace, which
    /// is why the extra check exists at all.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] for an unknown block
    /// id or an out-of-scope conversation.
    pub fn update_blocks(
        &self,
        id: &str,
        message_id: &str,
        body: &impl Serialize,
    ) -> Result<Value, ChatlyError> {
        self.client().write(
            "PATCH",
            &format!(
                "/v1/conversations/{}/messages/{}/blocks",
                encode_path_segment(id),
                encode_path_segment(message_id)
            ),
            &Query::new(),
            Some(body),
        )
    }

    /// `GET /v1/conversations/unread-counts` — the inbox badge numbers.
    ///
    /// **Near-useless from an API key, and it answers 200 anyway.** This is
    /// *per-user* state: the handler reads `membership.userId`, which for a
    /// `ck_` key is the key's own id, and a key is never assigned a thread,
    /// never mentioned and never a participant. So expect zeros. It is
    /// implemented rather than omitted because it does not fail — it tells
    /// you the truth about a principal that happens to have no unread
    /// anything — and a method that is absent looks like an oversight,
    /// whereas this docblock is the warning. For workspace-wide "what needs
    /// attention", use
    /// [`unattended`](Conversations::unattended) instead.
    ///
    /// One object: four scalars plus three bucket arrays. **`mentions`,
    /// `participating` and `unattended` are subsets of `unread`**, so adding
    /// them double-counts. `byLabel` is capped at 50 buckets ordered by
    /// count. Not a page and not a bare array.
    ///
    /// Takes an optional `brand`.
    ///
    /// The path is a static segment that wins against
    /// `GET /v1/conversations/{id}` — Fastify's router resolves static
    /// before parametric regardless of registration order — so it is not at
    /// risk of being read as a conversation id.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn unread_counts(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client().get("/v1/conversations/unread-counts", query)
    }
}
