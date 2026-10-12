//! CDP events — the customer-data-platform activity stream.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::Query;

/// An event to record, for [`Events::track`].
///
/// Mirrors the API's `trackSchema`. There is no timestamp field: the API
/// sets `occurredAt` itself, so you cannot backdate an event through this
/// route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventInput {
    /// The contact this happened to. Required, and a UUID v7.
    pub contact_id: String,
    /// The event name, e.g. `order_placed`. Required, 1–120 characters.
    pub name: String,
    /// Arbitrary properties. Omitted when `None`; the API defaults it to an
    /// empty object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Value>,
}

impl EventInput {
    /// An event with no properties.
    pub fn new(contact_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            contact_id: contact_id.into(),
            name: name.into(),
            properties: None,
        }
    }

    /// Attach properties.
    pub fn properties(mut self, properties: Value) -> Self {
        self.properties = Some(properties);
        self
    }
}

/// The `events` namespace. Get one from [`ChatlyClient::events`].
///
/// One collection, `/v1/cdp/events`, written by [`Events::track`] and read
/// back by [`Events::list`]. Both halves matter: an SDK that could write
/// events and not read them left "did my tracking call actually land?" with
/// no answer outside the dashboard.
#[derive(Debug, Clone, Copy)]
pub struct Events<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Events<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `POST /v1/cdp/events` — record an event against a contact.
    ///
    /// The body is `{ "contactId": …, "name": …, "properties"?: {…} }` —
    /// see [`EventInput`].
    ///
    /// This route needs the `contacts:write:events` scope, which is
    /// narrower than `contacts:write`. A key with the latter and not the
    /// former gets [`ChatlyError::Permission`], so check
    /// [`ChatlyClient::whoami`] if a write that looks allowed is refused.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Permission`] for a missing
    /// scope, [`ChatlyError::Validation`] for a bad body.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), chatly::ChatlyError> {
    /// # let chatly = chatly::ChatlyClient::new("ck_x")?;
    /// use chatly::EventInput;
    /// chatly.events().track(
    ///     &EventInput::new("018f…", "order_placed")
    ///         .properties(serde_json::json!({ "total_cents": 4200 })),
    /// )?;
    /// # Ok(()) }
    /// ```
    pub fn track(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/cdp/events", &Query::new(), Some(body))
    }

    /// `GET /v1/cdp/events` — one page of the event stream.
    ///
    /// Takes the standard `cursor`, `limit` (1–100, default 25), `sort`,
    /// `fields` and `filter[]`, plus a `contactId` to scope the stream to
    /// one person. Sortable by `id`, `createdAt`, `occurredAt` and `name`
    /// — but see [`crate::Page`]: do not sort by `id` expecting time order.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page("/v1/cdp/events", query)
    }

    /// `GET /v1/cdp/events` — every page, as an iterator.
    pub fn iter(&self, query: &Query) -> PageIter<Value> {
        self.client.paginate("/v1/cdp/events", query)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn event_input_matches_the_track_schema_field_names() {
        let json = serde_json::to_value(EventInput::new("c_1", "order_placed")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "contactId": "c_1", "name": "order_placed" })
        );
        let json = serde_json::to_value(
            EventInput::new("c_1", "order_placed").properties(serde_json::json!({ "n": 1 })),
        )
        .unwrap();
        assert_eq!(json["properties"]["n"], 1);
    }
}
