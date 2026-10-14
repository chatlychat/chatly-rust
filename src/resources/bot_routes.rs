//! Bot routing rules — which bot answers, and when.
//!
//! An ordered table, first match wins. The evaluator is pure and shared with
//! the worker, which is what makes [`BotRoutes::resolve`] worth having: it
//! runs the real decision over a hypothetical visitor and reports why each
//! route did or did not win.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `op` values a routing condition may use.
///
/// Seven. [`BotRoutes::fields`] returns the authoritative catalogue with the
/// fields each operator is valid on; this constant is for a picker.
pub const ROUTE_CONDITION_OPS: [&str; 7] = ["eq", "neq", "in", "gt", "lt", "contains", "matches"];

/// The `bot_routes` namespace. Get one from [`ChatlyClient::bot_routes`].
#[derive(Debug, Clone, Copy)]
pub struct BotRoutes<'a> {
    client: &'a ChatlyClient,
}

impl<'a> BotRoutes<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/bot-routes` — every rule, as a **bare JSON array** in
    /// evaluation order.
    ///
    /// The order *is* the data: routing takes the first match, so the
    /// position of a row is as meaningful as its conditions. Do not re-sort
    /// the array client-side. Pagination-exempt — a workspace keeps a
    /// handful — so there is no cursor and no iterator.
    ///
    /// This is also the only route that shows `enabled`: a disabled rule is
    /// here and is deliberately absent from [`BotRoutes::resolve`].
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/bot-routes", query)
    }

    /// `GET /v1/bot-routes/fields` — the condition vocabulary, with types.
    ///
    /// An object, not an array. Static and tenant-free: it is the set of
    /// field paths a condition may be keyed on and the operators each
    /// accepts. Worth reading before building a rule, because a field path
    /// that resolves to nothing is the commonest routing bug and the API
    /// cannot refuse one — a condition on a path no event carries simply
    /// never matches.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn fields(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/bot-routes/fields", query)
    }

    /// `POST /v1/bot-routes` — add a rule.
    ///
    /// `botId` and `name` (1–160 chars) are required. `priority` (0–10,000),
    /// `enabled`, `channelTypes` (≤20), `conditions` (≤20, each
    /// `{ field, op, value }` — see [`ROUTE_CONDITION_OPS`]), `brandId` and
    /// `evaluate` are optional.
    ///
    /// Omitting `evaluate` is not the same as sending its default: the column
    /// defaults to `conversation_start`, so the *database* answers rather
    /// than the request schema, and the two can never disagree.
    ///
    /// Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if the body fails
    /// the API's schema.
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/bot-routes", &Query::new(), Some(body))
    }

    /// `PATCH /v1/bot-routes/{id}` — edit a rule.
    ///
    /// Every field of the create body is optional here, `botId` included —
    /// so a patch can point an existing rule at a different bot.
    ///
    /// `conditions` is **replaced wholesale**, not merged: send the array you
    /// want to end up with, which means reading the rule first if you only
    /// mean to add one condition.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such rule.
    pub fn update(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/bot-routes/{}", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `DELETE /v1/bot-routes/{id}` — answers `{ "deleted": true }` with a
    /// 200, not a 204.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such rule.
    pub fn delete(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/bot-routes/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }

    /// `POST /v1/bot-routes/resolve` — dry-run the table against a
    /// hypothetical visitor.
    ///
    /// A read dressed as a POST: the body carries the fake event — `body`,
    /// `channelType`, `brandId`, `channelPublicId`, `locale`, `contact`
    /// (`attributes`, `identityVerified`), `context` (`verified`,
    /// `unverified`) and `page` (`url` and/or `path`) — and **nothing is
    /// written**, which is why no idempotency key is demanded.
    ///
    /// Three things about the answer are worth knowing:
    ///
    /// - `selected` is nullable. No match is a valid outcome, not an error.
    /// - **Disabled rules are absent**, not reported with a verdict. The
    ///   production query filters on `enabled` before the evaluator sees a
    ///   row, so including them would invent a verdict the real decision
    ///   never computes. Use [`BotRoutes::list`] to see the flag.
    /// - `event` is the object the conditions were evaluated against, and it
    ///   is the point of the route: half of every routing bug is a field path
    ///   that resolves to nothing, and this is the only way to see that.
    ///
    /// `page.url` and `page.path` fill each other in — give either. An
    /// unparseable `url` yields a null path rather than a 400, so a
    /// malformed URL does not masquerade as a routing problem.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if the body fails
    /// the API's schema.
    pub fn resolve(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/bot-routes/resolve", &Query::new(), Some(body))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_condition_operators_match_the_evaluators_union() {
        assert_eq!(ROUTE_CONDITION_OPS.len(), 7);
        assert!(ROUTE_CONDITION_OPS.contains(&"matches"));
        // Not operators this evaluator has; `contains` covers the first.
        assert!(!ROUTE_CONDITION_OPS.contains(&"starts_with"));
        assert!(!ROUTE_CONDITION_OPS.contains(&"gte"));
    }
}
