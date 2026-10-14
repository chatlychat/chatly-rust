//! Triggers — an event, some conditions, and one or more actions.
//!
//! The part of a workspace an integrator most wants to manage from code,
//! because it is the part that changes as *their* product changes; for a long
//! time the dashboard was the only door. Opening it to API keys does not open
//! an arbitrary side effect: the action `type` is a closed catalogue, shared
//! with the workflow builder and checked against it in CI, so a key can
//! configure exactly the set an agent could configure by hand.
//!
//! A trigger is not a workflow. A trigger fires one flat list of actions on
//! an event; a workflow is a DAG the worker walks. `enqueue_workflow` is the
//! action that bridges them.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::{encode_path_segment, Query};

/// The `op` values a trigger condition may use.
///
/// The same seven as a bot route's, deliberately: the two evaluators share
/// their condition vocabulary.
pub const TRIGGER_CONDITION_OPS: [&str; 7] = ["eq", "neq", "in", "gt", "lt", "contains", "matches"];

/// The `triggers` namespace. Get one from [`ChatlyClient::triggers`].
#[derive(Debug, Clone, Copy)]
pub struct Triggers<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Triggers<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/triggers` — one page of triggers.
    ///
    /// A real cursor page. Rows are `.partial()` because `?fields=`
    /// projects, so treat every field as optional rather than assuming the
    /// shape [`Triggers::upsert`] returns.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page("/v1/triggers", query)
    }

    /// `GET /v1/triggers` — every page, as an iterator.
    pub fn iter(&self, query: &Query) -> PageIter<Value> {
        self.client.paginate("/v1/triggers", query)
    }

    /// `POST /v1/triggers` — create **or update** a trigger.
    ///
    /// An upsert keyed on an optional `id` in the body, not two routes:
    /// include `id` and you replace that trigger, omit it and you create
    /// one. There is no PATCH, so **an update is a whole replacement** —
    /// read the trigger first if you only mean to change one condition.
    ///
    /// Required: `name` (1–160), `eventType` (1–64), `enabled`,
    /// `conditions` (≤50, each `{ field, op, value }` — see
    /// [`TRIGGER_CONDITION_OPS`]) and `actions` (**1–20**, each
    /// `{ type, params }`; an empty list is a 400, because a trigger with no
    /// action is a rule that fires and does nothing). `priority` defaults to
    /// 0.
    ///
    /// `debounceSeconds` (up to 7 days) and `debounceKeyTemplate` are the
    /// rate limit: without a template the key defaults to
    /// `contact:<id>` or `conversation:<id>` at evaluation time, which is
    /// usually what you want and is why the field is nullable rather than
    /// required.
    ///
    /// **`eventType` is a free string on this route**, validated only for
    /// length. It is matched by literal string equality against the
    /// publisher's name, so a typo creates a trigger that is syntactically
    /// perfect and never fires. The offered vocabulary is the workflow
    /// builder's trigger catalogue.
    ///
    /// Answers **201 on both arms** — an update is a 201 too, because Nest's
    /// POST default applies to the update branch as well.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an empty
    /// `actions`, an action `type` outside the catalogue, or an `id` that is
    /// not a UUID v7.
    pub fn upsert(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/triggers", &Query::new(), Some(body))
    }

    /// Named for `surface.json`'s `triggers.create`, and the same route as
    /// [`Triggers::upsert`].
    ///
    /// Kept as a distinct name because the manifest calls it `create` and
    /// the parity gate asserts the method list; omit `id` from the body and
    /// it is a create.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.upsert(body)
    }

    /// `DELETE /v1/triggers/{id}` — answers `{ "ok": true }` with a 200, not
    /// a 204.
    ///
    /// Until this route existed there was no way to delete a trigger through
    /// any surface at all, so one created by mistake fired forever.
    ///
    /// The id is validated as a UUID v7 **before** the query, so a
    /// malformed id is a 400 rather than a 404 — and rather than a Postgres
    /// cast error, which 500s.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a malformed id,
    /// [`ChatlyError::NotFound`] if no such trigger.
    pub fn delete(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/triggers/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_condition_operators_match_the_bot_route_set() {
        assert_eq!(TRIGGER_CONDITION_OPS.len(), 7);
        assert_eq!(
            TRIGGER_CONDITION_OPS,
            crate::resources::ROUTE_CONDITION_OPS,
            "the two evaluators share one condition vocabulary"
        );
    }
}
