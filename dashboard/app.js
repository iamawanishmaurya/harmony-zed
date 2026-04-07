const state = {
  status: null,
  agents: [],
  overlaps: [],
  files: [],
  memory: [],
  logs: [],
  config: null,
  ws: null,
};

async function fetchJson(url, options) {
  const response = await fetch(url, options);
  const data = await response.json();
  if (!response.ok) {
    throw new Error(data.error || `Request failed: ${response.status}`);
  }
  return data;
}

function setHeroStatus(text) {
  const target = document.getElementById("hero-status");
  if (target) {
    target.textContent = text;
  }
}

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

function formatTimestamp(value) {
  if (!value) return "unknown";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function normalizeActorId(actorId) {
  if (Array.isArray(actorId)) return actorId[0] || "unknown";
  if (typeof actorId === "string" && actorId.trim()) return actorId;
  return "unknown";
}

function formatActor(change) {
  if (!change) return "unknown";
  const actorId = normalizeActorId(change.actor_id);
  if (actorId.includes("@")) return actorId;
  if (!change.machine_name || change.machine_name === "local") return actorId;
  return `${actorId}@${change.machine_name}`;
}

function overlapRange(overlap) {
  const start =
    Math.min(
      overlap.region_a?.start_line ?? 0,
      overlap.region_b?.start_line ?? overlap.region_a?.start_line ?? 0
    ) + 1;
  const end =
    Math.max(
      overlap.region_a?.end_line ?? 0,
      overlap.region_b?.end_line ?? overlap.region_a?.end_line ?? 0
    ) + 1;
  return { start, end };
}

function formatOverlapRange(overlap) {
  const { start, end } = overlapRange(overlap);
  return start === end ? `${start}` : `${start}-${end}`;
}

function buildOverlapSummary(overlap) {
  const actorA = formatActor(overlap.change_a);
  const actorB = formatActor(overlap.change_b);
  const { start, end } = overlapRange(overlap);
  const promptA = overlap.change_a?.task_prompt?.trim();
  const promptB = overlap.change_b?.task_prompt?.trim();
  const summary = [
    `${actorA} and ${actorB} both changed ${overlap.file_path} around lines ${start}-${end}.`,
  ];

  if (promptA || promptB) {
    const tasks = [];
    if (promptA) tasks.push(`${actorA} was working on "${promptA}"`);
    if (promptB) tasks.push(`${actorB} was working on "${promptB}"`);
    summary.push(`${tasks.join(" while ")}.`);
  } else {
    summary.push(
      "No task rationale was recorded for these edits yet, so Harmony can only show the overlap metadata."
    );
  }

  return summary.join(" ");
}

function renderStatus() {
  const target = document.getElementById("status-grid");
  if (!target) return;
  if (!state.status) {
    target.innerHTML = `<div class="empty">Loading status…</div>`;
    return;
  }

  const machines = state.status.connected_machines || [];
  const hostUrl = state.status.host_url || "not configured";
  const services = `
    <div class="metric"><span>Mode</span><strong>${escapeHtml(state.status.mode || "unknown")}</strong></div>
    <div class="metric"><span>Project</span><strong>${escapeHtml(state.status.project_root || "unknown")}</strong></div>
    <div class="metric"><span>MCP</span><strong>${escapeHtml(hostUrl)}</strong></div>
    <div class="metric"><span>IPC</span><strong>127.0.0.1:${state.status.ports?.ipc ?? "?"}</strong></div>
    <div class="metric"><span>Dashboard</span><strong>http://${escapeHtml(state.status.machine_ip || "localhost")}:${state.status.ports?.web ?? "?"}</strong></div>
    <div class="metric"><span>Uptime</span><strong>${state.status.uptime_seconds ?? 0}s</strong></div>
  `;
  const machineCards = machines.length
    ? machines
        .map(
          (machine) => `
            <article class="card machine-card">
              <div class="card-top">
                <h4>${escapeHtml(machine.name)}</h4>
                <span class="pill ${machine.status === "online" ? "ok" : "warn"}">${escapeHtml(machine.status)}</span>
              </div>
              <p>${escapeHtml(machine.role)} • ${escapeHtml(machine.ip)}</p>
              <p>${machine.agent_count ?? 0} tracked agents • last seen ${formatTimestamp(machine.last_seen)}</p>
            </article>
          `
        )
        .join("")
    : `<div class="empty">No machines registered yet.</div>`;

  target.innerHTML = `
    <div class="metrics">${services}</div>
    <div class="subheading">Connected Machines</div>
    <div class="card-grid">${machineCards}</div>
  `;
}

function renderAgents() {
  const target = document.getElementById("agents-grid");
  if (!target) return;
  if (!state.agents.length) {
    target.innerHTML = `<div class="empty">No agents registered yet.</div>`;
    return;
  }

  target.innerHTML = state.agents
    .map(
      (agent) => `
        <article class="card">
          <div class="card-top">
            <h4>${escapeHtml(agent.role?.name || normalizeActorId(agent.actor_id) || "Agent")}</h4>
            <span class="pill">${escapeHtml(String(agent.status || "unknown"))}</span>
          </div>
          <p>${escapeHtml(normalizeActorId(agent.actor_id))}</p>
          <p>${escapeHtml(agent.machine_name || "local")} • ${escapeHtml(agent.machine_ip || "127.0.0.1")}</p>
          <p>${escapeHtml(agent.task_prompt || "No active task prompt.")}</p>
        </article>
      `
    )
    .join("");
}

function overlapActionButton(overlapId, resolution, label) {
  return `<button class="small-button" data-overlap-id="${escapeHtml(overlapId)}" data-resolution="${escapeHtml(resolution)}">${escapeHtml(label)}</button>`;
}

function overlapCard(overlap, resolved) {
  const title = `${escapeHtml(overlap.file_path)} • ${formatOverlapRange(overlap)}`;
  const statusText =
    typeof overlap.status === "object"
      ? Object.keys(overlap.status)[0]
      : String(overlap.status || "pending");
  const actions = resolved
    ? ""
    : `<div class="button-row">
        ${overlapActionButton(overlap.id, "accept_a", "Keep A")}
        ${overlapActionButton(overlap.id, "accept_b", "Keep B")}
        ${overlapActionButton(overlap.id, "manual", "Manual")}
        ${overlapActionButton(overlap.id, "negotiate", "Negotiate")}
      </div>`;

  return `
    <article class="card overlap-card">
      <div class="card-top">
        <h4>${title}</h4>
        <span class="pill ${resolved ? "ok" : "warn"}">${escapeHtml(statusText)}</span>
      </div>
      <p class="summary-line">${escapeHtml(buildOverlapSummary(overlap))}</p>
      <p>${escapeHtml(formatActor(overlap.change_a))} vs ${escapeHtml(formatActor(overlap.change_b))}</p>
      <p>Detected ${formatTimestamp(overlap.detected_at)}</p>
      ${actions}
    </article>
  `;
}

function renderOverlaps() {
  const pendingTarget = document.getElementById("overlaps-pending");
  const resolvedTarget = document.getElementById("overlaps-resolved");
  if (!pendingTarget || !resolvedTarget) return;

  const pending = state.overlaps.filter((overlap) => overlap.status === "pending");
  const resolved = state.overlaps.filter((overlap) => overlap.status !== "pending");

  pendingTarget.innerHTML = pending.length
    ? pending.map((overlap) => overlapCard(overlap, false)).join("")
    : `<div class="empty">No pending overlaps.</div>`;
  resolvedTarget.innerHTML = resolved.length
    ? resolved.map((overlap) => overlapCard(overlap, true)).join("")
    : `<div class="empty">No resolved overlaps yet.</div>`;
}

function renderFiles() {
  const summaryTarget = document.getElementById("files-summary");
  const gridTarget = document.getElementById("files-grid");
  if (!summaryTarget || !gridTarget) return;

  if (!state.files.length) {
    summaryTarget.innerHTML = `<div class="empty">No shared file activity recorded yet.</div>`;
    gridTarget.innerHTML = "";
    return;
  }

  const createdFiles = state.files.filter(
    (event) => event.entry_kind === "file" && event.change_kind === "created"
  ).length;
  const createdFolders = state.files.filter(
    (event) => event.entry_kind === "directory" && event.change_kind === "created"
  ).length;
  const updatedFiles = state.files.filter(
    (event) => event.entry_kind === "file" && event.change_kind === "updated"
  ).length;
  const deletedItems = state.files.filter((event) => event.change_kind === "deleted").length;

  summaryTarget.innerHTML = `
    <div class="metrics">
      <div class="metric"><span>New Files</span><strong>${createdFiles}</strong></div>
      <div class="metric"><span>New Folders</span><strong>${createdFolders}</strong></div>
      <div class="metric"><span>Updated Files</span><strong>${updatedFiles}</strong></div>
      <div class="metric"><span>Deleted Items</span><strong>${deletedItems}</strong></div>
    </div>
  `;

  gridTarget.innerHTML = state.files
    .slice()
    .reverse()
    .map((event) => {
      const kind = event.entry_kind === "directory" ? "Folder" : "File";
      const change =
        event.change_kind === "created"
          ? "Created"
          : event.change_kind === "deleted"
            ? "Deleted"
            : "Updated";
      const pillClass =
        event.change_kind === "created"
          ? "ok"
          : event.change_kind === "deleted"
            ? "warn"
            : "sync";

      return `
        <article class="card file-card">
          <div class="card-top">
            <h4>${escapeHtml(event.relative_path)}</h4>
            <span class="pill ${pillClass}">${escapeHtml(change)}</span>
          </div>
          <p>${kind} • ${escapeHtml(event.machine_name || "unknown machine")} • ${formatTimestamp(event.detected_at)}</p>
          <p class="summary-line">${escapeHtml(event.impact_summary || `${change} ${kind.toLowerCase()} in the shared project.`)}</p>
          <p>${escapeHtml(normalizeActorId(event.actor_id))}</p>
        </article>
      `;
    })
    .join("");
}

function renderMemory() {
  const target = document.getElementById("memory-grid");
  if (!target) return;
  if (!state.memory.length) {
    target.innerHTML = `<div class="empty">No shared memory records yet.</div>`;
    return;
  }

  target.innerHTML = state.memory
    .map(
      (record) => `
        <article class="card">
          <div class="tag-row">${(record.tags || [])
            .map((tag) => `<span class="tag">${escapeHtml(tag)}</span>`)
            .join("")}</div>
          <p>${escapeHtml(record.content || "")}</p>
          <small>${formatTimestamp(record.created_at)}</small>
        </article>
      `
    )
    .join("");
}

function renderLogs() {
  const target = document.getElementById("logs-output");
  if (!target) return;
  if (!state.logs.length) {
    target.textContent = "Waiting for Harmony logs…";
    return;
  }

  target.innerHTML = state.logs
    .slice(-500)
    .map((line) => {
      if (typeof line === "string") {
        return `<div class="log-line">${escapeHtml(line)}</div>`;
      }
      const level = line.level || "INFO";
      return `<div class="log-line ${level.toLowerCase()}">[${escapeHtml(line.ts || "")}] ${escapeHtml(level)} [${escapeHtml(line.module || "mcp")}] ${escapeHtml(line.msg || "")}</div>`;
    })
    .join("");
  target.scrollTop = target.scrollHeight;
}

function renderConfig() {
  const target = document.getElementById("config-output");
  if (!target) return;
  target.textContent = state.config?.content || "Configuration not loaded yet.";
}

function attachOverlapButtons() {
  document.querySelectorAll("[data-overlap-id]").forEach((button) => {
    button.addEventListener("click", async () => {
      button.setAttribute("disabled", "disabled");
      try {
        await fetchJson("/api/resolve", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            overlap_id: button.dataset.overlapId,
            resolution: button.dataset.resolution,
          }),
        });
        await refreshOverlaps();
        setHeroStatus(`Resolved overlap ${button.dataset.overlapId}`);
      } catch (error) {
        setHeroStatus(`Resolve failed: ${error.message}`);
      } finally {
        button.removeAttribute("disabled");
      }
    });
  });
}

async function refreshStatus() {
  state.status = await fetchJson("/api/status");
  renderStatus();
}

async function refreshAgents() {
  const data = await fetchJson("/api/agents");
  state.agents = data.agents || [];
  renderAgents();
}

async function refreshOverlaps() {
  const data = await fetchJson("/api/overlaps");
  state.overlaps = data.overlaps || [];
  renderOverlaps();
  attachOverlapButtons();
}

async function refreshFiles() {
  const data = await fetchJson("/api/files");
  state.files = data.events || [];
  renderFiles();
}

async function refreshMemory() {
  const data = await fetchJson("/api/memory");
  state.memory = data.records || [];
  renderMemory();
}

async function refreshLogs() {
  const data = await fetchJson("/api/logs");
  state.logs = data.lines || [];
  renderLogs();
}

async function refreshConfig() {
  state.config = await fetchJson("/api/config");
  renderConfig();
}

async function refreshAll() {
  setHeroStatus("Refreshing dashboard…");
  try {
    await Promise.all([
      refreshStatus(),
      refreshAgents(),
      refreshOverlaps(),
      refreshFiles(),
      refreshMemory(),
      refreshLogs(),
      refreshConfig(),
    ]);
    setHeroStatus("Live");
  } catch (error) {
    setHeroStatus(`Refresh failed: ${error.message}`);
  }
}

function pushLogEvent(event) {
  state.logs.push(event);
  if (state.logs.length > 500) {
    state.logs = state.logs.slice(-500);
  }
  renderLogs();
}

function connectWs() {
  const protocol = window.location.protocol === "https:" ? "wss" : "ws";
  const url = `${protocol}://${window.location.host}/ws`;
  state.ws = new WebSocket(url);

  state.ws.addEventListener("open", () => {
    setHeroStatus("Live stream connected");
  });

  state.ws.addEventListener("message", async (event) => {
    const payload = JSON.parse(event.data);
    switch (payload.type) {
      case "log":
        pushLogEvent(payload);
        break;
      case "machine_update":
        await refreshStatus();
        break;
      case "agent_update":
        await refreshAgents();
        break;
      case "overlap":
        await refreshOverlaps();
        break;
      case "file_sync":
        await refreshFiles();
        break;
      case "memory_added":
        await refreshMemory();
        break;
      case "error":
        pushLogEvent({
          level: "ERROR",
          module: "ws",
          msg: payload.message || "Dashboard action failed",
          ts: new Date().toLocaleTimeString(),
        });
        break;
      default:
        break;
    }
  });

  state.ws.addEventListener("close", () => {
    setHeroStatus("Live stream disconnected, retrying…");
    window.setTimeout(connectWs, 1500);
  });
}

document.getElementById("refresh")?.addEventListener("click", refreshAll);
refreshAll();
connectWs();
window.setInterval(() => {
  refreshStatus().catch(() => {});
  refreshAgents().catch(() => {});
  refreshOverlaps().catch(() => {});
  refreshFiles().catch(() => {});
  refreshMemory().catch(() => {});
}, 5000);
