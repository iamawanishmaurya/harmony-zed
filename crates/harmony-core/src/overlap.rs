use crate::types::*;
use chrono::{Duration, Utc};
use uuid::Uuid;

/// Main entry point for overlap detection.
/// Call this after every ProvenanceTag is written to the DB.
/// Returns all new OverlapEvents found (may be empty).
pub fn detect_overlaps(
    new_tag: &ProvenanceTag,
    recent_tags: &[ProvenanceTag],
    window_minutes: u32,
) -> Vec<OverlapEvent> {
    let mut overlaps = Vec::new();
    let window = Duration::minutes(window_minutes as i64);

    for candidate in recent_tags {
        if is_real_overlap(new_tag, candidate, window) {
            let overlap = OverlapEvent {
                id: Uuid::new_v4(),
                file_path: new_tag.file_path.clone(),
                region_a: new_tag.region.clone(),
                region_b: candidate.region.clone(),
                change_a: new_tag.clone(),
                change_b: candidate.clone(),
                detected_at: Utc::now(),
                status: OverlapStatus::Pending,
            };
            overlaps.push(overlap);
        }
    }

    overlaps
}

/// Returns true if two provenance tags constitute a real overlap.
/// Rules:
///   1. Must be same file_path.
///   2. Must be different actor_ids.
///   3. Regions must overlap (TextRange::overlaps).
///   4. Timestamps must be within window_minutes of each other.
///   5. Must not already have a resolved OverlapEvent for this pair.
///      (Note: rule 5 requires DB check — the caller must filter already-resolved pairs)
fn is_real_overlap(
    a: &ProvenanceTag,
    b: &ProvenanceTag,
    window: Duration,
) -> bool {
    // Rule 1: Same file path
    if a.file_path != b.file_path {
        return false;
    }

    // Rule 2: Different actors
    if a.actor_id == b.actor_id {
        return false;
    }

    // Rule 3: Regions overlap
    if !a.region.overlaps(&b.region) {
        return false;
    }

    // Rule 4: Within time window
    let time_diff = if a.timestamp > b.timestamp {
        a.timestamp - b.timestamp
    } else {
        b.timestamp - a.timestamp
    };
    if time_diff > window {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_tag(
        actor_id: &str,
        file_path: &str,
        start_line: u32,
        end_line: u32,
        timestamp_offset_minutes: i64,
    ) -> ProvenanceTag {
        ProvenanceTag {
            id: Uuid::new_v4(),
            actor_id: ActorId(actor_id.to_string()),
            machine_name: "local".to_string(),
            machine_ip: "127.0.0.1".to_string(),
            actor_kind: if actor_id.starts_with("human:") {
                ActorKind::Human
            } else {
                ActorKind::Agent
            },
            task_id: None,
            task_prompt: None,
            timestamp: Utc::now() + Duration::minutes(timestamp_offset_minutes),
            file_path: file_path.to_string(),
            region: TextRange {
                start_line,
                end_line,
                start_col: 0,
                end_col: 0,
            },
            mode: AgentMode::Shadow,
            diff_unified: String::new(),
            session_id: Uuid::new_v4(),
        }
    }

    #[test]
    fn test_overlapping_regions_different_actors() {
        // Test 1: Two tags same file, overlapping region, different actors → 1 OverlapEvent
        let tag_a = make_tag("human:awanish", "src/auth.ts", 44, 67, 0);
        let tag_b = make_tag("agent:architect-01", "src/auth.ts", 52, 71, -5);
        let overlaps = detect_overlaps(&tag_a, &[tag_b], 30);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].file_path, "src/auth.ts");
        assert_eq!(overlaps[0].status, OverlapStatus::Pending);
    }

    #[test]
    fn test_non_overlapping_regions_different_actors() {
        // Test 2: Two tags same file, non-overlapping region, different actors → 0 events
        let tag_a = make_tag("human:awanish", "src/auth.ts", 10, 20, 0);
        let tag_b = make_tag("agent:coder-01", "src/auth.ts", 50, 60, -5);
        let overlaps = detect_overlaps(&tag_a, &[tag_b], 30);
        assert_eq!(overlaps.len(), 0);
    }

    #[test]
    fn test_overlapping_regions_same_actor() {
        // Test 3: Two tags same file, overlapping region, SAME actor → 0 events
        let tag_a = make_tag("agent:coder-01", "src/auth.ts", 44, 67, 0);
        let tag_b = make_tag("agent:coder-01", "src/auth.ts", 52, 71, -5);
        let overlaps = detect_overlaps(&tag_a, &[tag_b], 30);
        assert_eq!(overlaps.len(), 0);
    }

    #[test]
    fn test_different_files() {
        // Test 4: Two tags different files → 0 events
        let tag_a = make_tag("human:awanish", "src/auth.ts", 44, 67, 0);
        let tag_b = make_tag("agent:architect-01", "src/routes.ts", 44, 67, -5);
        let overlaps = detect_overlaps(&tag_a, &[tag_b], 30);
        assert_eq!(overlaps.len(), 0);
    }

    #[test]
    fn test_outside_time_window() {
        // Outside window: 45 minutes apart, window is 30 minutes
        let tag_a = make_tag("human:awanish", "src/auth.ts", 44, 67, 0);
        let tag_b = make_tag("agent:architect-01", "src/auth.ts", 52, 71, -45);
        let overlaps = detect_overlaps(&tag_a, &[tag_b], 30);
        assert_eq!(overlaps.len(), 0);
    }

    #[test]
    fn test_multiple_overlaps() {
        // Multiple candidates, some overlapping
        let new_tag = make_tag("human:awanish", "src/auth.ts", 44, 67, 0);
        let candidates = vec![
            make_tag("agent:architect-01", "src/auth.ts", 52, 71, -5), // overlaps
            make_tag("agent:coder-01", "src/auth.ts", 10, 20, -5),    // no region overlap
            make_tag("agent:tester-01", "src/auth.ts", 60, 80, -5),   // overlaps
        ];
        let overlaps = detect_overlaps(&new_tag, &candidates, 30);
        assert_eq!(overlaps.len(), 2);
    }
}
