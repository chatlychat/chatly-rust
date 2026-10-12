//! Billing — the plan catalogue and this workspace's metered usage.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::Query;

/// The `eventType` values `GET /v1/billing/usage` meters.
///
/// The server validates against its own copy of this list and is the
/// authority; this constant is here so you can populate a picker or a loop
/// without reading the API reference. If the API adds a type, the API will
/// accept it before this constant mentions it — so do not use this to
/// reject input, only to offer choices.
pub const USAGE_EVENT_TYPES: [&str; 4] = [
    "conversation.opened",
    "message.sent.agent",
    "message.sent.bot",
    "ai.tokens",
];

/// The `billing` namespace. Get one from [`ChatlyClient::billing`].
///
/// Read-only. Changing a subscription is not key-accessible: it needs a
/// signed-in person with billing permission, so there is deliberately no
/// `subscribe` or `cancel` here.
#[derive(Debug, Clone, Copy)]
pub struct Billing<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Billing<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/billing/plans` — the plan catalogue, as a **JSON array**.
    ///
    /// An empty array is a success, not an error: it is what a deployment
    /// with no Stripe configuration returns.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn plans(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/billing/plans", query)
    }

    /// `GET /v1/billing/usage` — metered usage for **one** event type.
    ///
    /// `event_type` is required, and that is why it is an argument rather
    /// than something you put in `query`: the handler validates it against
    /// a closed list and throws before doing anything else, so a call
    /// without it is a 400 — never an "all event types" rollup. Four SDKs
    /// shipped it as optional and every call that omitted it failed.
    /// [`USAGE_EVENT_TYPES`] lists the values the server currently accepts.
    ///
    /// `query` carries the window: `from` and `to`, both optional ISO
    /// timestamps. `from` defaults to the first of the current UTC month and
    /// `to` to one UTC month after `from`, exclusive. `to` must be strictly
    /// greater than `from`. [`Billing::usage_range`] sets both.
    ///
    /// Answers a single object — `workspaceId`, `eventType`, `from`, `to`,
    /// `total`, `count` — not a list, so there is nothing to paginate.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`], plus [`ChatlyError::Config`] if `event_type` is
    /// empty — refused here rather than sent, since an empty value cannot be
    /// anything but a mistake. An unrecognised but non-empty value is sent
    /// and comes back as [`ChatlyError::Validation`] whose message lists
    /// what the server will accept, which is more current than this crate.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), chatly::ChatlyError> {
    /// # let chatly = chatly::ChatlyClient::new("ck_x")?;
    /// use chatly::Query;
    /// let tokens = chatly.billing().usage("ai.tokens", &Query::new())?;
    /// println!("{}", tokens["total"]);
    /// # Ok(()) }
    /// ```
    pub fn usage(&self, event_type: &str, query: &Query) -> Result<Value, ChatlyError> {
        if event_type.trim().is_empty() {
            return Err(ChatlyError::Config(format!(
                "billing usage: event_type is empty; expected one of {USAGE_EVENT_TYPES:?}"
            )));
        }
        self.client.get(
            "/v1/billing/usage",
            &query.clone().set("eventType", event_type),
        )
    }

    /// [`Billing::usage`] over an explicit window.
    ///
    /// `from` is inclusive and `to` exclusive, both ISO timestamps. Passed
    /// through as given — this crate has no date type and will not reformat
    /// them, so an unparseable value comes back as
    /// [`ChatlyError::Validation`] rather than being silently corrected.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn usage_range(
        &self,
        event_type: &str,
        from: &str,
        to: &str,
    ) -> Result<Value, ChatlyError> {
        self.usage(event_type, &Query::new().set("from", from).set("to", to))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_event_types_match_the_servers_closed_list() {
        assert_eq!(USAGE_EVENT_TYPES.len(), 4);
        for expected in [
            "conversation.opened",
            "message.sent.agent",
            "message.sent.bot",
            "ai.tokens",
        ] {
            assert!(USAGE_EVENT_TYPES.contains(&expected), "{expected}");
        }
        // Dots, not underscores or colons.
        assert!(USAGE_EVENT_TYPES.iter().all(|t| t.contains('.')));
    }
}
