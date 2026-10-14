//! Contacts — the people your agents talk to.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::{encode_path_segment, Query};

/// A contact to create or update, for [`Contacts::upsert`].
///
/// Mirrors the API's `contactUpsertInputSchema`. Every field is optional on
/// the wire; `external_id` is what makes the call an *upsert* rather than an
/// insert, so omitting it creates a new contact every time.
///
/// You do not have to use this struct — every write method on this crate
/// takes any `Serialize`, so a `serde_json::json!` literal or your own type
/// works identically. It exists so the common case is discoverable without
/// reading the API reference.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactInput {
    /// Your own stable id for this person. The upsert key. Max 256 chars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    /// Email address. Validated server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Phone number. Max 32 chars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Display name. Max 160 chars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Avatar URL. Validated server-side as a URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// BCP 47 locale, max 10 chars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// IANA timezone, max 64 chars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Arbitrary custom attributes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Value>,
}

impl ContactInput {
    /// An input with nothing set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the upsert key.
    pub fn external_id(mut self, external_id: impl Into<String>) -> Self {
        self.external_id = Some(external_id.into());
        self
    }

    /// Set the email address.
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Set the display name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the phone number.
    pub fn phone(mut self, phone: impl Into<String>) -> Self {
        self.phone = Some(phone.into());
        self
    }

    /// Set the custom attributes.
    pub fn attributes(mut self, attributes: Value) -> Self {
        self.attributes = Some(attributes);
        self
    }
}

/// The `contacts` namespace. Get one from [`ChatlyClient::contacts`].
#[derive(Debug, Clone, Copy)]
pub struct Contacts<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Contacts<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/contacts` — one page.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page("/v1/contacts", query)
    }

    /// `GET /v1/contacts` — every page, as an iterator.
    ///
    /// Any `cursor` in `query` is the starting point; the rest is managed
    /// for you. See [`PageIter`] for how it terminates.
    pub fn iter(&self, query: &Query) -> PageIter<Value> {
        self.client.paginate("/v1/contacts", query)
    }

    /// `GET /v1/contacts/{id}`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such contact.
    pub fn get(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/contacts/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }

    /// `POST /v1/contacts` — create or update one contact.
    ///
    /// Takes a [`ContactInput`], or any `Serialize`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if the body fails
    /// the API's schema.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), chatly::ChatlyError> {
    /// # let chatly = chatly::ChatlyClient::new("ck_x")?;
    /// use chatly::ContactInput;
    /// chatly.contacts().upsert(
    ///     &ContactInput::new().external_id("u_42").email("ada@example.com"),
    /// )?;
    /// # Ok(()) }
    /// ```
    pub fn upsert(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/contacts", &Query::new(), Some(body))
    }

    /// `POST /v1/contacts/bulk` — up to 5,000 contacts in one call.
    ///
    /// The API expects `{ "contacts": [ … ] }` with between 1 and 5,000
    /// entries, each shaped like a [`ContactInput`]. Pass
    /// [`BulkContactsInput`] to get that envelope built for you.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an empty or
    /// oversized batch.
    pub fn bulk_upsert(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/contacts/bulk", &Query::new(), Some(body))
    }

    /// `POST /v1/contacts/{id}/merge` — fold another contact into this one.
    ///
    /// The API expects `{ "mergedId": "<uuid>" }`: `id` is the contact that
    /// survives and `merged_id` is the one absorbed. Getting them the wrong
    /// way round is not an error the API can detect for you.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn merge(&self, id: &str, merged_id: &str) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!("/v1/contacts/{}/merge", encode_path_segment(id)),
            &Query::new(),
            Some(&serde_json::json!({ "mergedId": merged_id })),
        )
    }

    /// `POST /v1/contacts/{id}/tags` — add **one** tag.
    ///
    /// The route is plural but its body is `{ "tag": "<one tag>" }`, so this
    /// is one call per tag. [`Contacts::add_tags`] loops for you.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn add_tag(&self, id: &str, tag: &str) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!("/v1/contacts/{}/tags", encode_path_segment(id)),
            &Query::new(),
            Some(&serde_json::json!({ "tag": tag })),
        )
    }

    /// Add several tags, one request each, stopping at the first failure.
    ///
    /// Named for `surface.json`'s `contacts.add_tags`. Not atomic: the
    /// underlying route takes a single tag, so a failure partway through
    /// leaves the earlier tags applied. Returns the last response.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`], plus [`ChatlyError::Config`] if `tags` is empty.
    pub fn add_tags(&self, id: &str, tags: &[&str]) -> Result<Value, ChatlyError> {
        let Some((last, rest)) = tags.split_last() else {
            return Err(ChatlyError::Config("add_tags: no tags given".to_owned()));
        };
        for tag in rest {
            self.add_tag(id, tag)?;
        }
        self.add_tag(id, last)
    }

    /// `DELETE /v1/contacts/{id}/tags/{tag}` — remove one tag.
    ///
    /// `tag` is percent-encoded, so a tag with a space or a `/` in it works.
    /// Answers the surviving contact rather than a 204.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn remove_tag(&self, id: &str, tag: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!(
                "/v1/contacts/{}/tags/{}",
                encode_path_segment(id),
                encode_path_segment(tag)
            ),
            &Query::new(),
        )
    }

    /// `POST /v1/contacts/{id}/unsubscribe` — set the marketing
    /// subscription flag.
    ///
    /// The API expects `{ "unsubscribed": <bool>, "reason"?: "<text>" }`, so
    /// this is also how you *re*-subscribe someone: pass `false`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn unsubscribe(
        &self,
        id: &str,
        unsubscribed: bool,
        reason: Option<&str>,
    ) -> Result<Value, ChatlyError> {
        let mut body = serde_json::Map::new();
        body.insert("unsubscribed".to_owned(), Value::Bool(unsubscribed));
        if let Some(reason) = reason {
            body.insert("reason".to_owned(), Value::String(reason.to_owned()));
        }
        self.client.write(
            "POST",
            &format!("/v1/contacts/{}/unsubscribe", encode_path_segment(id)),
            &Query::new(),
            Some(&Value::Object(body)),
        )
    }

    /// `GET /v1/contacts/{id}/journey` — one page of this contact's
    /// activity timeline.
    ///
    /// Accepts `days` (1–365, default 90), `limit` (1–100, default 25) and
    /// `cursor`.
    ///
    /// **The cursor is an ISO-8601 instant**, not an opaque token —
    /// `journeyQuerySchema` declares `cursor: z.string().datetime({ offset:
    /// true })`. That makes it tempting to synthesise one from a timestamp
    /// you already have. Do not: it is still a cursor, with the server's
    /// tie-breaking and inclusivity rules behind it. Walk it, or use
    /// [`Contacts::iter_journey`].
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn journey(&self, id: &str, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page(
            &format!("/v1/contacts/{}/journey", encode_path_segment(id)),
            query,
        )
    }

    /// `GET /v1/contacts/{id}/journey` — the whole timeline, as an iterator.
    ///
    /// Walks the ISO-8601 cursors for you, which is the only way you should
    /// be producing them.
    pub fn iter_journey(&self, id: &str, query: &Query) -> PageIter<Value> {
        self.client.paginate(
            &format!("/v1/contacts/{}/journey", encode_path_segment(id)),
            query,
        )
    }

    /// `GET /v1/contacts/{id}/mood` — weekly sentiment buckets for one
    /// contact.
    ///
    /// `days` is 7–365, default **90** — note the floor: fewer than seven
    /// days is refused rather than clamped, because a week is the bucket
    /// width.
    ///
    /// **`buckets: []` with a low `sampleCount` is the ordinary
    /// not-enough-signal answer, not an error.** The series is computed from
    /// a sentiment cache that only fills for contacts who have said enough,
    /// so an empty array for a new contact is correct and the dashboard
    /// renders an empty state from it. Read `sampleCount` before drawing a
    /// chart.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for `days` outside
    /// 7–365.
    pub fn mood(&self, id: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/contacts/{}/mood", encode_path_segment(id)),
            query,
        )
    }

    /// `POST /v1/contacts/{contact_id}/cards` — pull this contact's customer
    /// cards live from **your** backend.
    ///
    /// A POST for a read, and three reasons it has to be: the answer depends
    /// on a `conversationId` in the body, it is not cacheable by any
    /// intermediary, and a GET with a conversation id in the query string
    /// would land in an access log beside the contact's email.
    ///
    /// Body: `{ "conversationId"?: "<uuid>" }`, `.strict()`. Omit it for the
    /// contact-level view.
    ///
    /// **This makes outbound HTTP calls to the URLs you configured** (see
    /// [`crate::CustomerCards`]) while the request is open, one per card, so
    /// it is the slowest route in this namespace and its latency is your
    /// backend's. Each entry carries a `state` — `ok`, `empty`, `missing`,
    /// `invalid` or `error` — and `blocks: null` is a real value meaning "no
    /// data for this contact", not a failure. **Read `state`, not
    /// `blocks == null`**, to tell an empty card from a broken one.
    ///
    /// Answers **201**, and needs `contacts:read` rather than an
    /// integrations scope.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]. A card endpoint of yours that times out does
    /// **not** fail this call: it comes back as that card's `state`.
    pub fn cards(&self, contact_id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!("/v1/contacts/{}/cards", encode_path_segment(contact_id)),
            &Query::new(),
            Some(body),
        )
    }
}

/// The envelope [`Contacts::bulk_upsert`] expects: `{ "contacts": [ … ] }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BulkContactsInput {
    /// Between 1 and 5,000 contacts.
    pub contacts: Vec<ContactInput>,
}

impl BulkContactsInput {
    /// Wrap a batch of contacts.
    pub fn new(contacts: Vec<ContactInput>) -> Self {
        Self { contacts }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn contact_input_omits_unset_fields_and_uses_camel_case() {
        let json =
            serde_json::to_value(ContactInput::new().external_id("u_1").name("Ada")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "externalId": "u_1", "name": "Ada" })
        );
    }

    #[test]
    fn bulk_envelope_is_a_contacts_array() {
        let json = serde_json::to_value(BulkContactsInput::new(vec![
            ContactInput::new().external_id("a")
        ]))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "contacts": [{ "externalId": "a" }] })
        );
    }

    #[test]
    fn contact_input_round_trips() {
        let input = ContactInput::new()
            .external_id("u_1")
            .email("a@b.c")
            .phone("+100")
            .attributes(serde_json::json!({ "plan": "pro" }));
        let text = serde_json::to_string(&input).unwrap();
        assert_eq!(serde_json::from_str::<ContactInput>(&text).unwrap(), input);
    }
}
