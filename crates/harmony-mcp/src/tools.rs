use std::sync::{Arc, Mutex};
use harmony_core::types::*;
use harmony_core::overlap::detect_overlaps;
use harmony_memory::store::MemoryStore;
use uuid::Uuid;
use chrono::Utc;

/// List all available MCP tools.
pub fn list_tools() -> serde_json::Value {
    serde_json::json!({
        "tools": [
            {
                "name": "harmony_pulse",
                "description": "Return the current Harmony status for this project, including registered agents and pending overlaps.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "query_memory",
                "description": "Semantic search for team memory. Returns relevant decisions, notes, and context.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Semantic search query. E.g. 'why did we reject Redis caching'"
                        },
                        "namespace": {
                            "type": "string",
                            "description": "Memory namespace. Use 'shared' for team memory.",
                            "default": "shared"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Max results to return",
                            "default": 5,
                            "minimum": 1,
                            "maximum": 20
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "add_memory",
                "description": "Store a memory record for the team. Be specific and self-contained.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "Memory content to store. Be specific and self-contained."
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Tags for filtering. E.g. ['decision', 'rejected', 'auth', 'redis']"
                        },
                        "namespace": {
                            "type": "string",
                            "default": "shared"
                        }
                    },
                    "required": ["content", "tags"]
                }
            },
            {
                "name": "report_change",
                "description": "Report a code change for overlap detection. Agents call this after modifying files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "actor_id": { "type": "string" },
                        "file_path": { "type": "string" },
                        "diff_unified": { "type": "string" },
                        "start_line": { "type": "integer" },
                        "end_line": { "type": "integer" },
                        "task_id": { "type": "string", "description": "UUID of the task this change belongs to" },
                        "task_prompt": { "type": "string" }
                    },
                    "required": ["actor_id", "file_path", "diff_unified", "start_line", "end_line"]
                }
            },
            {
                "name": "list_decisions",
                "description": "List stored decisions filtered by file pattern and time range.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file_pattern": {
                            "type": "string",
                            "description": "Glob pattern to filter by file. E.g. 'src/auth/**'"
                        },
                        "since_days": {
                            "type": "integer",
                            "description": "Only decisions from the last N days",
                            "default": 30
                        },
                        "limit": { "type": "integer", "default": 10 }
                    }
                }
            }
        ]
    })
}

/// Call a specific MCP tool.
pub fn call_tool(
    tool_name: &str,
    arguments: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
) -> serde_json::Value {
    match tool_name {
        "harmony_pulse" => handle_harmony_pulse(store),
        "query_memory" => handle_query_memory(arguments, store),
        "add_memory" => handle_add_memory(arguments, store),
        "report_change" => handle_report_change(arguments, store),
        "list_decisions" => handle_list_decisions(arguments, store),
        _ => serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!("Unknown tool: {}", tool_name)
            }],
            "isError": true
        }),
    }
}

fn handle_harmony_pulse(store: &Arc<Mutex<MemoryStore>>) -> serde_json::Value {
    let store = store.lock().unwrap();
    let db_path = store.db_path().display().to_string();
    let project_path = infer_project_path(&db_path);
    let overlaps = match store.get_pending_overlaps() {
        Ok(overlaps) => overlaps,
        Err(error) => {
            return serde_json::json!({
                "content": [{ "type": "text", "text": format!("Error reading pending overlaps: {}", error) }],
                "isError": true
            });
        }
    };
    let agents = match store.get_agents() {
        Ok(agents) => agents,
        Err(error) => {
            return serde_json::json!({
                "content": [{ "type": "text", "text": format!("Error reading registered agents: {}", error) }],
                "isError": true
            });
        }
    };

    let mut lines = vec![
        "Harmony Pulse".to_string(),
        format!("Project: {}", project_path),
        format!("Database: {}", db_path),
        format!("Registered agents: {}", agents.len()),
        format!("Pending overlaps: {}", overlaps.len()),
        String::new(),
    ];

    if overlaps.is_empty() {
        lines.push("No active overlaps found.".to_string());
        lines.push(
            "Next: keep Harmony connected, make overlapping human and agent edits in the same file, then run Harmony Pulse again."
                .to_string(),
        );
    } else {
        lines.push("Active overlaps:".to_string());
        for overlap in overlaps.iter().take(5) {
            lines.push(format!(
                "- {} lines {}-{}: {} vs {}",
                overlap.file_path,
                overlap.region_a.start_line + 1,
                overlap.region_a.end_line + 1,
                overlap.change_a.actor_id.0,
                overlap.change_b.actor_id.0,
            ));
        }

        if overlaps.len() > 5 {
            lines.push(format!("...and {} more overlap(s).", overlaps.len() - 5));
        }
    }

    serde_json::json!({
        "content": [{
            "type": "text",
            "text": lines.join("\n")
        }]
    })
}

fn infer_project_path(db_path: &str) -> String {
    let db = std::path::Path::new(db_path);
    let parent = db.parent().unwrap_or(db);
    let project_root = parent
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case(".harmony"))
        .unwrap_or(false)
        .then(|| parent.parent().unwrap_or(parent))
        .unwrap_or(parent);

    project_root.display().to_string()
}

fn handle_query_memory(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
) -> serde_json::Value {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let namespace_str = args.get("namespace").and_then(|v| v.as_str()).unwrap_or("shared");
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    let namespace = parse_namespace(namespace_str);

    let store = store.lock().unwrap();
    match store.query_memory(query, namespace, limit) {
        Ok(results) => {
            let records: Vec<serde_json::Value> = results.into_iter().map(|(record, similarity)| {
                serde_json::json!({
                    "content": record.content,
                    "tags": record.tags,
                    "similarity": similarity,
                    "created_at": record.created_at.to_rfc3339()
                })
            }).collect();

            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&records).unwrap_or_default()
                }]
            })
        }
        Err(e) => serde_json::json!({
            "content": [{ "type": "text", "text": format!("Error: {}", e) }],
            "isError": true
        }),
    }
}

fn handle_add_memory(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
) -> serde_json::Value {
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return serde_json::json!({
            "content": [{ "type": "text", "text": "Missing required field: content" }],
            "isError": true
        }),
    };

    let tags: Vec<String> = args.get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let namespace_str = args.get("namespace").and_then(|v| v.as_str()).unwrap_or("shared");
    let namespace = parse_namespace(namespace_str);

    let store = store.lock().unwrap();
    match store.add_memory(content, tags, namespace, None, vec![]) {
        Ok(id) => serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::json!({
                    "id": id.to_string(),
                    "message": "Memory stored successfully"
                }).to_string()
            }]
        }),
        Err(e) => serde_json::json!({
            "content": [{ "type": "text", "text": format!("Error: {}", e) }],
            "isError": true
        }),
    }
}

fn handle_report_change(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
) -> serde_json::Value {
    let actor_id_str = args.get("actor_id").and_then(|v| v.as_str()).unwrap_or("");
    let file_path = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
    let diff_unified = args.get("diff_unified").and_then(|v| v.as_str()).unwrap_or("");
    let start_line = args.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let end_line = args.get("end_line").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let task_id = args.get("task_id").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok());
    let task_prompt = args.get("task_prompt").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Build provenance tag
    let tag = ProvenanceTag {
        id: Uuid::new_v4(),
        actor_id: ActorId(actor_id_str.to_string()),
        actor_kind: if actor_id_str.starts_with("human:") { ActorKind::Human } else { ActorKind::Agent },
        task_id,
        task_prompt,
        timestamp: Utc::now(),
        file_path: file_path.to_string(),
        region: TextRange {
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
        },
        mode: AgentMode::Shadow,
        diff_unified: diff_unified.to_string(),
        session_id: Uuid::new_v4(),
    };

    let store = store.lock().unwrap();

    // Step 1: Write provenance tag
    if let Err(e) = store.insert_provenance_tag(&tag) {
        return serde_json::json!({
            "content": [{ "type": "text", "text": format!("Error storing tag: {}", e) }],
            "isError": true
        });
    }

    // Step 2: Detect overlaps
    let recent = store.get_recent_tags_for_file(file_path, 30)
        .unwrap_or_default();
    let overlaps = detect_overlaps(&tag, &recent, 30);

    // Step 3: Store overlap events
    let mut overlap_ids: Vec<String> = Vec::new();
    for overlap in &overlaps {
        if let Err(e) = store.insert_overlap_event(overlap) {
            tracing::error!("Failed to store overlap: {}", e);
        }
        overlap_ids.push(overlap.id.to_string());
    }

    serde_json::json!({
        "content": [{
            "type": "text",
            "text": serde_json::json!({
                "tag_id": tag.id.to_string(),
                "overlaps_detected": overlap_ids
            }).to_string()
        }]
    })
}

fn handle_list_decisions(
    args: &serde_json::Value,
    store: &Arc<Mutex<MemoryStore>>,
) -> serde_json::Value {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    let store = store.lock().unwrap();
    match store.query_memory_by_tag("decision", MemoryNamespace::Shared, limit) {
        Ok(records) => {
            let decisions: Vec<serde_json::Value> = records.into_iter().map(|record| {
                serde_json::json!({
                    "content": record.content,
                    "tags": record.tags,
                    "created_at": record.created_at.to_rfc3339()
                })
            }).collect();

            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&decisions).unwrap_or_default()
                }]
            })
        }
        Err(e) => serde_json::json!({
            "content": [{ "type": "text", "text": format!("Error: {}", e) }],
            "isError": true
        }),
    }
}

fn parse_namespace(s: &str) -> MemoryNamespace {
    if s == "shared" {
        MemoryNamespace::Shared
    } else if let Some(uuid_str) = s.strip_prefix("agent:") {
        if let Ok(uuid) = Uuid::parse_str(uuid_str) {
            MemoryNamespace::Agent(uuid)
        } else {
            MemoryNamespace::Shared
        }
    } else {
        MemoryNamespace::Shared
    }
}

#[cfg(test)]
mod tests {
    use super::{call_tool, list_tools};
    use harmony_memory::store::MemoryStore;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    fn test_store() -> Arc<Mutex<MemoryStore>> {
        Arc::new(Mutex::new(
            MemoryStore::open(Path::new(":memory:")).expect("memory store"),
        ))
    }

    #[test]
    fn list_tools_includes_harmony_pulse() {
        let tools = list_tools();
        let tool_names: Vec<&str> = tools["tools"]
            .as_array()
            .expect("tool list")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();

        assert!(tool_names.contains(&"harmony_pulse"));
    }

    #[test]
    fn harmony_pulse_tool_returns_status_text() {
        let response = call_tool("harmony_pulse", &serde_json::json!({}), &test_store());
        let text = response["content"][0]["text"]
            .as_str()
            .expect("text content");

        assert!(text.contains("Harmony Pulse"));
        assert!(text.contains("Database: :memory:"));
        assert!(text.contains("Registered agents: 0"));
        assert!(text.contains("Pending overlaps: 0"));
    }
}
