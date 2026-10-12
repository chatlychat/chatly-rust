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
//! The test fails, rather than skipping, when:
//!
//! - the manifest has an entry with no closure here (a new method to write),
//! - this file has a closure with no manifest entry (surface drift),
//! - any method issues a different verb or path than the manifest says,
//! - any method issues more or fewer than one request, or
//! - the regenerated lock differs from the manifest.
//!
//! Run with `cargo test --test surface_lock`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use chatly::{ChatlyClient, Query};
use common::{client, shape_for, BodyShape, MockTransport};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// One manifest / lock entry. Field order here is the field order written to
/// `surface.lock.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    ns: String,
    name: String,
    http: String,
    path: String,
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

/// One method call, discarding the decoded value but **keeping the error**.
///
/// Returning `Result` rather than `()` is load-bearing: these closures used
/// to write `let _ = …`, which threw the error away, so a route answered with
/// a shape its own return type could not decode still looked fine here. That
/// is half of why `macros().list` could raise `ChatlyError::Decode` for real
/// callers with this suite green.
type Invocation = fn(&ChatlyClient) -> Result<(), chatly::ChatlyError>;

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
        ("root", "whoami", |c| (c.whoami()).map(|_| ())),
        // -- contacts -----------------------------------------------------
        ("contacts", "list", |c| (c.contacts().list(&Query::new())).map(|_| ())),
        ("contacts", "get", |c| (c.contacts().get("c_1")).map(|_| ())),
        ("contacts", "upsert", |c| (c.contacts().upsert(&json!({}))).map(|_| ())),
        ("contacts", "bulk_upsert", |c| (c.contacts().bulk_upsert(&json!({ "contacts": [] }))).map(|_| ())),
        ("contacts", "merge", |c| (c.contacts().merge("c_1", "c_2")).map(|_| ())),
        ("contacts", "add_tags", |c| (c.contacts().add_tags("c_1", &["vip"])).map(|_| ())),
        ("contacts", "remove_tag", |c| (c.contacts().remove_tag("c_1", "vip")).map(|_| ())),
        ("contacts", "unsubscribe", |c| (c.contacts().unsubscribe("c_1", true, None)).map(|_| ())),
        ("contacts", "journey", |c| (c.contacts().journey("c_1", &Query::new())).map(|_| ())),
        // -- conversations ------------------------------------------------
        ("conversations", "list", |c| (c.conversations().list(&Query::new())).map(|_| ())),
        ("conversations", "get", |c| (c.conversations().get("v_1")).map(|_| ())),
        ("conversations", "messages", |c| (c.conversations().messages("v_1", &Query::new())).map(|_| ())),
        ("conversations", "send", |c| (c.conversations().send("v_1", &json!({ "body": "hi" }))).map(|_| ())),
        ("conversations", "unattended", |c| (c.conversations().unattended(&Query::new())).map(|_| ())),
        ("conversations", "agent_handling", |c| (c.conversations().agent_handling("v_1")).map(|_| ())),
        ("conversations", "set_agent_handling", |c| (c .conversations() .set_agent_handling("v_1", &json!({ "state": "handled" }))).map(|_| ())),
        ("conversations", "generated_replies", |c| (c.conversations().generated_replies("v_1", &Query::new())).map(|_| ())),
        ("conversations", "generate_reply", |c| (c .conversations() .generate_reply("v_1", &json!({ "body": "draft" }))).map(|_| ())),
        ("conversations", "scheduled_messages", |c| (c.conversations().scheduled_messages("v_1", &Query::new())).map(|_| ())),
        ("conversations", "schedule_message", |c| (c.conversations().schedule_message( "v_1", &json!({ "body": "later", "sendAt": "2026-10-13T09:00:00Z" }), )).map(|_| ())),
        ("conversations", "cancel_scheduled", |c| (c.conversations().cancel_scheduled("s_1")).map(|_| ())),
        // -- tickets ------------------------------------------------------
        ("tickets", "list", |c| (c.tickets().list(&Query::new())).map(|_| ())),
        ("tickets", "comments", |c| (c.tickets().comments("t_1", &Query::new())).map(|_| ())),
        ("tickets", "for_conversation", |c| (c.tickets().for_conversation("v_1")).map(|_| ())),
        ("tickets", "queues", |c| (c.tickets().queues(&Query::new())).map(|_| ())),
        ("tickets", "merge", |c| (c.tickets().merge("t_1", &json!({ "loserId": "t_2" }))).map(|_| ())),
        ("tickets", "split", |c| (c.tickets().split("t_1", &json!({ "title": "split" }))).map(|_| ())),
        ("tickets", "bulk", |c| (c .tickets() .bulk(&json!({ "ids": ["t_1"], "action": "status" }))).map(|_| ())),
        // -- macros -------------------------------------------------------
        ("macros", "list", |c| (c.macros().list(&Query::new())).map(|_| ())),
        ("macros", "create", |c| (c .macros() .create(&json!({ "name": "m", "scope": "workspace" }))).map(|_| ())),
        ("macros", "update", |c| (c.macros().update("m_1", &json!({ "name": "m2" }))).map(|_| ())),
        ("macros", "delete", |c| (c.macros().delete("m_1")).map(|_| ())),
        // -- kb -----------------------------------------------------------
        ("kb", "articles", |c| (c.kb().articles(&Query::new())).map(|_| ())),
        ("kb", "article", |c| (c.kb().article("how-to")).map(|_| ())),
        ("kb", "collections", |c| (c.kb().collections(&Query::new())).map(|_| ())),
        // -- search -------------------------------------------------------
        ("search", "query", |c| (c.search().query("refund", &Query::new())).map(|_| ())),
        // -- events -------------------------------------------------------
        ("events", "track", |c| (c .events() .track(&json!({ "contactId": "c_1", "name": "evt" }))).map(|_| ())),
        ("events", "list", |c| (c.events().list(&Query::new())).map(|_| ())),
        // -- webhooks -----------------------------------------------------
        ("webhooks", "deliveries", |c| (c.webhooks().deliveries(&Query::new())).map(|_| ())),
        ("webhooks", "delivery", |c| (c.webhooks().delivery("d_1")).map(|_| ())),
        ("webhooks", "redeliver", |c| (c.webhooks().redeliver("d_1")).map(|_| ())),
        // -- reports ------------------------------------------------------
        ("reports", "overview", |c| (c.reports().overview(&Query::new())).map(|_| ())),
        ("reports", "agents", |c| (c.reports().agents(&Query::new())).map(|_| ())),
        ("reports", "conversations_by_day", |c| (c.reports().conversations_by_day(&Query::new())).map(|_| ())),
        ("reports", "sla", |c| (c.reports().sla(&Query::new())).map(|_| ())),
        // -- channels -----------------------------------------------------
        ("channels", "list", |c| (c.channels().list(&Query::new())).map(|_| ())),
        ("channels", "get", |c| (c.channels().get("ch_1")).map(|_| ())),
        // -- billing ------------------------------------------------------
        ("billing", "plans", |c| (c.billing().plans(&Query::new())).map(|_| ())),
        ("billing", "usage", |c| (c.billing().usage(&Query::new())).map(|_| ())),
        // -- workspace ----------------------------------------------------
        ("workspace", "settings", |c| (c.workspace().settings()).map(|_| ())),
        ("workspace", "labels", |c| (c.workspace().labels(&Query::new())).map(|_| ())),
        ("workspace", "inboxes", |c| (c.workspace().inboxes(&Query::new())).map(|_| ())),
        ("workspace", "team_members", |c| (c.workspace().team_members("tm_1", &Query::new())).map(|_| ())),
        ("workspace", "agent_skills", |c| (c.workspace().agent_skills(&Query::new())).map(|_| ())),
    ]
}

/// Invoke one method and return the `METHOD /path` it issued, with the query
/// string dropped.
///
/// The scripted response is the shape **that route really returns**, from
/// [`shape_for`] — not a page envelope for everything. That matters: a
/// one-size-fits-all page envelope is what let `macros().list` deserialise
/// into a `Page` and raise `ChatlyError::Decode` for real callers while this
/// suite stayed green. The method's own `Result` is checked below, so a
/// shape its return type cannot decode now fails here.
fn observe(ns: &str, name: &str, invoke: Invocation) -> (String, String) {
    let shape = shape_for(ns, name)
        .unwrap_or_else(|| panic!("no response shape recorded for {ns}.{name}"));
    let body = shape.body();
    let mock = MockTransport::new();
    // Three queued, so a method that issues more than one request is caught
    // by the count assertion rather than by the mock panicking.
    mock.push_json(200, body)
        .push_json(200, body)
        .push_json(200, body);
    let outcome = invoke(&client(&mock));
    assert!(
        outcome.is_ok(),
        "{ns}.{name} failed against the shape the route really returns \
         ({shape:?}): {}",
        outcome.err().map_or_else(String::new, |e| e.to_string())
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
    (request.method.clone(), path)
}

/// Fold concrete path segments back to the manifest's `{...}` names, by
/// position.
///
/// Every non-parameter segment is asserted equal, so a method that hits
/// `/v1/contact/{id}` instead of `/v1/contacts/{id}` fails here rather than
/// being normalised into agreement.
fn normalise(actual: &str, manifest: &str) -> String {
    let actual_segments: Vec<&str> = actual.split('/').collect();
    let manifest_segments: Vec<&str> = manifest.split('/').collect();
    if actual_segments.len() != manifest_segments.len() {
        // Return the actual path untouched; the caller's comparison reports
        // it, which is more useful than a panic here.
        return actual.to_owned();
    }
    manifest_segments
        .iter()
        .zip(actual_segments.iter())
        .map(|(expected, got)| {
            if expected.starts_with('{') && expected.ends_with('}') {
                (*expected).to_owned()
            } else {
                (*got).to_owned()
            }
        })
        .collect::<Vec<String>>()
        .join("/")
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
        let (method, path) = observe(ns, name, *invoke);
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
        });
    }
    assert!(
        mismatches.is_empty(),
        "methods that do not issue what the manifest says:\n  {}",
        mismatches.join("\n  ")
    );

    // 2b. The shape table and the manifest must agree about what is paged.
    //
    //     Both directions. A manifest entry marked `paged` whose shape here
    //     is not `Page` means the fake is lying to a `Page`-typed method;
    //     a `Page` shape here with no `paged` flag means this SDK is
    //     deserialising an envelope the route does not send. That second
    //     case is the `macros.list` bug, and this is the assertion that
    //     would have caught it.
    let mut shape_disagreements: Vec<String> = Vec::new();
    for ((ns, name), entry) in &manifest_keys {
        let Some(shape) = shape_for(ns, name) else {
            shape_disagreements.push(format!("{ns}.{name}: no response shape recorded"));
            continue;
        };
        let manifest_paged = entry.paged.unwrap_or(false);
        let shape_paged = shape == BodyShape::Page;
        if manifest_paged != shape_paged {
            shape_disagreements.push(format!(
                "{ns}.{name}: manifest paged={manifest_paged}, response shape={shape:?}"
            ));
        }
    }
    assert!(
        shape_disagreements.is_empty(),
        "the manifest and the response-shape table disagree:\n  {}",
        shape_disagreements.join("\n  ")
    );

    // 2c. Anti-vacuity floor. If the manifest ever parses to zero paged
    //     entries, every check above still passes and the auto-paginator is
    //     no longer exercised by any of them.
    //
    //     7, not 8: `macros.list` was removed from the paged set because its
    //     handler is `@PaginationExempt` and answers a bare array — the
    //     route changed category, the spec did not stop parsing. Raise this
    //     when a genuinely paginated route is added; do not lower it to make
    //     a failing parse go away.
    let paged_count = manifest.entries.iter().filter(|e| e.paged == Some(true)).count();
    assert!(
        paged_count >= 7,
        "only {paged_count} paged manifest entries; expected at least 7 \
         (contacts.list, conversations.list, conversations.messages, \
         tickets.list, kb.articles, events.list, webhooks.deliveries). \
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
