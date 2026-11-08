//! Networks shaping where things go: a source placed so its runs keep within `limit`, and
//! fixtures joined where they meet a wall the network can run in.

use serde_json::{json, Value};

use super::testkit::{messages, parts, pinned_flat_with, run};
use super::Solution;

/// A stack in the bath's walls (near `near`), a drain on its floor, the run at most `limit`.
fn plumbing(near: &str, limit: f64) -> Value {
    json!({
        "parts": {
            "stack": { "mount": "cavity", "width": 0.1, "depth": 0.1 }
        },
        "place": [
            { "place": "one", "part": "stack", "spaces": "#wet", "near": near, "wall": { "cavity": 0.12 } }
        ],
        "connect": [{ "from": "stack", "to": "#drain", "draw": "routed", "limit": limit }]
    })
}

fn run_length(s: &Solution) -> f64 {
    s.layout.circuits.iter().filter(|c| c.service == "plumbing").map(|c| c.length).sum()
}

/// The pinned flat with the stack's plumbing, and a drain pinned in a corner of the bath.
fn with_drain(near: &str, limit: f64) -> Solution {
    let mut v = pinned_flat_with(json!({ "plumbing": plumbing(near, limit) }));
    v["spaces"]["bath"]["plan"] = json!({
        "spaces": { "drain": { "kind": "fixture", "width": 0.3, "depth": 0.3, "at": [0, 0], "tags": ["drain"] } }
    });
    run(v)
}

#[test]
fn a_source_keeps_its_runs_within_the_limit() {
    // Near the facade, the stack is as far along it as it gets from the drain.
    let free = with_drain("facade", 100.0);
    let far = run_length(&free);
    assert!(far > 2.5, "{far} {:?}", messages(&free));
    // With a limit, the place nearest the facade from which the drain is within it.
    for limit in [2.0, 1.2] {
        let s = with_drain("facade", limit);
        assert_eq!(s.report.unmet, 0, "{limit}: {:?}", messages(&s));
        let length = run_length(&s);
        assert!(length <= limit + 1e-9 && length > limit - 0.2, "{limit}: {length}");
        assert_eq!(parts(&s, "stack").len(), 1);
    }
}
