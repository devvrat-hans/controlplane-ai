//! Contract guard — "No LLM in the decision path" (`AGENTS.md` rule 4,
//! `docs/analysis/laya-integration-plan.md` §7 and §13).
//!
//! The fast path is only guaranteed to be LLM-free while there is no *route* from it to
//! the judge. Two things could create one, and neither stays visible in code review once
//! the crate grows:
//!
//!   1. a dependency on `shadow-analysis` (which owns the Laya client), or
//!   2. a source file that talks to the judge directly over HTTP.
//!
//! This asserts both structurally, so the invariant fails the build rather than a
//! reviewer's memory. It also guards the same invariant from the other side: the decision
//! engine must **consume** judge verdicts, never **call** the judge — that is what keeps
//! the final verdict deterministic given the scores and the policy thresholds, and it is
//! why `laya-` may appear there as a verdict-name prefix but an HTTP client may not.
//!
//! If this test fails, the failure message names the exact file and token. Do not relax
//! the list to make it pass — move the code into `shadow-analysis`.

use std::fs;
use std::path::{Path, PathBuf};

/// Ways to reach the judge over the wire. Forbidden in every crate that is not
/// `shadow-analysis`.
const WIRE_MARKERS: [&str; 6] = [
    "shadow-analysis",
    "shadow_analysis",
    "laya_client",
    "laya-serve",
    "systemone",
    "reqwest",
];

/// The fast path must not even know the judge exists: the *name* is enough to couple it.
const FAST_PATH_EXTRA_MARKERS: [&str; 2] = ["laya", "jev"];

fn workspace_service(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate lives in services/")
        .join(name)
}

/// Every `.rs` file under `dir`, recursively, without pulling in a walkdir dependency.
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Strip line and block comments, keeping string literals intact.
///
/// Prose is not a route to the judge: the decision engine's doc comments legitimately name
/// the shadow-path convention it mirrors, but a comment cannot make an HTTP call. Only
/// code and string literals can, so only those are scanned.
fn strip_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');

        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push(c);
            }
            i += 1;
        } else if in_block_comment {
            if c == '*' && next == '/' {
                in_block_comment = false;
                i += 2;
            } else {
                if c == '\n' {
                    out.push(c);
                }
                i += 1;
            }
        } else if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(escaped) = chars.get(i + 1) {
                    out.push(*escaped);
                    i += 2;
                    continue;
                }
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
        } else if c == '/' && next == '/' {
            in_line_comment = true;
            i += 2;
        } else if c == '/' && next == '*' {
            in_block_comment = true;
            i += 2;
        } else {
            if c == '"' {
                in_string = true;
            }
            out.push(c);
            i += 1;
        }
    }

    out
}

/// Dependency names declared in a manifest, ignoring comments.
fn declared_dependencies(manifest: &Path) -> Vec<String> {
    let text = fs::read_to_string(manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));
    let mut names = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            names.push(name.trim().to_string());
        }
    }
    names
}

fn assert_clean(crate_dir: &Path, what: &str, markers: &[&str]) {
    for dep in declared_dependencies(&crate_dir.join("Cargo.toml")) {
        for marker in markers {
            assert!(
                !dep.to_lowercase().contains(marker),
                "{what} depends on `{dep}`, which reaches the judge. The judge may only be \
                 reached from shadow-analysis (AGENTS.md rule 4)."
            );
        }
    }

    let mut sources = Vec::new();
    rust_sources(&crate_dir.join("src"), &mut sources);
    assert!(
        !sources.is_empty(),
        "found no sources under {}",
        crate_dir.display()
    );

    for file in &sources {
        let code = strip_comments(&fs::read_to_string(file).expect("read source"));
        let lowered = code.to_lowercase();
        for marker in markers {
            assert!(
                !lowered.contains(marker),
                "{} mentions `{marker}`. {what} must stay LLM-free; the judge is \
                 shadow-path only.",
                file.display()
            );
        }
    }
}

#[test]
fn the_fast_path_has_no_route_to_the_judge() {
    let mut markers = WIRE_MARKERS.to_vec();
    markers.extend_from_slice(&FAST_PATH_EXTRA_MARKERS);
    assert_clean(&workspace_service("fast-path"), "the fast path", &markers);
}

#[test]
fn the_decision_engine_consumes_verdicts_and_never_calls_the_judge() {
    let decision = workspace_service("decision");
    assert_clean(&decision, "the decision engine", &WIRE_MARKERS);

    // Belt and braces: nothing in the dependency set can make an outbound HTTP call, so
    // the fusion is a pure function of the scores and the policy thresholds.
    for dep in declared_dependencies(&decision.join("Cargo.toml")) {
        for client in ["reqwest", "hyper", "ureq", "curl"] {
            assert!(
                !dep.to_lowercase().contains(client),
                "the decision engine depends on `{dep}`; it must not be able to call the \
                 judge itself (AGENTS.md rule 4)."
            );
        }
    }
}

/// The guard is only worth having if it can fail: prove the detector works on a real
/// token rather than trusting an empty match.
#[test]
fn the_guard_detects_a_leak_when_one_exists() {
    let leaked = workspace_service("shadow-analysis");
    let mut sources = Vec::new();
    rust_sources(&leaked.join("src"), &mut sources);
    let names: Vec<String> = sources
        .iter()
        .map(|p| strip_comments(&fs::read_to_string(p).expect("read source")).to_lowercase())
        .collect();
    assert!(
        names.iter().any(|t| t.contains("laya_client")),
        "expected the judge client to be reachable from shadow-analysis"
    );
}
