//! Overlap detection integration tests.
//! These tests simulate the golden path from §17 Task 04:
//!   1. Human edits lines 44–67 of auth.ts
//!   2. Agent edits lines 52–71 of auth.ts
//!   3. detect_overlaps() returns exactly one OverlapEvent

use harmony_core::types::*;
use harmony_core::overlap::detect_overlaps;
use chrono::Utc;
use uuid::Uuid;

fn make_tag(actor: &str, start: u32, end: u32) -> ProvenanceTag {
    ProvenanceTag {
        id: Uuid::new_v4(),
        actor_id: ActorId(actor.to_string()),
        machine_name: "local".to_string(),
        machine_ip: "127.0.0.1".to_string(),
        actor_kind: if actor.starts_with("human:") { ActorKind::Human } else { ActorKind::Agent },
        task_id: None,
        task_prompt: None,
        timestamp: Utc::now(),
        file_path: "src/middleware/auth.ts".to_string(),
        region: TextRange { start_line: start, end_line: end, start_col: 0, end_col: 0 },
        mode: AgentMode::Shadow,
        diff_unified: String::new(),
        session_id: Uuid::new_v4(),
    }
}

#[test]
fn golden_path_overlap() {
    let human_tag = make_tag("human:awanish", 44, 67);
    let agent_tag = make_tag("agent:architect-01", 52, 71);
    let overlaps = detect_overlaps(&human_tag, &[agent_tag], 30);
    assert_eq!(overlaps.len(), 1, "Should detect exactly 1 overlap");
    assert_eq!(overlaps[0].file_path, "src/middleware/auth.ts");
}

#[test]
fn no_overlap_same_actor() {
    let tag_a = make_tag("agent:coder-01", 44, 67);
    let tag_b = make_tag("agent:coder-01", 52, 71);
    let overlaps = detect_overlaps(&tag_a, &[tag_b], 30);
    assert_eq!(overlaps.len(), 0, "Same actor should not produce overlap");
}

#[test]
fn no_overlap_disjoint_regions() {
    let tag_a = make_tag("human:awanish", 10, 20);
    let tag_b = make_tag("agent:architect-01", 30, 40);
    let overlaps = detect_overlaps(&tag_a, &[tag_b], 30);
    assert_eq!(overlaps.len(), 0, "Non-overlapping regions should not produce overlap");
}

#[test]
fn boundary_overlap_same_line() {
    let tag_a = make_tag("human:awanish", 10, 20);
    let tag_b = make_tag("agent:coder-01", 20, 30);
    let overlaps = detect_overlaps(&tag_a, &[tag_b], 30);
    assert_eq!(overlaps.len(), 1, "Adjacent regions sharing a line should overlap");
}
