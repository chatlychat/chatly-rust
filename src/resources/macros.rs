//! Macros — saved replies and canned actions.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::{encode_path_segment, Query};

/// The `scope` values a macro can have.
pub const MACRO_SCOPES: [&str; 3] = ["workspace", "team", "personal"];

/// The `key` values a macro action can have.
///
/// A macro action is one of the conversation primitives; anything outside
/// this list is rejected by the API's schema.
pub const MACRO_ACTION_KEYS: [&str; 11] = [
    "assign",
    "assign_team",
    "transfer_to_team",
    "unassign",
    "change_status",
    "close_conversation",
    "snooze",
    "tag",
    "remove_tag",
    "set_priority",
    "send_internal_note",
];

/// The `macros` namespace. Get one from [`ChatlyClient::macros`].
///
/// The only namespace in this crate with a full create/update/delete set
/// reachable by an API key.
#[derive(Debug, Clone, Copy)]
pub struct Macros<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Macros<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/macros` — one page.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page("/v1/macros", query)
    }

    /// `GET /v1/macros` — every page, as an iterator.
    pub fn iter(&self, query: &Query) -> PageIter<Value> {
        self.client.paginate("/v1/macros", query)
    }

    /// `POST /v1/macros` — create a macro.
    ///
    /// `name` (1–200 chars) and `scope` are required; `scope` is one of
    /// [`MACRO_SCOPES`], and `teamId` becomes required when it is `team`.
    /// `body` (≤20,000 chars), `actions`, `shortCode`, `language`
    /// (2–8 chars, default `en`) and `tags` (≤20, each ≤40 chars) are
    /// optional — but **a non-blank `body` or at least one `action` is
    /// mandatory**, and a macro with neither is rejected with the error
    /// reported against `body`.
    ///
    /// Each entry in `actions` is `{ "key": …, "params": {…} }`, where
    /// `key` is one of [`MACRO_ACTION_KEYS`] and there may be at most 16.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if the body fails
    /// the API's schema.
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/macros", &Query::new(), Some(body))
    }

    /// `PATCH /v1/macros/{id}` — update a macro.
    ///
    /// A PATCH: every field is optional and only what you send is changed.
    ///
    /// Two asymmetries with [`Macros::create`] worth knowing, because they
    /// let an update produce a macro the create route would have refused.
    /// The "body or actions must be non-empty" rule is re-checked only when
    /// the patch contains **both** keys, so patching `body` to `""` alone
    /// is accepted. And the "`teamId` is required when `scope` is `team`"
    /// rule is not re-applied at all, so moving a macro to `team` scope
    /// without a `teamId` is accepted here.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn update(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/macros/{}", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `DELETE /v1/macros/{id}`.
    ///
    /// Carries an `Idempotency-Key` like every other write, so an SDK-level
    /// retry of a delete is not a second delete.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such macro.
    pub fn delete(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/macros/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn scopes_and_action_keys_match_the_api_vocabularies() {
        assert_eq!(MACRO_SCOPES.len(), 3);
        assert!(MACRO_SCOPES.contains(&"personal"));
        assert_eq!(MACRO_ACTION_KEYS.len(), 11);
        assert!(MACRO_ACTION_KEYS.contains(&"send_internal_note"));
        // Not a conversation primitive, so not a macro action.
        assert!(!MACRO_ACTION_KEYS.contains(&"send_message"));
    }
}
