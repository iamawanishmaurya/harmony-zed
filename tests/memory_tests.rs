//! Memory store integration tests.
//! Tests golden path CRUD operations against a real SQLite database.

use harmony_memory::store::MemoryStore;
use harmony_core::types::*;
use chrono::Utc;
use uuid::Uuid;
use tempfile::TempDir;

fn setup() -> (MemoryStore, TempDir) {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.db");
    let store = MemoryStore::open(&db).unwrap();
    (store, tmp)
}

fn make_tag(actor: &str, file: &str) -> ProvenanceTag {
    ProvenanceTag {
        id: Uuid::new_v4(),
        actor_id: ActorId(actor.to_string()),
        actor_kind: if actor.starts_with("human:") { ActorKind::Human } else { ActorKind::Agent },
        task_id: None, task_prompt: None,
        timestamp: Utc::now(),
        file_path: file.to_string(),
        region: TextRange { start_line: 0, end_line: 10, start_col: 0, end_col: 0 },
        mode: AgentMode::Shadow,
        diff_unified: String::new(),
        session_id: Uuid::new_v4(),
    }
}

#[test]
fn test_provenance_crud() {
    let (store, _tmp) = setup();
    let tag = make_tag("human:awanish", "src/main.rs");
    store.insert_provenance_tag(&tag).unwrap();
    let recent = store.get_recent_tags_for_file("src/main.rs", 30).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].actor_id.0, "human:awanish");
}

#[test]
fn test_memory_add_query() {
    let (store, _tmp) = setup();
    store.add_memory(
        "We chose PostgreSQL over MySQL for better JSON support",
        vec!["decision".to_string(), "database".to_string()],
        MemoryNamespace::Shared, None, vec![],
    ).unwrap();

    let results = store.query_memory("database choice", MemoryNamespace::Shared, 5).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].0.content.contains("PostgreSQL"));
}

#[test]
fn test_agent_lifecycle() {
    let (store, _tmp) = setup();

    let agent = Agent {
        id: Uuid::new_v4(),
        actor_id: ActorId("agent:architect-01".to_string()),
        role: AgentRole {
            name: "Architect".to_string(),
            avatar_key: "agent-architect".to_string(),
            description: "Plans approach".to_string(),
        },
        status: AgentStatus::Working,
        mode: AgentMode::Shadow,
        task_prompt: Some("Build auth".to_string()),
        task_id: Some(Uuid::new_v4()),
        memory_health: MemoryHealth::Good,
        spawned_at: Utc::now(),
        acp_endpoint: None,
    };

    store.upsert_agent(&agent).unwrap();

    let agents = store.get_agents().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].role.name, "Architect");

    store.delete_agent(agent.id).unwrap();
    let agents = store.get_agents().unwrap();
    assert_eq!(agents.len(), 0);
}
