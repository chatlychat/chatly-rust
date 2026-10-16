//! Minting a v2 signature — the other direction from [`crate::verify_chatly_signature`].
//!
//! Verifying is what a customer's handler does and it lives next door.
//! MINTING is what a test harness does: to prove a verifier accepts a genuine
//! signature you have to be able to produce one, and a test that hard-codes a
//! hex digest proves only that the digest has not changed, not that it is the
//! right digest for the string it claims to cover.
//!
//! This mirrors `@chatlychat/sdk-node`'s `signChatlyV2` / `chatlyV2BaseString`
//! (`packages/sdk-node/src/signature-mint.ts`) so the two can be held against
//! each other: [`chatly_v2_base_string`]'s tests pin the exact signed string
//! and the exact digest from the cross-SDK corpus, which is the only way a
//! port can be shown to agree byte for byte rather than merely to round-trip
//! against itself. A self-consistent signer and verifier agree on a wrong
//! construction perfectly happily — that is how an earlier implementation in
//! this repository shipped a body-only digest under a header documenting a
//! timestamped one.
//!
//! ## No new dependency
//!
//! HMAC-SHA256 is already in this crate: `src/crypto.rs` carries ~200 lines
//! of std SHA-256 and HMAC, pinned by the contract's own vectors, which is
//! why `sha2` and `hmac` are listed in `Cargo.toml` as deliberate
//! non-dependencies. This module adds nothing to the dependency tree.
//!
//! ## Why it returns a `Result`
//!
//! [`crate::verify_chatly_signature`] deliberately never fails loudly — a
//! verifier that panics is an accepted webhook in a handler that cannot
//! `catch_unwind`. Minting is the opposite situation. There is no caller to
//! protect (the caller IS the test) and every way of getting it wrong
//! produces a signature that is perfectly well formed over the wrong bytes.
//! The three are enumerated on [`ChatlyV2MintError`].

use crate::crypto;
use std::fmt;

/// Which string a v2 signature is taken over.
///
/// No `Default`, on purpose: the two constructions are indistinguishable on
/// the wire without the `b=` marker this emits, which is the whole reason the
/// marker exists, so a caller has to say which one it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatlyV2MintBase {
    /// `<t>.<METHOD>.<path>.<rawBody>` — the unified base. Emits `b=t`.
    Target,
    /// `<t>.<rawBody>`, the workflow action's pre-unified construction.
    /// Emits `b=b`. Mint one to test a receiver that still has to accept it.
    Body,
}

impl ChatlyV2MintBase {
    /// The `b=` marker this base is labelled with on the wire.
    pub fn marker(self) -> &'static str {
        match self {
            Self::Target => "t",
            Self::Body => "b",
        }
    }
}

/// Why a v2 signature could not be minted.
///
/// Both variants describe an input that would have yielded a well-formed
/// signature covering the wrong bytes, which is strictly worse than an error:
/// it verifies nowhere and the symptom at the far end is "invalid signature",
/// indistinguishable from a wrong secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatlyV2MintError {
    /// [`ChatlyV2MintBase::Target`] with no method or no path.
    ///
    /// Signing without them yields `"<t>..."` — the same string for every
    /// request the surface ever makes, replayable against any path. That is
    /// the precise weakness the target base was introduced to close (see
    /// `signature_table.rs`), so minting one silently would manufacture the
    /// bug the construction exists to prevent.
    MissingTarget {
        /// What was supplied, for the message.
        method: Option<String>,
        /// What was supplied, for the message.
        path: Option<String>,
    },
    /// A negative `timestamp_seconds`.
    ///
    /// It renders `t=-1`, and every verifier in this family refuses that as
    /// `malformed_timestamp` because a timestamp is a run of digits. The
    /// signature would be unacceptable everywhere, for a reason that looks
    /// nothing like its cause.
    NegativeTimestamp(i64),
}

impl fmt::Display for ChatlyV2MintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTarget { method, path } => write!(
                f,
                "ChatlyV2MintBase::Target signs <t>.<METHOD>.<path>.<rawBody>, so both a method \
                 and a path are required; got method={method:?} path={path:?}. Signing without \
                 them yields \"<t>...\" — the same string for every request on the surface, \
                 replayable against any path, which is the weakness the target base exists to \
                 close"
            ),
            Self::NegativeTimestamp(seconds) => write!(
                f,
                "timestamp_seconds is {seconds}, which renders t={seconds} — every verifier in \
                 this family requires t to be a run of digits and would refuse it as \
                 malformed_timestamp"
            ),
        }
    }
}

impl std::error::Error for ChatlyV2MintError {}

/// Everything needed to mint one `x-chatly-signature-v2` value.
///
/// Build it with [`ChatlyV2Mint::new`] and the chained setters, mirroring
/// [`crate::ChatlySignatureInput`] so the two directions read alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatlyV2Mint<'a> {
    /// Which string to sign. Required; there is no default.
    pub base: ChatlyV2MintBase,
    /// The signing secret for the surface being imitated.
    ///
    /// An empty secret is NOT an error: HMAC accepts a zero-length key, the
    /// reference implementation signs with one, and a test proving a verifier
    /// answers `missing_secret` has to be able to produce the input.
    pub secret: &'a str,
    /// The exact bytes that will be sent. Empty is the empty body, which is
    /// what a GET signs.
    pub body: &'a [u8],
    /// Unix SECONDS, not milliseconds. The v2 construction's `t` is seconds
    /// on every surface; a millisecond value here lands roughly 54,000 years
    /// out and a verifier refuses it on time rather than on the digest.
    pub timestamp_seconds: i64,
    /// The request's HTTP method, upper-cased before signing, so a test may
    /// pass `"get"` exactly as the corpus does. Required for
    /// [`ChatlyV2MintBase::Target`], ignored for [`ChatlyV2MintBase::Body`].
    pub method: Option<&'a str>,
    /// `pathname + search` of the request line, never the scheme or the
    /// host. Required for [`ChatlyV2MintBase::Target`], ignored for
    /// [`ChatlyV2MintBase::Body`].
    pub path: Option<&'a str>,
}

impl<'a> ChatlyV2Mint<'a> {
    /// Start a mint over `body` with `secret`, at `timestamp_seconds`.
    pub fn new(
        base: ChatlyV2MintBase,
        secret: &'a str,
        body: &'a (impl AsRef<[u8]> + ?Sized),
        timestamp_seconds: i64,
    ) -> Self {
        Self {
            base,
            secret,
            body: body.as_ref(),
            timestamp_seconds,
            method: None,
            path: None,
        }
    }

    /// Set the method and the path, required for [`ChatlyV2MintBase::Target`].
    pub fn target(mut self, method: &'a str, path: &'a str) -> Self {
        self.method = Some(method);
        self.path = Some(path);
        self
    }

    /// The method and path, or the error naming what is missing. Empty is
    /// treated as absent, the same test [`crate::ChatlySignatureInput`]
    /// applies, so an empty path is "no target" rather than a target of `""`.
    fn require_target(&self) -> Result<(&str, &str), ChatlyV2MintError> {
        match (self.method, self.path) {
            (Some(m), Some(p)) if !m.is_empty() && !p.is_empty() => Ok((m, p)),
            _ => Err(ChatlyV2MintError::MissingTarget {
                method: self.method.map(str::to_owned),
                path: self.path.map(str::to_owned),
            }),
        }
    }
}

/// The exact bytes the MAC is taken over, exported so a test can pin them
/// independently of the digest.
///
/// Worth having separately: a wrong base string and a wrong HMAC are the same
/// symptom at the verifier ("signature mismatch") and completely different
/// bugs, and pinning the plaintext is what tells them apart. It is also the
/// value to print when a cross-language comparison disagrees.
///
/// A `Vec<u8>` rather than a `String` because the body is arbitrary bytes and
/// need not be UTF-8 — HMAC does not care, and forcing a lossy conversion
/// here would sign something other than what will be sent.
pub fn chatly_v2_base_string(mint: &ChatlyV2Mint<'_>) -> Result<Vec<u8>, ChatlyV2MintError> {
    if mint.timestamp_seconds < 0 {
        return Err(ChatlyV2MintError::NegativeTimestamp(mint.timestamp_seconds));
    }
    let t = mint.timestamp_seconds.to_string();
    let mut out = Vec::with_capacity(t.len() + mint.body.len() + 32);
    out.extend_from_slice(t.as_bytes());
    out.push(b'.');
    if mint.base == ChatlyV2MintBase::Target {
        let (method, path) = mint.require_target()?;
        out.extend_from_slice(method.to_uppercase().as_bytes());
        out.push(b'.');
        out.extend_from_slice(path.as_bytes());
        out.push(b'.');
    }
    out.extend_from_slice(mint.body);
    Ok(out)
}

/// The `x-chatly-signature-v2` value for one request: `t=…,v1=…,b=…`.
///
/// `b=t` for the unified base and `b=b` for the body base. An ABSENT marker
/// means the body base to a verifier — every signature minted before the
/// marker existed was that construction — but this always emits one, because
/// there is no reason for something minted today to be ambiguous.
///
/// To build the dual-digest value a sender emits mid key-rotation, call this
/// once per secret and join the `v1` fields;
/// [`crate::verify_chatly_signature`] accepts a value carrying several when
/// any one of them matches.
///
/// ```
/// use chatly::{sign_chatly_v2, ChatlyV2Mint, ChatlyV2MintBase};
///
/// let mint = ChatlyV2Mint::new(ChatlyV2MintBase::Target, "whsec_x", "{}", 1_760_000_000)
///     .target("POST", "/hooks/chatly");
/// let value = sign_chatly_v2(&mint)?;
/// assert!(value.starts_with("t=1760000000,v1="));
/// assert!(value.ends_with(",b=t"));
/// # Ok::<(), chatly::ChatlyV2MintError>(())
/// ```
pub fn sign_chatly_v2(mint: &ChatlyV2Mint<'_>) -> Result<String, ChatlyV2MintError> {
    let base = chatly_v2_base_string(mint)?;
    let digest = crypto::hmac_sha256(mint.secret.as_bytes(), &[&base]);
    Ok(format!(
        "t={},v1={},b={}",
        mint.timestamp_seconds,
        crypto::hex_encode(&digest),
        mint.base.marker()
    ))
}
