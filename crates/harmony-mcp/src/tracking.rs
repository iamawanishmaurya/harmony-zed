use chrono::Utc;
use harmony_core::overlap::detect_overlaps;
use harmony_core::types::*;
use harmony_memory::store::MemoryStore;
use uuid::Uuid;

pub(crate) struct RecordChangeArgs<'a> {
    pub actor_id: &'a str,
    pub file_path: &'a str,
    pub diff_unified: &'a str,
    pub start_line: u32,
    pub end_line: u32,
    pub task_id: Option<Uuid>,
    pub task_prompt: Option<String>,
    pub machine_name: &'a str,
    pub machine_ip: &'a str,
}

pub(crate) struct RecordChangeResult {
    pub tag_id: Uuid,
    pub overlaps_detected: Vec<Uuid>,
    pub agent_registered: bool,
}

pub(crate) fn record_change(
    store: &MemoryStore,
    args: RecordChangeArgs<'_>,
) -> anyhow::Result<RecordChangeResult> {
    if args.actor_id.trim().is_empty() {
        anyhow::bail!("Missing required field: actor_id");
    }

    if args.file_path.trim().is_empty() {
        anyhow::bail!("Missing required field: file_path");
    }

    if args.end_line < args.start_line {
        anyhow::bail!("end_line must be greater than or equal to start_line");
    }

    let machine_name = normalized_machine_name(args.machine_name);
    let machine_ip = normalized_machine_ip(args.machine_ip);
    let actor_id = canonical_actor_id(args.actor_id, &machine_name);

    let tag = ProvenanceTag {
        id: Uuid::new_v4(),
        actor_id: ActorId(actor_id.clone()),
        machine_name: machine_name.clone(),
        machine_ip: machine_ip.clone(),
        actor_kind: actor_kind_from_actor_id(&actor_id),
        task_id: args.task_id,
        task_prompt: args.task_prompt.clone(),
        timestamp: Utc::now(),
        file_path: args.file_path.to_string(),
        region: TextRange {
            start_line: args.start_line,
            end_line: args.end_line,
            start_col: 0,
            end_col: 0,
        },
        mode: AgentMode::Shadow,
        diff_unified: args.diff_unified.to_string(),
        session_id: Uuid::new_v4(),
    };

    let agent_registered =
        ensure_agent_registered(store, &actor_id, &machine_name, &machine_ip, args.task_prompt)?;

    store.insert_provenance_tag(&tag)?;

    let recent = store.get_recent_tags_for_file(args.file_path, 30).unwrap_or_default();
    let overlaps = detect_overlaps(&tag, &recent, 30);

    let mut overlap_ids = Vec::with_capacity(overlaps.len());
    for overlap in &overlaps {
        if let Err(error) = store.insert_overlap_event(overlap) {
            tracing::error!("Failed to store overlap: {}", error);
        }
        overlap_ids.push(overlap.id);
    }

    Ok(RecordChangeResult {
        tag_id: tag.id,
        overlaps_detected: overlap_ids,
        agent_registered,
    })
}

pub(crate) fn normalize_file_path(file_path: &str) -> String {
    file_path.replace('\\', "/")
}

pub(crate) fn canonical_actor_id(actor_id_str: &str, machine_name: &str) -> String {
    let trimmed_actor = actor_id_str.trim();
    let trimmed_machine = machine_name.trim();
    if trimmed_actor.is_empty()
        || trimmed_actor.contains('@')
        || trimmed_machine.is_empty()
        || trimmed_machine.eq_ignore_ascii_case("local")
    {
        trimmed_actor.to_string()
    } else {
        format!("{trimmed_actor}@{trimmed_machine}")
    }
}

pub(crate) fn default_machine_name() -> String {
    std::env::var("HARMONY_MACHINE_NAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "local".to_string())
}

pub(crate) fn default_machine_ip() -> String {
    std::env::var("HARMONY_MACHINE_IP")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

pub(crate) fn synthetic_diff_for_content(start_line: u32, content: &str) -> String {
    let normalized_content = if content.is_empty() {
        "// Harmony tracked edit".to_string()
    } else {
        content.replace("\r\n", "\n")
    };
    let body = normalized_content
        .lines()
        .map(|line| format!("+{}", line))
        .collect::<Vec<_>>()
        .join("\n");
    let line_count = normalized_content.lines().count().max(1);
    format!(
        "@@ -{},0 +{},{} @@\n{}",
        start_line + 1,
        start_line + 1,
        line_count,
        body
    )
}

fn actor_kind_from_actor_id(actor_id_str: &str) -> ActorKind {
    if actor_id_str.starts_with("human:") {
        ActorKind::Human
    } else {
        ActorKind::Agent
    }
}

fn ensure_agent_registered(
    store: &MemoryStore,
    actor_id_str: &str,
    machine_name: &str,
    machine_ip: &str,
    task_prompt: Option<String>,
) -> anyhow::Result<bool> {
    if !actor_id_str.starts_with("agent:") {
        return Ok(false);
    }

    let existing = store
        .get_agents()?
        .into_iter()
        .find(|agent| agent.actor_id.0 == actor_id_str);

    let agent = if let Some(mut agent) = existing {
        agent.status = AgentStatus::Working;
        agent.task_prompt = task_prompt.or(agent.task_prompt);
        agent.memory_health = MemoryHealth::Good;
        agent.machine_name = machine_name.to_string();
        agent.machine_ip = machine_ip.to_string();
        agent
    } else {
        Agent {
            id: Uuid::new_v4(),
            actor_id: ActorId(actor_id_str.to_string()),
            machine_name: machine_name.to_string(),
            machine_ip: machine_ip.to_string(),
            role: inferred_agent_role(actor_id_str),
            status: AgentStatus::Working,
            mode: AgentMode::Shadow,
            task_prompt,
            task_id: None,
            memory_health: MemoryHealth::Good,
            spawned_at: Utc::now(),
            acp_endpoint: None,
        }
    };

    store.upsert_agent(&agent)?;
    Ok(true)
}

fn inferred_agent_role(actor_id_str: &str) -> AgentRole {
    let raw_name = actor_id_str
        .strip_prefix("agent:")
        .unwrap_or(actor_id_str)
        .split('@')
        .next()
        .unwrap_or(actor_id_str);
    let name = raw_name
        .split(['-', '_', '/', ':'])
        .filter(|segment| !segment.is_empty())
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join(" ");
    let role_name = if name.is_empty() {
        "Agent".to_string()
    } else {
        name
    };

    AgentRole {
        name: role_name.clone(),
        avatar_key: "agent-generic".to_string(),
        description: format!("Auto-registered Harmony agent for {}", role_name),
    }
}

fn capitalize_word(segment: &str) -> String {
    let mut chars = segment.chars();
    match chars.next() {
        Some(first) => {
            let mut capitalized = first.to_uppercase().collect::<String>();
            capitalized.push_str(chars.as_str());
            capitalized
        }
        None => String::new(),
    }
}

fn normalized_machine_name(machine_name: &str) -> String {
    let trimmed = machine_name.trim();
    if trimmed.is_empty() {
        default_machine_name()
    } else {
        trimmed.to_string()
    }
}

fn normalized_machine_ip(machine_ip: &str) -> String {
    let trimmed = machine_ip.trim();
    if trimmed.is_empty() {
        default_machine_ip()
    } else {
        trimmed.to_string()
    }
}
