//! The input and result types of [`crate::verify_chatly_signature`].
//!
//! Split from `signatures.rs` to keep each file under the 650-line ceiling;
//! everything here is re-exported from the crate root.

use crate::signature_table::{
    ChatlySignatureBase, ChatlySignatureFailure, ChatlySignatureSurface, ChatlyV2Policy,
    CHATLY_SIGNATURE_TOLERANCE_SECONDS,
};

/// Everything [`crate::verify_chatly_signature`] needs, borrowed from the request.
///
/// Build it with [`ChatlySignatureInput::new`] and the chained setters, or
/// fill the public fields directly.
///
/// Headers are a LIST of `(name, value)` pairs rather than a map, because a
/// list is what can represent the attack this verifier refuses: the same
/// header twice. Names are compared case-insensitively and every matching
/// pair counts, so `("x-a", v)` and `("X-A", w)` are two values — exactly
/// what an `http::HeaderMap` yields from `iter()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatlySignatureInput<'a> {
    /// The surface's stable name. A string rather than the enum so a name
    /// held in configuration can be passed straight through — an unknown one
    /// is the `unknown_surface` refusal, not a parse step the caller writes.
    pub surface: &'a str,
    /// The EXACT bytes received, before any JSON parsing. Re-serialising
    /// parsed JSON reorders keys and changes whitespace, and the digest will
    /// not match.
    pub body: &'a [u8],
    /// The request's headers, as `(name, value)` pairs.
    pub headers: Vec<(&'a str, &'a str)>,
    /// Every secret currently live on this surface, in order. Empty strings
    /// are skipped, and [`ChatlySignatureVerified::secret_index`] indexes
    /// THIS list — not the list with the empties removed.
    pub secrets: Vec<&'a str>,
    /// Freshness window in SECONDS, whatever unit the surface sends, applied
    /// in both directions. Default [`CHATLY_SIGNATURE_TOLERANCE_SECONDS`].
    /// `u64::MAX` disables it in practice — only for re-verifying an archive.
    pub tolerance_seconds: u64,
    /// The request's HTTP method, inside the signed string on `unified_v2`
    /// and `ai_tool_call`. Upper-cased before signing.
    pub method: Option<&'a str>,
    /// `pathname + search` of the request as it arrived — never the scheme
    /// or the host.
    pub path: Option<&'a str>,
    /// How hard to insist on the v2 header. Default [`ChatlyV2Policy::Prefer`].
    pub v2: ChatlyV2Policy,
    /// `workflow_webhook_legacy` only, and required there: that surface signs
    /// the body with NO timestamp, so a captured request verifies forever.
    pub allow_unprotected_replay: bool,
    /// Pin "now", in Unix MILLISECONDS. `None` reads the system clock.
    pub now_ms: Option<i64>,
}

impl<'a> ChatlySignatureInput<'a> {
    /// Start an input for `surface` over the raw `body`. Add the secret(s)
    /// with [`ChatlySignatureInput::secret`] and the headers with
    /// [`ChatlySignatureInput::header`] or [`ChatlySignatureInput::headers`].
    pub fn new(surface: ChatlySignatureSurface, body: &'a (impl AsRef<[u8]> + ?Sized)) -> Self {
        Self::named(surface.as_str(), body)
    }

    /// [`ChatlySignatureInput::new`] with the surface as a string, which may
    /// be unknown (and will then be refused as `unknown_surface`).
    pub fn named(surface: &'a str, body: &'a (impl AsRef<[u8]> + ?Sized)) -> Self {
        Self {
            surface,
            body: body.as_ref(),
            headers: Vec::new(),
            secrets: Vec::new(),
            tolerance_seconds: CHATLY_SIGNATURE_TOLERANCE_SECONDS,
            method: None,
            path: None,
            v2: ChatlyV2Policy::Prefer,
            allow_unprotected_replay: false,
            now_ms: None,
        }
    }

    /// Append a secret. The first call is index 0, the next index 1, and so
    /// on — pass the old and the new secret during a rotation.
    pub fn secret(mut self, secret: &'a str) -> Self {
        self.secrets.push(secret);
        self
    }

    /// Append one header. Call it twice with one name for a repeated header.
    pub fn header(mut self, name: &'a str, value: &'a str) -> Self {
        self.headers.push((name, value));
        self
    }

    /// Append every `(name, value)` pair.
    pub fn headers(mut self, pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        self.headers.extend(pairs);
        self
    }

    /// Set the method and the path, which is what lets `v2: Prefer` pick up
    /// the v2 header on any surface.
    pub fn target(mut self, method: &'a str, path: &'a str) -> Self {
        self.method = Some(method);
        self.path = Some(path);
        self
    }

    /// Set the freshness window, in seconds.
    pub fn tolerance_seconds(mut self, seconds: u64) -> Self {
        self.tolerance_seconds = seconds;
        self
    }

    /// Set the v2 policy.
    pub fn v2(mut self, policy: ChatlyV2Policy) -> Self {
        self.v2 = policy;
        self
    }

    /// Accept `workflow_webhook_legacy`. Make the endpoint idempotent first.
    pub fn allow_unprotected_replay(mut self, allow: bool) -> Self {
        self.allow_unprotected_replay = allow;
        self
    }

    /// Pin "now", in Unix milliseconds.
    pub fn now_ms(mut self, now_ms: i64) -> Self {
        self.now_ms = Some(now_ms);
        self
    }

    /// Both method and path present and non-empty — the same test the
    /// reference applies, so an empty path is "no target", not a target of "".
    pub(crate) fn has_target(&self) -> bool {
        matches!(self.method, Some(m) if !m.is_empty())
            && matches!(self.path, Some(p) if !p.is_empty())
    }
}

/// A request that verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatlySignatureVerified {
    /// What actually verified — `UnifiedV2` when the v2 header was preferred
    /// over the surface asked for.
    pub surface: ChatlySignatureSurface,
    /// When the sender says it signed, in Unix milliseconds — normalised, so
    /// a seconds surface and a milliseconds surface are comparable. `None` on
    /// `workflow_webhook_legacy`, which sends no timestamp.
    pub timestamp_ms: Option<u64>,
    /// Which string the matching MAC was taken over.
    pub base: ChatlySignatureBase,
    /// Index into [`ChatlySignatureInput::secrets`] that verified. During a
    /// rotation this is how you learn the old secret is no longer in use.
    pub secret_index: usize,
}

/// A request that was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatlySignatureRefusal {
    /// The surface the refusal is about, as its stable name. A `String`
    /// because an `unknown_surface` refusal names a surface that has no
    /// variant; otherwise it is a [`ChatlySignatureSurface::as_str`] value.
    pub surface: String,
    /// Why. Stable, safe to branch on.
    pub reason: ChatlySignatureFailure,
    /// A sentence written to be printed to whoever has to fix it.
    pub message: String,
}

/// The outcome of [`crate::verify_chatly_signature`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatlySignatureResult {
    /// The signature verified.
    Verified(ChatlySignatureVerified),
    /// It did not, and here is why.
    Refused(ChatlySignatureRefusal),
}

impl ChatlySignatureResult {
    /// `true` for [`ChatlySignatureResult::Verified`].
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Verified(_))
    }

    /// The surface's stable name, on either arm.
    pub fn surface(&self) -> &str {
        match self {
            Self::Verified(v) => v.surface.as_str(),
            Self::Refused(r) => &r.surface,
        }
    }

    /// The refusal reason, or `None` when it verified.
    pub fn reason(&self) -> Option<ChatlySignatureFailure> {
        match self {
            Self::Verified(_) => None,
            Self::Refused(r) => Some(r.reason),
        }
    }
}
