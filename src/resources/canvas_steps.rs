//! Canvas steps — the half of an interactive widget flow that happens on
//! your server.
//!
//! A canvas is a small form rendered inside the widget. When a visitor
//! submits one, Chatly posts the submission to your app and shows the visitor
//! a progress line; your app then calls [`CanvasSteps::resolve`] with the
//! next canvas, which replaces that line. A step is the row that tracks one
//! of those hand-offs.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `canvas_steps` namespace. Get one from
/// [`ChatlyClient::canvas_steps`].
#[derive(Debug, Clone, Copy)]
pub struct CanvasSteps<'a> {
    client: &'a ChatlyClient,
}

impl<'a> CanvasSteps<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/canvas-steps/{id}` — is this step still pending, resolved,
    /// or expired?
    ///
    /// The question to ask before deciding whether to refund a charge you
    /// took for a step you never managed to resolve.
    ///
    /// **It carries no canvas.** What the visitor is looking at is readable
    /// from the conversation; this route answers about the hand-off, not
    /// about the form.
    ///
    /// Needs `messages:read`. The conversation is read from the step row and
    /// never from the request, so there is no way to use a step id to reach a
    /// thread it does not belong to.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if the id names no
    /// step in this workspace.
    pub fn get(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/canvas-steps/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }

    /// `POST /v1/canvas-steps/{id}/resolve` — replace the visitor's progress
    /// line with the next canvas.
    ///
    /// Body: `{ "canvas": { … } }`. Publishes `message.updated`, so an open
    /// widget updates without a reload. The conversation and the message both
    /// come from the step id and never from your request.
    ///
    /// Two refusals that look like bugs and are not:
    ///
    /// - **404 can mean you were too fast.** The step row is written *after*
    ///   we answer the visitor, so resolving from inside the very request
    ///   that is about to return `{ pending: true }` races us. Answer first,
    ///   then resolve.
    /// - **409 means the step is spent** — already resolved, or lapsed after
    ///   ten minutes. The visitor has already been told it did not complete,
    ///   so a fresh canvas is the honest next move rather than a retry.
    ///
    /// A step is claimed by an UPDATE and visibility is checked before that
    /// claim, so a refused call has not consumed the step.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] and
    /// [`ChatlyError::Conflict`] as above.
    pub fn resolve(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!("/v1/canvas-steps/{}/resolve", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }
}
