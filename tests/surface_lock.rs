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
use common::{client, MockTransport};
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

/// Every method in the manifest, as `(ns, name, invocation)`.
///
/// The closures capture nothing, so they coerce to `fn` pointers and this is
/// a plain table. Arguments are throwaway; only the request that comes out
/// matters. Each closure must issue **exactly one** request — `add_tags` is
/// given a single tag for that reason, since the underlying route takes one
/// tag per call.
#[allow(clippy::type_complexity)]
fn invocations() -> Vec<(&'static str, &'static str, fn(&ChatlyClient))> {
    vec![
        ("root", "whoami", |c| {
            let _ = c.whoami();
        }),
        // -- contacts -----------------------------------------------------
        ("contacts", "list", |c| {
            let _ = c.contacts().list(&Query::new());
        }),
        ("contacts", "get", |c| {
            let _ = c.contacts().get("c_1");
        }),
        ("contacts", "upsert", |c| {
            let _ = c.contacts().upsert(&json!({}));
        }),
        ("contacts", "bulk_upsert", |c| {
            let _ = c.contacts().bulk_upsert(&json!({ "contacts": [] }));
        }),
        ("contacts", "merge", |c| {
            let _ = c.contacts().merge("c_1", "c_2");
        }),
        ("contacts", "add_tags", |c| {
            let _ = c.contacts().add_tags("c_1", &["vip"]);
        }),
        ("contacts", "remove_tag", |c| {
            let _ = c.contacts().remove_tag("c_1", "vip");
        }),
        ("contacts", "unsubscribe", |c| {
            let _ = c.contacts().unsubscribe("c_1", true, None);
        }),
        ("contacts", "journey", |c| {
            let _ = c.contacts().journey("c_1", &Query::new());
        }),
        // -- conversations ------------------------------------------------
        ("conversations", "list", |c| {
            let _ = c.conversations().list(&Query::new());
        }),
        ("conversations", "get", |c| {
            let _ = c.conversations().get("v_1");
        }),
        ("conversations", "messages", |c| {
            let _ = c.conversations().messages("v_1", &Query::new());
        }),
        ("conversations", "send", |c| {
            let _ = c.conversations().send("v_1", &json!({ "body": "hi" }));
        }),
        ("conversations", "unattended", |c| {
            let _ = c.conversations().unattended(&Query::new());
        }),
        ("conversations", "agent_handling", |c| {
            let _ = c.conversations().agent_handling("v_1");
        }),
        ("conversations", "set_agent_handling", |c| {
            let _ = c
                .conversations()
                .set_agent_handling("v_1", &json!({ "state": "handled" }));
        }),
        ("conversations", "generated_replies", |c| {
            let _ = c.conversations().generated_replies("v_1", &Query::new());
        }),
        ("conversations", "generate_reply", |c| {
            let _ = c
                .conversations()
                .generate_reply("v_1", &json!({ "body": "draft" }));
        }),
        ("conversations", "scheduled_messages", |c| {
            let _ = c.conversations().scheduled_messages("v_1", &Query::new());
        }),
        ("conversations", "schedule_message", |c| {
            let _ = c.conversations().schedule_message(
                "v_1",
                &json!({ "body": "later", "sendAt": "2026-10-13T09:00:00Z" }),
            );
        }),
        ("conversations", "cancel_scheduled", |c| {
            let _ = c.conversations().cancel_scheduled("s_1");
        }),
        // -- tickets ------------------------------------------------------
        ("tickets", "list", |c| {
            let _ = c.tickets().list(&Query::new());
        }),
        ("tickets", "comments", |c| {
            let _ = c.tickets().comments("t_1", &Query::new());
        }),
        ("tickets", "for_conversation", |c| {
            let _ = c.tickets().for_conversation("v_1");
        }),
        ("tickets", "queues", |c| {
            let _ = c.tickets().queues(&Query::new());
        }),
        ("tickets", "merge", |c| {
            let _ = c.tickets().merge("t_1", &json!({ "loserId": "t_2" }));
        }),
        ("tickets", "split", |c| {
            let _ = c.tickets().split("t_1", &json!({ "title": "split" }));
        }),
        ("tickets", "bulk", |c| {
            let _ = c
                .tickets()
                .bulk(&json!({ "ids": ["t_1"], "action": "status" }));
        }),
        // -- macros -------------------------------------------------------
        ("macros", "list", |c| {
            let _ = c.macros().list(&Query::new());
        }),
        ("macros", "create", |c| {
            let _ = c
                .macros()
                .create(&json!({ "name": "m", "scope": "workspace" }));
        }),
        ("macros", "update", |c| {
            let _ = c.macros().update("m_1", &json!({ "name": "m2" }));
        }),
        ("macros", "delete", |c| {
            let _ = c.macros().delete("m_1");
        }),
        // -- kb -----------------------------------------------------------
        ("kb", "articles", |c| {
            let _ = c.kb().articles(&Query::new());
        }),
        ("kb", "article", |c| {
            let _ = c.kb().article("how-to");
        }),
        ("kb", "collections", |c| {
            let _ = c.kb().collections(&Query::new());
        }),
        // -- search -------------------------------------------------------
        ("search", "query", |c| {
            let _ = c.search().query("refund", &Query::new());
        }),
        // -- events -------------------------------------------------------
        ("events", "track", |c| {
            let _ = c
                .events()
                .track(&json!({ "contactId": "c_1", "name": "evt" }));
        }),
        ("events", "list", |c| {
            let _ = c.events().list(&Query::new());
        }),
        // -- webhooks -----------------------------------------------------
        ("webhooks", "deliveries", |c| {
            let _ = c.webhooks().deliveries(&Query::new());
        }),
        ("webhooks", "delivery", |c| {
            let _ = c.webhooks().delivery("d_1");
        }),
        ("webhooks", "redeliver", |c| {
            let _ = c.webhooks().redeliver("d_1");
        }),
        // -- reports ------------------------------------------------------
        ("reports", "overview", |c| {
            let _ = c.reports().overview(&Query::new());
        }),
        ("reports", "agents", |c| {
            let _ = c.reports().agents(&Query::new());
        }),
        ("reports", "conversations_by_day", |c| {
            let _ = c.reports().conversations_by_day(&Query::new());
        }),
        ("reports", "sla", |c| {
            let _ = c.reports().sla(&Query::new());
        }),
        // -- channels -----------------------------------------------------
        ("channels", "list", |c| {
            let _ = c.channels().list(&Query::new());
        }),
        ("channels", "get", |c| {
            let _ = c.channels().get("ch_1");
        }),
        // -- billing ------------------------------------------------------
        ("billing", "plans", |c| {
            let _ = c.billing().plans(&Query::new());
        }),
        ("billing", "usage", |c| {
            let _ = c.billing().usage(&Query::new());
        }),
        // -- workspace ----------------------------------------------------
        ("workspace", "settings", |c| {
            let _ = c.workspace().settings();
        }),
        ("workspace", "labels", |c| {
            let _ = c.workspace().labels(&Query::new());
        }),
        ("workspace", "inboxes", |c| {
            let _ = c.workspace().inboxes(&Query::new());
        }),
        ("workspace", "team_members", |c| {
            let _ = c.workspace().team_members("tm_1", &Query::new());
        }),
        ("workspace", "agent_skills", |c| {
            let _ = c.workspace().agent_skills(&Query::new());
        }),
    ]
}

/// A body that decodes both as a bare JSON object and as a `Page`, so one
/// scripted response serves every method regardless of its return type.
const ANY_BODY: &str = r#"{"items":[],"nextCursor":null}"#;

/// Invoke one method and return the `METHOD /path` it issued, with the query
/// string dropped.
fn observe(invoke: fn(&ChatlyClient)) -> (String, String) {
    let mock = MockTransport::new();
    // Three queued, so a method that issues more than one request is caught
    // by the count assertion rather than by the mock panicking.
    mock.push_json(200, ANY_BODY)
        .push_json(200, ANY_BODY)
        .push_json(200, ANY_BODY);
    invoke(&client(&mock));

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
    let table_keys: BTreeMap<(String, String), fn(&ChatlyClient)> = table
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
        let (method, path) = observe(*invoke);
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
