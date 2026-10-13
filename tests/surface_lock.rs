//! Generates and verifies `surface.lock.json`.
//!
//! The lock is **executed evidence**. Each of the 55 entries in
//! `packages/sdk-spec/surface.json` is produced by actually invoking the
//! corresponding method against an injected transport and recording the
//! `METHOD /path` the client built. Interpolated path segments are folded
//! back to the manifest's own `{...}` names by position, so
//! `GET /v1/contacts/c_1/tags/vip` records as
//! `GET /v1/contacts/{id}/tags/{tag}`.
//!
//! Why it is generated this way: a parity gate that greps SDK source text
//! cannot tell a doc comment from a call site, and its first version counted
//! prose about two broken routes as implementations and printed 55/55 for
//! methods that did not exist. Nothing in this file can be satisfied by a
//! comment — if a method is missing, the closure does not compile; if it
//! builds the wrong URL, the recorded path differs from the manifest and the
//! assertion fails.
//!
//! Each entry also records the top-level **response shape** this SDK's
//! decoder expects — `array`, `page`, `object` or `empty`. That column is
//! hand-written in `common::shape_for` and deliberately NOT read from the
//! manifest, because a fixture derived from the manifest agrees with the
//! manifest even when the manifest is wrong. `macros.list` is the proof: it
//! was flagged `paged: true` for a day, a derived `Page` read its bare array
//! as an *empty page* and returned it with a 200, and the suite was green
//! because the fake answered every route with a page envelope.
//!
//! The test fails, rather than skipping, when:
//!
//! - the manifest has an entry with no closure here (a new method to write),
//! - this file has a closure with no manifest entry (surface drift),
//! - any method issues a different verb or path than the manifest says,
//! - any method issues more or fewer than one request,
//! - any method hands back a value of a shape other than the one declared
//!   for it, or drops the row the fake sent, or
//! - the regenerated lock differs from the manifest, shape column included.
//!
//! Run with `cargo test --test surface_lock`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use chatly::{ChatlyClient, Query};
use common::{client, normalise, shape_for, BodyShape, MockTransport, Returned};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// One manifest / lock entry. Field order here is the field order written to
/// `surface.lock.json`: `ns, name, http, path, shape`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    ns: String,
    name: String,
    http: String,
    path: String,
    /// The top-level response shape this SDK's decoder expects, named by
    /// [`BodyShape::name`]. Comes from `common::shape_for`, which is written
    /// by hand from the handlers and never read from the manifest.
    shape: String,
}

#[derive(Debug, Serialize)]
struct Lock {
    sdk: &'static str,
    #[serde(rename = "generatedBy")]
    generated_by: &'static str,
    entries: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    entries: Vec<ManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    ns: String,
    name: String,
    http: String,
    path: String,
    /// `true` only on the genuinely cursor-paginated routes. Absent on the
    /// rest, including `macros.list`, which is pagination-exempt.
    #[serde(default)]
    paged: Option<bool>,
    /// The manifest's own statement of the top-level response shape. Read
    /// here only to be COMPARED with this SDK's hand-written column — never
    /// to populate it. `None` means the manifest has not been through the
    /// shape pass, which is a failure and not a default.
    #[serde(default)]
    shape: Option<String>,
}

fn package_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn manifest_path() -> PathBuf {
    // Monorepo first, then a mirror's vendored copy at its root.
    let monorepo = package_dir().join("../sdk-spec/surface.json");
    if monorepo.exists() {
        return monorepo;
    }
    let mirror = package_dir().join("sdk-spec/surface.json");
    if mirror.exists() {
        return mirror;
    }
    monorepo
}

/// One method call, returning **both** the decoded value and the error.
///
/// Returning a `Result` rather than `()` is load-bearing: these closures once
/// wrote `let _ = …`, which threw the error away, so a route answered with a
/// shape its own return type could not decode still looked fine here. That is
/// half of why `macros().list` could raise `ChatlyError::Decode` for real
/// callers with this suite green.
///
/// Carrying the VALUE rather than `()` is the other half. A decoder can
/// succeed and still be wrong — unwrap a page into its `items`, wrap an array
/// into an envelope, or decode into a default that drops the payload — and
/// the request it issued is perfect in every one of those cases.
/// [`Returned`] exists so `observe` can classify what came back.
type Invocation = fn(&ChatlyClient) -> Result<Returned, chatly::ChatlyError>;

/// Every method in the manifest, as `(ns, name, invocation)`.
///
/// The closures capture nothing, so they coerce to `fn` pointers and this is
/// a plain table. Arguments are throwaway; only the request that comes out,
/// and whether the call succeeded, matter. Each closure must issue
/// **exactly one** request — `add_tags` is given a single tag for that
/// reason, since the underlying route takes one tag per call.
#[allow(clippy::type_complexity)]
fn invocations() -> Vec<(&'static str, &'static str, Invocation)> {
    vec![
        ("root", "whoami", |c| (c.whoami()).map(Returned::value)),
        // -- contacts -----------------------------------------------------
        ("contacts", "list", |c| {
            (c.contacts().list(&Query::new())).map(Returned::page)
        }),
        ("contacts", "get", |c| {
            (c.contacts().get("c_1")).map(Returned::value)
        }),
        ("contacts", "upsert", |c| {
            (c.contacts().upsert(&json!({}))).map(Returned::value)
        }),
        ("contacts", "bulk_upsert", |c| {
            (c.contacts().bulk_upsert(&json!({ "contacts": [] }))).map(Returned::value)
        }),
        ("contacts", "merge", |c| {
            (c.contacts().merge("c_1", "c_2")).map(Returned::value)
        }),
        ("contacts", "add_tags", |c| {
            (c.contacts().add_tags("c_1", &["vip"])).map(Returned::value)
        }),
        ("contacts", "remove_tag", |c| {
            (c.contacts().remove_tag("c_1", "vip")).map(Returned::value)
        }),
        ("contacts", "unsubscribe", |c| {
            (c.contacts().unsubscribe("c_1", true, None)).map(Returned::value)
        }),
        ("contacts", "journey", |c| {
            (c.contacts().journey("c_1", &Query::new())).map(Returned::page)
        }),
        // -- conversations ------------------------------------------------
        ("conversations", "list", |c| {
            (c.conversations().list(&Query::new())).map(Returned::page)
        }),
        ("conversations", "get", |c| {
            (c.conversations().get("v_1")).map(Returned::value)
        }),
        ("conversations", "messages", |c| {
            (c.conversations().messages("v_1", &Query::new())).map(Returned::page)
        }),
        ("conversations", "send", |c| {
            (c.conversations().send("v_1", &json!({ "body": "hi" }))).map(Returned::value)
        }),
        ("conversations", "unattended", |c| {
            (c.conversations().unattended(&Query::new())).map(Returned::value)
        }),
        ("conversations", "agent_handling", |c| {
            (c.conversations().agent_handling("v_1")).map(Returned::value)
        }),
        ("conversations", "set_agent_handling", |c| {
            (c.conversations()
                .set_agent_handling("v_1", &json!({ "state": "handled" })))
            .map(Returned::value)
        }),
        ("conversations", "generated_replies", |c| {
            (c.conversations().generated_replies("v_1", &Query::new())).map(Returned::value)
        }),
        ("conversations", "generate_reply", |c| {
            (c.conversations()
                .generate_reply("v_1", &json!({ "body": "draft" })))
            .map(Returned::value)
        }),
        ("conversations", "scheduled_messages", |c| {
            (c.conversations().scheduled_messages("v_1", &Query::new())).map(Returned::value)
        }),
        ("conversations", "schedule_message", |c| {
            (c.conversations().schedule_message(
                "v_1",
                &json!({ "body": "later", "sendAt": "2026-10-13T09:00:00Z" }),
            ))
            .map(Returned::value)
        }),
        ("conversations", "cancel_scheduled", |c| {
            (c.conversations().cancel_scheduled("s_1")).map(Returned::value)
        }),
        // -- tickets ------------------------------------------------------
        ("tickets", "list", |c| {
            (c.tickets().list(&Query::new())).map(Returned::page)
        }),
        ("tickets", "comments", |c| {
            (c.tickets().comments("t_1", &Query::new())).map(Returned::value)
        }),
        ("tickets", "for_conversation", |c| {
            (c.tickets().for_conversation("v_1")).map(Returned::value)
        }),
        ("tickets", "queues", |c| {
            (c.tickets().queues(&Query::new())).map(Returned::value)
        }),
        ("tickets", "merge", |c| {
            (c.tickets().merge("t_1", &json!({ "loserId": "t_2" }))).map(Returned::value)
        }),
        ("tickets", "split", |c| {
            (c.tickets().split("t_1", &json!({ "title": "split" }))).map(Returned::value)
        }),
        ("tickets", "bulk", |c| {
            (c.tickets()
                .bulk(&json!({ "ids": ["t_1"], "action": "status" })))
            .map(Returned::value)
        }),
        // -- macros -------------------------------------------------------
        ("macros", "list", |c| {
            (c.macros().list(&Query::new())).map(Returned::value)
        }),
        ("macros", "create", |c| {
            (c.macros()
                .create(&json!({ "name": "m", "scope": "workspace" })))
            .map(Returned::value)
        }),
        ("macros", "update", |c| {
            (c.macros().update("m_1", &json!({ "name": "m2" }))).map(Returned::value)
        }),
        ("macros", "delete", |c| {
            (c.macros().delete("m_1")).map(Returned::value)
        }),
        // -- kb -----------------------------------------------------------
        ("kb", "articles", |c| {
            (c.kb().articles(&Query::new())).map(Returned::page)
        }),
        ("kb", "article", |c| {
            (c.kb().article("how-to")).map(Returned::value)
        }),
        ("kb", "collections", |c| {
            (c.kb().collections(&Query::new())).map(Returned::value)
        }),
        // -- search -------------------------------------------------------
        ("search", "query", |c| {
            (c.search().query("refund", &Query::new())).map(Returned::value)
        }),
        // -- events -------------------------------------------------------
        ("events", "track", |c| {
            (c.events()
                .track(&json!({ "contactId": "c_1", "name": "evt" })))
            .map(Returned::value)
        }),
        ("events", "list", |c| {
            (c.events().list(&Query::new())).map(Returned::page)
        }),
        // -- webhooks -----------------------------------------------------
        ("webhooks", "deliveries", |c| {
            (c.webhooks().deliveries(&Query::new())).map(Returned::page)
        }),
        ("webhooks", "delivery", |c| {
            (c.webhooks().delivery("d_1")).map(Returned::value)
        }),
        ("webhooks", "redeliver", |c| {
            (c.webhooks().redeliver("d_1")).map(Returned::value)
        }),
        // -- reports ------------------------------------------------------
        ("reports", "overview", |c| {
            (c.reports().overview(&Query::new())).map(Returned::value)
        }),
        ("reports", "agents", |c| {
            (c.reports().agents(&Query::new())).map(Returned::value)
        }),
        ("reports", "conversations_by_day", |c| {
            (c.reports().conversations_by_day(&Query::new())).map(Returned::value)
        }),
        ("reports", "sla", |c| {
            (c.reports().sla(&Query::new())).map(Returned::value)
        }),
        // -- channels -----------------------------------------------------
        ("channels", "list", |c| {
            (c.channels().list(&Query::new())).map(Returned::value)
        }),
        ("channels", "get", |c| {
            (c.channels().get("ch_1")).map(Returned::value)
        }),
        // -- billing ------------------------------------------------------
        ("billing", "plans", |c| {
            (c.billing().plans(&Query::new())).map(Returned::value)
        }),
        ("billing", "usage", |c| {
            (c.billing().usage("ai.tokens", &Query::new())).map(Returned::value)
        }),
        // -- workspace ----------------------------------------------------
        ("workspace", "settings", |c| {
            (c.workspace().settings()).map(Returned::value)
        }),
        ("workspace", "labels", |c| {
            (c.workspace().labels(&Query::new())).map(Returned::value)
        }),
        ("workspace", "inboxes", |c| {
            (c.workspace().inboxes(&Query::new())).map(Returned::value)
        }),
        ("workspace", "team_members", |c| {
            (c.workspace().team_members("tm_1", &Query::new())).map(Returned::value)
        }),
        ("workspace", "agent_skills", |c| {
            (c.workspace().agent_skills(&Query::new())).map(Returned::value)
        }),
    ]
}

/// Invoke one method against a fake answering with **that route's own
/// declared shape**, and return the `METHOD /path` it issued plus the shape.
///
/// The fixture comes from [`shape_for`], a hand-written table — not one page
/// envelope for everything, and not the manifest's `shape` field either. A
/// fake driven by the manifest agrees with the manifest, so it proves
/// nothing; a fake that answers `{ items, nextCursor }` everywhere is what
/// let `macros().list` deserialise a bare array into a `Page` and raise
/// `ChatlyError::Decode` for real callers while this suite stayed green.
///
/// Three things are asserted here, in order of how hard they bite:
///
/// 1. the call SUCCEEDS against that body, so a method typed `Page` over a
///    route that does not send one fails — now that `Page` refuses a bare
///    array instead of reading it as an empty page;
/// 2. the value handed back really has the declared shape, so a method that
///    wraps or unwraps fails even though its request was perfect; and
/// 3. the fake's single row survived, so decoding into a default does not
///    pass as a correct decode of an empty collection.
fn observe(ns: &str, name: &str, invoke: Invocation) -> (String, String, BodyShape) {
    let shape =
        shape_for(ns, name).unwrap_or_else(|| panic!("no response shape recorded for {ns}.{name}"));
    let mock = MockTransport::new();
    // Three queued, so a method that issues more than one request is caught
    // by the count assertion rather than by the mock panicking.
    mock.push_shape(shape).push_shape(shape).push_shape(shape);
    let returned = invoke(&client(&mock)).unwrap_or_else(|err| {
        panic!(
            "{ns}.{name} failed against the shape the route really returns \
             ({shape:?}): {err}"
        )
    });
    assert_eq!(
        returned.shape(),
        shape,
        "{ns}.{name} was answered a {shape:?} body and handed back a {:?}",
        returned.shape()
    );
    assert!(
        returned.carries_the_fixture_row(),
        "{ns}.{name} dropped the row the fake sent: {returned:?}"
    );

    let requests = mock.requests();
    assert_eq!(
        requests.len(),
        1,
        "each locked method must issue exactly one request, got {}",
        requests.len()
    );
    let request = &requests[0];
    let after_host = request
        .url
        .strip_prefix(chatly::DEFAULT_BASE_URL)
        .unwrap_or(&request.url);
    let path = after_host
        .split('?')
        .next()
        .unwrap_or(after_host)
        .to_owned();
    (request.method.clone(), path, shape)
}

#[test]
fn the_surface_lock_is_regenerated_and_matches_the_manifest() {
    let manifest_file = manifest_path();
    let raw = std::fs::read_to_string(&manifest_file).unwrap_or_else(|err| {
        panic!(
            "cannot read the surface manifest at {}: {err}. \
             This test is the SDK's parity evidence and must not be skipped.",
            manifest_file.display()
        )
    });
    let manifest: Manifest = serde_json::from_str(&raw).expect("surface.json is valid JSON");

    let table = invocations();

    // 1. The table and the manifest must name the same 55 methods. Checked
    //    both ways, so neither a new manifest entry nor a stale closure can
    //    slip through.
    let manifest_keys: BTreeMap<(String, String), &ManifestEntry> = manifest
        .entries
        .iter()
        .map(|e| ((e.ns.clone(), e.name.clone()), e))
        .collect();
    let table_keys: BTreeMap<(String, String), Invocation> = table
        .iter()
        .map(|(ns, name, f)| (((*ns).to_owned(), (*name).to_owned()), *f))
        .collect();

    assert_eq!(
        table.len(),
        table_keys.len(),
        "duplicate (ns, name) in the invocation table"
    );
    let missing: Vec<_> = manifest_keys
        .keys()
        .filter(|k| !table_keys.contains_key(*k))
        .collect();
    assert!(
        missing.is_empty(),
        "manifest entries with no invocation here (unimplemented): {missing:?}"
    );
    let extra: Vec<_> = table_keys
        .keys()
        .filter(|k| !manifest_keys.contains_key(*k))
        .collect();
    assert!(
        extra.is_empty(),
        "invocations with no manifest entry (surface drift): {extra:?}"
    );

    // 2. Execute every method and record what it actually issued.
    let mut observed: Vec<Entry> = Vec::with_capacity(table_keys.len());
    let mut mismatches: Vec<String> = Vec::new();
    for ((ns, name), invoke) in &table_keys {
        let expected = manifest_keys
            .get(&(ns.clone(), name.clone()))
            .expect("checked above");
        let (method, path, shape) = observe(ns, name, *invoke);
        let normalised = normalise(&path, &expected.path);
        if method != expected.http || normalised != expected.path {
            mismatches.push(format!(
                "{ns}.{name}: issued `{method} {normalised}`, manifest says `{} {}`",
                expected.http, expected.path
            ));
        }
        observed.push(Entry {
            ns: ns.clone(),
            name: name.clone(),
            http: method,
            path: normalised,
            shape: shape.name().to_owned(),
        });
    }
    assert!(
        mismatches.is_empty(),
        "methods that do not issue what the manifest says:\n  {}",
        mismatches.join("\n  ")
    );

    // 2b. Every method must have a response shape on BOTH sides — declared
    //     locally in `tests/common/mod.rs`, and present in the manifest —
    //     and `paged` must agree with it in both directions.
    //
    //     Both directions is new, and it is only now true. `paged` used to
    //     mean "this SDK offers an auto-paginator" rather than "the wire
    //     shape is a page", and three routes that answer page envelopes —
    //     `contacts.journey`, `conversations.unattended`, `search.query` —
    //     carried no flag. The manifest fixed that on 2026-10-12: `paged`
    //     and `shape: "page"` are the same fact, all ten of them, and this
    //     SDK has a walker for every one (`tests/surface_shapes.rs` asserts
    //     that set exactly). So a route that loses either flag fails here.
    //
    //     The comparison of the two shape columns is step 5, not this: a
    //     wrong shape is the same class of failure as a wrong verb, so it is
    //     diffed with the verb and the path rather than on its own.
    let mut shape_problems: Vec<String> = Vec::new();
    for ((ns, name), entry) in &manifest_keys {
        let Some(shape) = shape_for(ns, name) else {
            shape_problems.push(format!(
                "{ns}.{name}: no response shape recorded in tests/common/mod.rs"
            ));
            continue;
        };
        if entry.shape.is_none() {
            shape_problems.push(format!(
                "{ns}.{name}: the manifest entry carries no `shape`. It is not a \
                 default — see `$shapeMeans` in surface.json"
            ));
        }
        if entry.paged == Some(true) && shape != BodyShape::Page {
            shape_problems.push(format!(
                "{ns}.{name}: manifest says paged, but its response shape is {shape:?}"
            ));
        }
        if entry.paged != Some(true) && shape == BodyShape::Page {
            shape_problems.push(format!(
                "{ns}.{name}: answers a page envelope but is not marked `paged` — \
                 a walker needs a cursor and the two fields are one fact"
            ));
        }
    }
    assert!(
        shape_problems.is_empty(),
        "manifest / response-shape problems:\n  {}",
        shape_problems.join("\n  ")
    );

    // 2c. Anti-vacuity floor. If the manifest ever parses to zero paged
    //     entries, every check above still passes and the auto-paginator is
    //     no longer exercised by any of them.
    //
    //     Ten, and it was 7 here until 2026-10-12. Not because the surface
    //     grew: `contacts.journey`, `conversations.unattended` and
    //     `search.query` always answered pages and the manifest had simply
    //     never flagged them, so a floor set from the flags was three short
    //     of the truth. Raise this when a genuinely paginated route is
    //     added; do not lower it to make a failing parse go away.
    let paged_count = manifest
        .entries
        .iter()
        .filter(|e| e.paged == Some(true))
        .count();
    assert!(
        paged_count >= 10,
        "only {paged_count} paged manifest entries; expected at least 10. \
         If the manifest really lost one, say which and why; if it did not, \
         the `paged` flag has stopped parsing."
    );

    // 3. Sorted by ns then name. `table_keys` is a BTreeMap, so `observed`
    //    is already in that order; sorting again makes the guarantee local.
    observed.sort_by(|a, b| (&a.ns, &a.name).cmp(&(&b.ns, &b.name)));

    // 4. Write the lock.
    let lock = Lock {
        sdk: "rust",
        generated_by: "tests/surface_lock.rs",
        entries: observed.clone(),
    };
    let mut rendered = serde_json::to_string_pretty(&lock).expect("the lock serialises");
    rendered.push('\n');
    let lock_file = package_dir().join("surface.lock.json");
    std::fs::write(&lock_file, &rendered)
        .unwrap_or_else(|err| panic!("cannot write {}: {err}", lock_file.display()));

    // 5. Assert the lock equals the manifest.
    let mut expected: Vec<Entry> = manifest
        .entries
        .iter()
        .map(|e| Entry {
            ns: e.ns.clone(),
            name: e.name.clone(),
            http: e.http.clone(),
            path: e.path.clone(),
            // Checked to be present in 2b, so an absent one has already
            // failed with a message that says so rather than diffing as "".
            shape: e.shape.clone().unwrap_or_default(),
        })
        .collect();
    expected.sort_by(|a, b| (&a.ns, &a.name).cmp(&(&b.ns, &b.name)));

    assert_eq!(
        observed.len(),
        expected.len(),
        "the lock has {} entries, the manifest has {}",
        observed.len(),
        expected.len()
    );
    assert_eq!(
        observed, expected,
        "the generated lock does not match the manifest"
    );

    // 6. And the file on disk round-trips to the same thing, so a lock
    //    committed from a stale run is not mistaken for a fresh one.
    let reread: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&lock_file).expect("lock is readable"))
            .expect("lock is valid JSON");
    assert_eq!(reread["sdk"], "rust");
    assert_eq!(reread["generatedBy"], "tests/surface_lock.rs");
    assert_eq!(
        reread["entries"].as_array().map(Vec::len),
        Some(expected.len())
    );

    println!(
        "surface.lock.json regenerated: {} entries, all matching {}",
        observed.len(),
        manifest_file.display()
    );
}

/// The manifest's base URL and this crate's default must agree. If they ever
/// diverge, every path in the lock is right and every request still goes to
/// the wrong host.
#[test]
fn the_manifest_base_url_is_this_crates_default() {
    let raw = std::fs::read_to_string(manifest_path()).expect("manifest is readable");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("manifest is JSON");
    assert_eq!(
        parsed["baseUrl"].as_str(),
        Some(chatly::DEFAULT_BASE_URL),
        "manifest baseUrl and DEFAULT_BASE_URL disagree"
    );
}

/// The manifest's two helpers exist as free functions with the contract's
/// names. They take no client, so no invocation table can cover them.
#[test]
fn the_manifest_helpers_exist() {
    let raw = std::fs::read_to_string(manifest_path()).expect("manifest is readable");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("manifest is JSON");
    let names: Vec<&str> = parsed["helpers"]
        .as_array()
        .map(|a| a.iter().filter_map(|h| h["name"].as_str()).collect())
        .unwrap_or_default();
    assert!(names.contains(&"identity_hash"), "{names:?}");
    assert!(names.contains(&"verify_webhook_signature"), "{names:?}");

    // Reached by name, so these lines fail to compile if either is renamed.
    assert!(chatly::identity_hash("sek_test_secret", "user_42").is_ok());
    assert!(!chatly::verify_webhook_signature(
        b"{}",
        "t=1,v1=ab",
        "whsec_test"
    ));
}
