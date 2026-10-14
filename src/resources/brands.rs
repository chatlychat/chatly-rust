//! Brands — optional sub-tenants inside one workspace.
//!
//! A brand carries its own name, help-centre hostname, reply domain and
//! widget chrome. Channels and conversations point at one; a workspace that
//! has never created a brand works fine without any.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `brands` namespace. Get one from [`ChatlyClient::brands`].
#[derive(Debug, Clone, Copy)]
pub struct Brands<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Brands<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/brands` — every brand, as a **bare JSON array**, each with
    /// usage counts.
    ///
    /// One list route serves both the brand switcher (which needs names) and
    /// the settings page (which needs to show what each brand is carrying),
    /// so every row pays for the usage counts. Pagination-exempt: typically
    /// under ten rows.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/brands", query)
    }

    /// `POST /v1/brands` — create a brand.
    ///
    /// `name` (1–120) and `slug` (1–64, lowercase alphanumerics and hyphens)
    /// are required. Then `logoUrl`, `primaryColor` (`#RRGGBB`),
    /// `supportEmail`, `domain`, `replyDomain`, `icons` and `isDefault`.
    ///
    /// Three things the field names do not tell you:
    ///
    /// - **`domain` and `replyDomain` are different hostnames.** `domain` is
    ///   the help-centre host (`help.acme.com`); `replyDomain` is where
    ///   `reply+<conversation>.<sig>@…` is addressed (`mail.acme.com`).
    ///   `domain` is the fallback when `replyDomain` is unset, so pointing an
    ///   MX record at a help-centre host is a mistake the schema cannot
    ///   catch.
    /// - **Setting `replyDomain` enables nothing by itself.** The domain must
    ///   also be claimed and verified through `POST /v1/domains` with
    ///   `kind: "email"`; inbound mail reads that verification, not this
    ///   field.
    /// - **`isDefault: true` demotes the current default.** A workspace has
    ///   at most one, enforced by a partial unique index, so this is a move
    ///   and not a second flag.
    ///
    /// `icons` values are validated as *markup*: an inline `<svg>` carrying a
    /// script or an event handler is refused, because editing branding must
    /// not amount to running JavaScript on every page the widget is embedded
    /// in.
    ///
    /// Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a slug that does
    /// not match, a colour that is not `#RRGGBB`, or an unsafe icon.
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/brands", &Query::new(), Some(body))
    }

    /// `PATCH /v1/brands/{id}` — update a brand.
    ///
    /// Every create field, all optional. `icons` is the one key that
    /// **merges** rather than replaces — a patch touching one icon does not
    /// clear the other four — so clearing an override means sending it
    /// explicitly rather than omitting it.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such brand.
    pub fn update(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/brands/{}", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `DELETE /v1/brands/{id}` — soft-delete, answering `{ "id": … }` with
    /// a 200.
    ///
    /// Not a 204: the id it soft-deleted is the caller's only confirmation.
    ///
    /// Refused with a **409** in two cases, both of which are a request to
    /// fix something first rather than a failure of this call: the brand is
    /// the workspace default (promote another one), or channels still point
    /// at it (the message names how many).
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Conflict`] for the two cases
    /// above, [`ChatlyError::NotFound`] if no such brand.
    pub fn delete(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/brands/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }
}
