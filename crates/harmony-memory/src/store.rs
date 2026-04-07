use std::path::{Path, PathBuf};
use rusqlite::{Connection, OptionalExtension, params};
use harmony_core::types::*;
use chrono::{Utc, Duration, DateTime};
use uuid::Uuid;
use tracing;

use crate::schema;

pub struct MemoryStore {
    conn: Connection,
    project_db_path: PathBuf,
}

impl MemoryStore {
    /// Open or create the SQLite DB at path. Runs all pending migrations.
    pub fn open(db_path: &Path) -> anyhow::Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;

        // Apply PRAGMAs
        conn.execute_batch(schema::PRAGMAS)?;

        // Create schema_version table
        conn.execute_batch(schema::SCHEMA_VERSION_TABLE)?;

        // Run pending migrations
        for (migration_id, migration_sql) in schema::MIGRATIONS {
            let already_applied: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM schema_version WHERE migration_id = ?1",
                params![migration_id],
                |row| row.get(0),
            )?;

            if !already_applied {
                tracing::info!("Applying migration: {}", migration_id);
                conn.execute_batch(migration_sql)?;
                conn.execute(
                    "INSERT INTO schema_version (migration_id, applied_at) VALUES (?1, ?2)",
                    params![migration_id, Utc::now().to_rfc3339()],
                )?;
            }
        }

        Ok(Self {
            conn,
            project_db_path: db_path.to_path_buf(),
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.project_db_path
    }

    // ── Provenance ────────────────────────────────────────────────────────────

    pub fn insert_provenance_tag(&self, tag: &ProvenanceTag) -> anyhow::Result<()> {
        let actor_kind = serde_json::to_string(&tag.actor_kind)?;
        let mode = serde_json::to_string(&tag.mode)?;

        self.conn.execute(
            "INSERT INTO provenance_tags (
                id, actor_id, actor_kind, task_id, task_prompt,
                timestamp, file_path, region_start_line, region_end_line,
                region_start_col, region_end_col, mode, diff_unified, session_id,
                machine_name, machine_ip
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                tag.id.to_string(),
                tag.actor_id.0,
                actor_kind,
                tag.task_id.map(|u| u.to_string()),
                tag.task_prompt,
                tag.timestamp.to_rfc3339(),
                tag.file_path,
                tag.region.start_line,
                tag.region.end_line,
                tag.region.start_col,
                tag.region.end_col,
                mode,
                tag.diff_unified,
                tag.session_id.to_string(),
                tag.machine_name,
                tag.machine_ip,
            ],
        )?;
        Ok(())
    }

    /// Fetch all provenance tags for a file, newer than `since_minutes`.
    pub fn get_recent_tags_for_file(
        &self,
        file_path: &str,
        since_minutes: u32,
    ) -> anyhow::Result<Vec<ProvenanceTag>> {
        let since = Utc::now() - Duration::minutes(since_minutes as i64);
        let since_str = since.to_rfc3339();

        let mut stmt = self.conn.prepare(
            "SELECT id, actor_id, actor_kind, task_id, task_prompt,
                    timestamp, file_path, region_start_line, region_end_line,
                    region_start_col, region_end_col, mode, diff_unified, session_id,
                    machine_name, machine_ip
             FROM provenance_tags
             WHERE file_path = ?1 AND timestamp > ?2
             ORDER BY timestamp DESC"
        )?;

        let tags = stmt.query_map(params![file_path, since_str], |row| {
            let id_str: String = row.get(0)?;
            let actor_id_str: String = row.get(1)?;
            let actor_kind_str: String = row.get(2)?;
            let task_id_str: Option<String> = row.get(3)?;
            let task_prompt: Option<String> = row.get(4)?;
            let timestamp_str: String = row.get(5)?;
            let file_path: String = row.get(6)?;
            let start_line: u32 = row.get(7)?;
            let end_line: u32 = row.get(8)?;
            let start_col: u32 = row.get(9)?;
            let end_col: u32 = row.get(10)?;
            let mode_str: String = row.get(11)?;
            let diff_unified: String = row.get(12)?;
            let session_id_str: String = row.get(13)?;
            let machine_name: String = row.get(14)?;
            let machine_ip: String = row.get(15)?;

            Ok((id_str, actor_id_str, actor_kind_str, task_id_str, task_prompt,
                timestamp_str, file_path, start_line, end_line, start_col, end_col,
                mode_str, diff_unified, session_id_str, machine_name, machine_ip))
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut result = Vec::new();
        for (id_str, actor_id_str, actor_kind_str, task_id_str, task_prompt,
             timestamp_str, file_path, start_line, end_line, start_col, end_col,
             mode_str, diff_unified, session_id_str, machine_name, machine_ip) in tags
        {
            let actor_kind: ActorKind = serde_json::from_str(&actor_kind_str)
                .unwrap_or(ActorKind::Human);
            let mode: AgentMode = serde_json::from_str(&mode_str)
                .unwrap_or(AgentMode::Shadow);
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            result.push(ProvenanceTag {
                id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
                actor_id: ActorId(actor_id_str),
                machine_name,
                machine_ip,
                actor_kind,
                task_id: task_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
                task_prompt,
                timestamp,
                file_path,
                region: TextRange { start_line, end_line, start_col, end_col },
                mode,
                diff_unified,
                session_id: Uuid::parse_str(&session_id_str).unwrap_or_else(|_| Uuid::new_v4()),
            });
        }

        Ok(result)
    }

    // ── Agents ────────────────────────────────────────────────────────────────

    pub fn upsert_agent(&self, agent: &Agent) -> anyhow::Result<()> {
        let status = serde_json::to_string(&agent.status)?;
        let mode = serde_json::to_string(&agent.mode)?;
        let memory_health = serde_json::to_string(&agent.memory_health)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO agents (
                id, actor_id, role_name, role_avatar, role_desc,
                status, mode, task_prompt, task_id, memory_health,
                spawned_at, acp_endpoint, session_id, machine_name, machine_ip
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                agent.id.to_string(),
                agent.actor_id.0,
                agent.role.name,
                agent.role.avatar_key,
                agent.role.description,
                status,
                mode,
                agent.task_prompt,
                agent.task_id.map(|u| u.to_string()),
                memory_health,
                agent.spawned_at.to_rfc3339(),
                agent.acp_endpoint,
                Uuid::new_v4().to_string(), // session_id
                agent.machine_name,
                agent.machine_ip,
            ],
        )?;
        Ok(())
    }

    pub fn get_agents(&self) -> anyhow::Result<Vec<Agent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, actor_id, role_name, role_avatar, role_desc,
                    status, mode, task_prompt, task_id, memory_health,
                    spawned_at, acp_endpoint, machine_name, machine_ip
             FROM agents"
        )?;

        let agents = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let actor_id_str: String = row.get(1)?;
            let role_name: String = row.get(2)?;
            let role_avatar: String = row.get(3)?;
            let role_desc: String = row.get(4)?;
            let status_str: String = row.get(5)?;
            let mode_str: String = row.get(6)?;
            let task_prompt: Option<String> = row.get(7)?;
            let task_id_str: Option<String> = row.get(8)?;
            let memory_health_str: String = row.get(9)?;
            let spawned_at_str: String = row.get(10)?;
            let acp_endpoint: Option<String> = row.get(11)?;
            let machine_name: String = row.get(12)?;
            let machine_ip: String = row.get(13)?;

            Ok((id_str, actor_id_str, role_name, role_avatar, role_desc,
                status_str, mode_str, task_prompt, task_id_str,
                memory_health_str, spawned_at_str, acp_endpoint, machine_name, machine_ip))
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut result = Vec::new();
        for (id_str, actor_id_str, role_name, role_avatar, role_desc,
             status_str, mode_str, task_prompt, task_id_str,
             memory_health_str, spawned_at_str, acp_endpoint, machine_name, machine_ip) in agents
        {
            result.push(Agent {
                id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
                actor_id: ActorId(actor_id_str),
                machine_name,
                machine_ip,
                role: AgentRole { name: role_name, avatar_key: role_avatar, description: role_desc },
                status: serde_json::from_str(&status_str).unwrap_or(AgentStatus::Idle),
                mode: serde_json::from_str(&mode_str).unwrap_or(AgentMode::Shadow),
                task_prompt,
                task_id: task_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
                memory_health: serde_json::from_str(&memory_health_str).unwrap_or(MemoryHealth::Good),
                spawned_at: DateTime::parse_from_rfc3339(&spawned_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                acp_endpoint,
            });
        }
        Ok(result)
    }

    pub fn get_agent(&self, id: Uuid) -> anyhow::Result<Option<Agent>> {
        let agents = self.get_agents()?;
        Ok(agents.into_iter().find(|a| a.id == id))
    }

    pub fn delete_agent(&self, id: Uuid) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM agents WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    // ── Shadow Diffs ──────────────────────────────────────────────────────────

    pub fn insert_shadow_diff(&self, diff: &ShadowDiff) -> anyhow::Result<()> {
        let status = serde_json::to_string(&diff.status)?;
        self.conn.execute(
            "INSERT INTO shadow_diffs (id, agent_id, file_path, diff_unified, base_hash, created_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                diff.id.to_string(),
                diff.agent_id.to_string(),
                diff.file_path,
                diff.diff_unified,
                diff.base_hash,
                diff.created_at.to_rfc3339(),
                status,
            ],
        )?;
        Ok(())
    }

    pub fn update_shadow_diff_status(&self, id: Uuid, status: ShadowDiffStatus) -> anyhow::Result<()> {
        let status_str = serde_json::to_string(&status)?;
        self.conn.execute(
            "UPDATE shadow_diffs SET status = ?1 WHERE id = ?2",
            params![status_str, id.to_string()],
        )?;
        Ok(())
    }

    pub fn get_shadow_diffs_for_agent(&self, agent_id: Uuid) -> anyhow::Result<Vec<ShadowDiff>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, agent_id, file_path, diff_unified, base_hash, created_at, status
             FROM shadow_diffs WHERE agent_id = ?1"
        )?;
        self.query_shadow_diffs(&mut stmt, params![agent_id.to_string()])
    }

    pub fn get_pending_shadow_diffs(&self) -> anyhow::Result<Vec<ShadowDiff>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, agent_id, file_path, diff_unified, base_hash, created_at, status
             FROM shadow_diffs WHERE status = '\"pending\"'"
        )?;
        self.query_shadow_diffs(&mut stmt, [])
    }

    fn query_shadow_diffs<P: rusqlite::Params>(&self, stmt: &mut rusqlite::Statement, params: P) -> anyhow::Result<Vec<ShadowDiff>> {
        let rows = stmt.query_map(params, |row| {
            let id_str: String = row.get(0)?;
            let agent_id_str: String = row.get(1)?;
            let file_path: String = row.get(2)?;
            let diff_unified: String = row.get(3)?;
            let base_hash: String = row.get(4)?;
            let created_at_str: String = row.get(5)?;
            let status_str: String = row.get(6)?;

            Ok((id_str, agent_id_str, file_path, diff_unified, base_hash, created_at_str, status_str))
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut result = Vec::new();
        for (id_str, agent_id_str, file_path, diff_unified, base_hash, created_at_str, status_str) in rows {
            result.push(ShadowDiff {
                id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
                agent_id: Uuid::parse_str(&agent_id_str).unwrap_or_else(|_| Uuid::new_v4()),
                file_path,
                diff_unified,
                base_hash,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                status: serde_json::from_str(&status_str).unwrap_or(ShadowDiffStatus::Pending),
            });
        }
        Ok(result)
    }

    // ── Overlap Events ────────────────────────────────────────────────────────

    pub fn insert_overlap_event(&self, event: &OverlapEvent) -> anyhow::Result<()> {
        let status = serde_json::to_string(&event.status)?;
        self.conn.execute(
            "INSERT INTO overlap_events (
                id, file_path, region_a_start, region_a_end, region_b_start, region_b_end,
                change_a_id, change_b_id, detected_at, status, session_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                event.id.to_string(),
                event.file_path,
                event.region_a.start_line,
                event.region_a.end_line,
                event.region_b.start_line,
                event.region_b.end_line,
                event.change_a.id.to_string(),
                event.change_b.id.to_string(),
                event.detected_at.to_rfc3339(),
                status,
                event.change_a.session_id.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn update_overlap_status(&self, id: Uuid, status: OverlapStatus) -> anyhow::Result<()> {
        let status_str = serde_json::to_string(&status)?;
        let resolved_at = match &status {
            OverlapStatus::Resolved(_) => Some(Utc::now().to_rfc3339()),
            _ => None,
        };
        let resolution_kind = match &status {
            OverlapStatus::Resolved(kind) => Some(serde_json::to_string(kind)?),
            _ => None,
        };
        self.conn.execute(
            "UPDATE overlap_events SET status = ?1, resolved_at = ?2, resolution_kind = ?3 WHERE id = ?4",
            params![status_str, resolved_at, resolution_kind, id.to_string()],
        )?;
        Ok(())
    }

    pub fn get_pending_overlaps(&self) -> anyhow::Result<Vec<OverlapEvent>> {
        self.get_overlaps_filtered(Some(OverlapStatus::Pending))
    }

    pub fn get_all_overlaps(&self) -> anyhow::Result<Vec<OverlapEvent>> {
        self.get_overlaps_filtered(None)
    }

    pub fn get_overlap(&self, id: Uuid) -> anyhow::Result<Option<OverlapEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT oe.id, oe.file_path, oe.region_a_start, oe.region_a_end,
                    oe.region_b_start, oe.region_b_end, oe.detected_at, oe.status,
                    oe.change_a_id, oe.change_b_id
             FROM overlap_events oe
             WHERE oe.id = ?1"
        )?;

        let row = stmt
            .query_row(params![id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, u32>(4)?,
                    row.get::<_, u32>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })
            .optional()?;

        row.map(|row| self.hydrate_overlap_event(row)).transpose()
    }

    fn get_overlaps_filtered(
        &self,
        status_filter: Option<OverlapStatus>,
    ) -> anyhow::Result<Vec<OverlapEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT oe.id, oe.file_path, oe.region_a_start, oe.region_a_end,
                    oe.region_b_start, oe.region_b_end, oe.detected_at, oe.status,
                    oe.change_a_id, oe.change_b_id
             FROM overlap_events oe
             ORDER BY oe.detected_at DESC"
        )?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, u32>(4)?,
                    row.get::<_, u32>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut overlaps = Vec::new();
        for row in rows {
            let event = self.hydrate_overlap_event(row)?;
            if let Some(filter) = &status_filter {
                if &event.status != filter {
                    continue;
                }
            }
            overlaps.push(event);
        }

        Ok(overlaps)
    }

    fn hydrate_overlap_event(
        &self,
        row: (String, String, u32, u32, u32, u32, String, String, String, String),
    ) -> anyhow::Result<OverlapEvent> {
        let (
            id_str,
            file_path,
            ra_start,
            ra_end,
            rb_start,
            rb_end,
            detected_at_str,
            status_str,
            change_a_id_str,
            change_b_id_str,
        ) = row;

        let change_a = self
            .get_provenance_tag(&change_a_id_str)?
            .ok_or_else(|| anyhow::anyhow!("Missing provenance tag {}", change_a_id_str))?;
        let change_b = self
            .get_provenance_tag(&change_b_id_str)?
            .ok_or_else(|| anyhow::anyhow!("Missing provenance tag {}", change_b_id_str))?;

        Ok(OverlapEvent {
            id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
            file_path,
            region_a: TextRange {
                start_line: ra_start,
                end_line: ra_end,
                start_col: 0,
                end_col: 0,
            },
            region_b: TextRange {
                start_line: rb_start,
                end_line: rb_end,
                start_col: 0,
                end_col: 0,
            },
            change_a,
            change_b,
            detected_at: DateTime::parse_from_rfc3339(&detected_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            status: serde_json::from_str(&status_str).unwrap_or(OverlapStatus::Pending),
        })
    }

    fn get_provenance_tag(&self, id_str: &str) -> anyhow::Result<Option<ProvenanceTag>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, actor_id, actor_kind, task_id, task_prompt,
                    timestamp, file_path, region_start_line, region_end_line,
                    region_start_col, region_end_col, mode, diff_unified, session_id,
                    machine_name, machine_ip
             FROM provenance_tags WHERE id = ?1"
        )?;

        let result = stmt.query_row(params![id_str], |row| {
            let id_str: String = row.get(0)?;
            let actor_id_str: String = row.get(1)?;
            let actor_kind_str: String = row.get(2)?;
            let task_id_str: Option<String> = row.get(3)?;
            let task_prompt: Option<String> = row.get(4)?;
            let timestamp_str: String = row.get(5)?;
            let file_path: String = row.get(6)?;
            let start_line: u32 = row.get(7)?;
            let end_line: u32 = row.get(8)?;
            let start_col: u32 = row.get(9)?;
            let end_col: u32 = row.get(10)?;
            let mode_str: String = row.get(11)?;
            let diff_unified: String = row.get(12)?;
            let session_id_str: String = row.get(13)?;
            let machine_name: String = row.get(14)?;
            let machine_ip: String = row.get(15)?;

            Ok((id_str, actor_id_str, actor_kind_str, task_id_str, task_prompt,
                timestamp_str, file_path, start_line, end_line, start_col, end_col,
                mode_str, diff_unified, session_id_str, machine_name, machine_ip))
        });

        match result {
            Ok((id_str, actor_id_str, actor_kind_str, task_id_str, task_prompt,
                timestamp_str, file_path, start_line, end_line, start_col, end_col,
                mode_str, diff_unified, session_id_str, machine_name, machine_ip)) => {
                Ok(Some(ProvenanceTag {
                    id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
                    actor_id: ActorId(actor_id_str),
                    machine_name,
                    machine_ip,
                    actor_kind: serde_json::from_str(&actor_kind_str).unwrap_or(ActorKind::Human),
                    task_id: task_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
                    task_prompt,
                    timestamp: DateTime::parse_from_rfc3339(&timestamp_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    file_path,
                    region: TextRange { start_line, end_line, start_col, end_col },
                    mode: serde_json::from_str(&mode_str).unwrap_or(AgentMode::Shadow),
                    diff_unified,
                    session_id: Uuid::parse_str(&session_id_str).unwrap_or_else(|_| Uuid::new_v4()),
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Memory Records ────────────────────────────────────────────────────────

    pub fn insert_file_sync_event(&self, event: &FileSyncEvent) -> anyhow::Result<FileSyncEvent> {
        let entry_kind = serde_json::to_string(&event.entry_kind)?;
        let change_kind = serde_json::to_string(&event.change_kind)?;

        self.conn.execute(
            "INSERT INTO file_sync_events (
                id, relative_path, entry_kind, change_kind, content_base64,
                content_sha256, size_bytes, actor_id, machine_name, machine_ip,
                detected_at, impact_summary
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                event.id.to_string(),
                event.relative_path,
                entry_kind,
                change_kind,
                event.content_base64,
                event.content_sha256,
                event.size_bytes as i64,
                event.actor_id.0,
                event.machine_name,
                event.machine_ip,
                event.detected_at.to_rfc3339(),
                event.impact_summary,
            ],
        )?;

        let mut inserted = event.clone();
        inserted.seq = self.conn.last_insert_rowid();
        Ok(inserted)
    }

    pub fn get_recent_file_sync_events(&self, limit: u32) -> anyhow::Result<Vec<FileSyncEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, id, relative_path, entry_kind, change_kind, content_base64,
                    content_sha256, size_bytes, actor_id, machine_name, machine_ip,
                    detected_at, impact_summary
             FROM file_sync_events
             ORDER BY seq DESC
             LIMIT ?1",
        )?;

        let mut events = self.query_file_sync_events(&mut stmt, params![limit as i64])?;
        events.reverse();
        Ok(events)
    }

    pub fn get_file_sync_events_since(
        &self,
        since_seq: i64,
        limit: u32,
    ) -> anyhow::Result<Vec<FileSyncEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, id, relative_path, entry_kind, change_kind, content_base64,
                    content_sha256, size_bytes, actor_id, machine_name, machine_ip,
                    detected_at, impact_summary
             FROM file_sync_events
             WHERE seq > ?1
             ORDER BY seq ASC
             LIMIT ?2",
        )?;

        self.query_file_sync_events(&mut stmt, params![since_seq, limit as i64])
    }

    fn query_file_sync_events<P: rusqlite::Params>(
        &self,
        stmt: &mut rusqlite::Statement,
        params: P,
    ) -> anyhow::Result<Vec<FileSyncEvent>> {
        let rows = stmt
            .query_map(params, |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut events = Vec::with_capacity(rows.len());
        for (
            seq,
            id_str,
            relative_path,
            entry_kind_str,
            change_kind_str,
            content_base64,
            content_sha256,
            size_bytes,
            actor_id_str,
            machine_name,
            machine_ip,
            detected_at_str,
            impact_summary,
        ) in rows
        {
            events.push(FileSyncEvent {
                seq,
                id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
                relative_path,
                entry_kind: serde_json::from_str(&entry_kind_str)
                    .unwrap_or(FileSyncEntryKind::File),
                change_kind: serde_json::from_str(&change_kind_str)
                    .unwrap_or(FileSyncChangeKind::Updated),
                content_base64,
                content_sha256,
                size_bytes: size_bytes.max(0) as u64,
                actor_id: ActorId(actor_id_str),
                machine_name,
                machine_ip,
                detected_at: DateTime::parse_from_rfc3339(&detected_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                impact_summary,
            });
        }

        Ok(events)
    }

    /// Add a new memory record. Embedding should be pre-computed (or empty vec for stub).
    pub fn add_memory(
        &self,
        content: &str,
        tags: Vec<String>,
        namespace: MemoryNamespace,
        provenance_id: Option<Uuid>,
        embedding: Vec<f32>,
    ) -> anyhow::Result<Uuid> {
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let namespace_str = match &namespace {
            MemoryNamespace::Shared => "shared".to_string(),
            MemoryNamespace::Agent(uuid) => format!("agent:{}", uuid),
        };
        let tags_json = serde_json::to_string(&tags)?;
        let embedding_bytes = vec_to_bytes(&embedding);

        self.conn.execute(
            "INSERT INTO memory_records (id, content, embedding, namespace, tags, provenance_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id.to_string(),
                content,
                embedding_bytes,
                namespace_str,
                tags_json,
                provenance_id.map(|u| u.to_string()),
                now,
                now,
            ],
        )?;
        Ok(id)
    }

    /// Query memory records with cosine-similarity ranking.
    ///
    /// Embeds the query string using the keyword-fallback engine, then
    /// fetches all matching records, scores each by cosine similarity,
    /// sorts descending, and returns the top `limit` results.
    pub fn query_memory(
        &self,
        query: &str,
        namespace: MemoryNamespace,
        limit: usize,
    ) -> anyhow::Result<Vec<(MemoryRecord, f32)>> {
        use crate::embeddings::EmbeddingEngine;

        let namespace_str = match &namespace {
            MemoryNamespace::Shared => "shared".to_string(),
            MemoryNamespace::Agent(uuid) => format!("agent:{}", uuid),
        };

        // Embed the query (keyword fallback is instant)
        let engine = EmbeddingEngine::new()?;
        let query_vec = engine.embed_one(query)?;

        // Fetch up to 500 candidates (more than limit so we can rank)
        let fetch_limit = std::cmp::max(limit * 20, 500);

        let mut stmt = self.conn.prepare(
            "SELECT id, content, embedding, namespace, tags, provenance_id, created_at, updated_at
             FROM memory_records WHERE namespace = ?1
             ORDER BY created_at DESC LIMIT ?2"
        )?;

        let rows = stmt.query_map(params![namespace_str, fetch_limit as i64], |row| {
            let id_str: String = row.get(0)?;
            let content: String = row.get(1)?;
            let embedding_bytes: Vec<u8> = row.get(2)?;
            let _namespace_str: String = row.get(3)?;
            let tags_json: String = row.get(4)?;
            let provenance_str: Option<String> = row.get(5)?;
            let created_at_str: String = row.get(6)?;
            let updated_at_str: String = row.get(7)?;
            Ok((id_str, content, embedding_bytes, tags_json, provenance_str,
                created_at_str, updated_at_str))
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut scored: Vec<(MemoryRecord, f32)> = Vec::new();
        for (id_str, content, embedding_bytes, tags_json, provenance_str,
             created_at_str, updated_at_str) in rows
        {
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            let embedding = bytes_to_vec(&embedding_bytes);

            let similarity = if embedding.is_empty() {
                // Legacy records without embeddings — re-embed content on the fly
                let content_vec = engine.embed_one(&content).unwrap_or_else(|_| vec![0.0; 384]);
                EmbeddingEngine::cosine_similarity(&query_vec, &content_vec)
            } else {
                EmbeddingEngine::cosine_similarity(&query_vec, &embedding)
            };

            let record = MemoryRecord {
                id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
                content,
                embedding,
                namespace: namespace.clone(),
                tags,
                provenance: provenance_str.and_then(|s| Uuid::parse_str(&s).ok()),
                created_at: DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            };
            scored.push((record, similarity));
        }

        // Sort by similarity descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        Ok(scored)
    }

    /// Get memory records filtered by tag (used by list_decisions)
    pub fn query_memory_by_tag(
        &self,
        tag_filter: &str,
        namespace: MemoryNamespace,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        let namespace_str = match &namespace {
            MemoryNamespace::Shared => "shared".to_string(),
            MemoryNamespace::Agent(uuid) => format!("agent:{}", uuid),
        };

        // Use LIKE to match tag within JSON array
        let tag_pattern = format!("%\"{}\"%" , tag_filter);

        let mut stmt = self.conn.prepare(
            "SELECT id, content, embedding, namespace, tags, provenance_id, created_at, updated_at
             FROM memory_records WHERE namespace = ?1 AND tags LIKE ?2
             ORDER BY created_at DESC LIMIT ?3"
        )?;

        let rows = stmt.query_map(params![namespace_str, tag_pattern, limit as i64], |row| {
            let id_str: String = row.get(0)?;
            let content: String = row.get(1)?;
            let embedding_bytes: Vec<u8> = row.get(2)?;
            let _namespace_str: String = row.get(3)?;
            let tags_json: String = row.get(4)?;
            let provenance_str: Option<String> = row.get(5)?;
            let created_at_str: String = row.get(6)?;
            let updated_at_str: String = row.get(7)?;
            Ok((id_str, content, embedding_bytes, tags_json, provenance_str,
                created_at_str, updated_at_str))
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut result = Vec::new();
        for (id_str, content, embedding_bytes, tags_json, provenance_str,
             created_at_str, updated_at_str) in rows
        {
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            let embedding = bytes_to_vec(&embedding_bytes);

            result.push(MemoryRecord {
                id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
                content,
                embedding,
                namespace: namespace.clone(),
                tags,
                provenance: provenance_str.and_then(|s| Uuid::parse_str(&s).ok()),
                created_at: DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            });
        }
        Ok(result)
    }
}

// ── Helper Functions ──────────────────────────────────────────────────────────

/// Serialize f32 vec to bytes for SQLite BLOB storage (little-endian IEEE 754).
fn vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &val in v {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Deserialize bytes from SQLite BLOB to f32 vec.
fn bytes_to_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap();
            f32::from_le_bytes(arr)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_store() -> (MemoryStore, TempDir) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let store = MemoryStore::open(&db_path).unwrap();
        (store, tmp)
    }

    fn make_test_tag(actor_id: &str, file_path: &str, start: u32, end: u32) -> ProvenanceTag {
        ProvenanceTag {
            id: Uuid::new_v4(),
            actor_id: ActorId(actor_id.to_string()),
            machine_name: "local".to_string(),
            machine_ip: "127.0.0.1".to_string(),
            actor_kind: if actor_id.starts_with("human:") { ActorKind::Human } else { ActorKind::Agent },
            task_id: None,
            task_prompt: None,
            timestamp: Utc::now(),
            file_path: file_path.to_string(),
            region: TextRange { start_line: start, end_line: end, start_col: 0, end_col: 0 },
            mode: AgentMode::Shadow,
            diff_unified: String::new(),
            session_id: Uuid::new_v4(),
        }
    }

    #[test]
    fn test_open_creates_db() {
        let (_, tmp) = create_test_store();
        assert!(tmp.path().join("test.db").exists());
    }

    #[test]
    fn test_insert_and_get_provenance_tags() {
        let (store, _tmp) = create_test_store();

        let tag1 = make_test_tag("human:awanish", "src/auth.ts", 10, 20);
        let tag2 = make_test_tag("agent:coder-01", "src/auth.ts", 30, 40);
        let tag3 = make_test_tag("agent:architect-01", "src/auth.ts", 50, 60);

        store.insert_provenance_tag(&tag1).unwrap();
        store.insert_provenance_tag(&tag2).unwrap();
        store.insert_provenance_tag(&tag3).unwrap();

        let recent = store.get_recent_tags_for_file("src/auth.ts", 30).unwrap();
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_provenance_tags_different_file() {
        let (store, _tmp) = create_test_store();

        let tag1 = make_test_tag("human:awanish", "src/auth.ts", 10, 20);
        let tag2 = make_test_tag("agent:coder-01", "src/routes.ts", 30, 40);

        store.insert_provenance_tag(&tag1).unwrap();
        store.insert_provenance_tag(&tag2).unwrap();

        let auth_tags = store.get_recent_tags_for_file("src/auth.ts", 30).unwrap();
        assert_eq!(auth_tags.len(), 1);

        let routes_tags = store.get_recent_tags_for_file("src/routes.ts", 30).unwrap();
        assert_eq!(routes_tags.len(), 1);
    }

    #[test]
    fn test_memory_store_add_and_query() {
        let (store, _tmp) = create_test_store();

        let id = store.add_memory(
            "We rejected Redis for caching due to cost constraints",
            vec!["decision".to_string(), "rejected".to_string(), "redis".to_string()],
            MemoryNamespace::Shared,
            None,
            vec![], // empty embedding for now
        ).unwrap();

        assert_ne!(id, Uuid::nil());

        let results = store.query_memory("redis caching", MemoryNamespace::Shared, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].0.content.contains("Redis"));
    }

    #[test]
    fn test_memory_by_tag() {
        let (store, _tmp) = create_test_store();

        store.add_memory(
            "Decided to use JWT over sessions",
            vec!["decision".to_string(), "auth".to_string()],
            MemoryNamespace::Shared, None, vec![],
        ).unwrap();

        store.add_memory(
            "Added logging middleware",
            vec!["implementation".to_string()],
            MemoryNamespace::Shared, None, vec![],
        ).unwrap();

        let decisions = store.query_memory_by_tag("decision", MemoryNamespace::Shared, 10).unwrap();
        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].content.contains("JWT"));
    }

    #[test]
    fn test_vec_bytes_round_trip() {
        let original = vec![1.0f32, 2.5, -3.14, 0.0, 42.0];
        let bytes = vec_to_bytes(&original);
        let recovered = bytes_to_vec(&bytes);
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_agent_upsert_and_get() {
        let (store, _tmp) = create_test_store();

        let agent = Agent {
            id: Uuid::new_v4(),
            actor_id: ActorId("agent:coder-01".to_string()),
            machine_name: "local".to_string(),
            machine_ip: "127.0.0.1".to_string(),
            role: AgentRole {
                name: "Coder".to_string(),
                avatar_key: "agent-coder".to_string(),
                description: "Writes code".to_string(),
            },
            status: AgentStatus::Idle,
            mode: AgentMode::Shadow,
            task_prompt: None,
            task_id: None,
            memory_health: MemoryHealth::Good,
            spawned_at: Utc::now(),
            acp_endpoint: Some("http://localhost:4231".to_string()),
        };

        store.upsert_agent(&agent).unwrap();
        let agents = store.get_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].role.name, "Coder");
    }
}
