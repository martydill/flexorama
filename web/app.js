const READONLY_TOOLS = ["search_in_files", "glob"];
const TODO_TOOLS = ["create_todo", "complete_todo", "list_todos"];

const CHART_COLORS = {
  neonGreen: '#39ff14',
  blue: '#6b7cff',
  pink: '#ff4fd8',
  cyan: '#7cffb2',
  purple: '#a855f7',
  orange: '#fb923c',
  yellow: '#fbbf24',
};

function getChartDefaults() {
  const isLight = state.theme === 'light';
  return {
    responsive: true,
    maintainAspectRatio: true,
    plugins: {
      legend: {
        labels: {
          color: isLight ? '#1a1a1a' : '#e6eaff',
          font: {
            family: "'Chakra Petch', sans-serif",
            size: 11,
          },
        },
      },
      tooltip: {
        backgroundColor: isLight ? 'rgba(255, 255, 255, 0.95)' : 'rgba(11, 16, 26, 0.95)',
        titleColor: isLight ? '#1a1a1a' : '#e6eaff',
        bodyColor: isLight ? '#1a1a1a' : '#e6eaff',
        borderColor: isLight ? 'rgba(0, 0, 0, 0.1)' : 'rgba(148, 163, 184, 0.22)',
        borderWidth: 1,
        padding: 10,
        titleFont: {
          family: "'Chakra Petch', sans-serif",
          size: 12,
        },
        bodyFont: {
          family: "'JetBrains Mono', monospace",
          size: 11,
        },
      },
    },
    scales: {
      x: {
        ticks: { color: isLight ? '#333333' : 'rgba(226, 232, 240, 0.68)' },
        grid: { color: isLight ? 'rgba(0, 0, 0, 0.1)' : 'rgba(148, 163, 184, 0.12)' },
      },
      y: {
        ticks: { color: isLight ? '#333333' : 'rgba(226, 232, 240, 0.68)' },
        grid: { color: isLight ? 'rgba(0, 0, 0, 0.1)' : 'rgba(148, 163, 184, 0.12)' },
      },
    },
  };
}

const state = {
  conversations: [],
  activeConversationId: localStorage.getItem("flexorama-active-conversation"),
  plans: [],
  activePlanId: localStorage.getItem("flexorama-active-plan"),
  mcpServers: [],
  activeServer: localStorage.getItem("flexorama-active-mcp"),
  agents: [],
  activeAgent: null,
  activeAgentEditing: localStorage.getItem("flexorama-active-agent-edit"),
  skills: [],
  activeSkillEditing: localStorage.getItem("flexorama-active-skill-edit"),
  commands: [],
  activeCommandEditing: localStorage.getItem("flexorama-active-command-edit"),
  provider: null,
  models: [],
  activeModel: null,
  theme: "dark",
  activeTab: "chats",
  streaming: true,
  planMode: false,
  pendingPermissions: new Set(),
  pendingImages: [],
  todos: [],
  statsCharts: {
    tokens: null,
    requests: null,
    conversations: null,
    models: null,
    providers: null,
    conversationsByProvider: null,
    conversationsTimeByProvider: null,
    subagents: null,
    conversationsTimeBySubagent: null,
  },
  statsData: {
    overview: null,
    usage: null,
    models: null,
    conversations: null,
    conversationsByProvider: null,
    conversationsBySubagent: null,
  },
  statsPeriod: localStorage.getItem("flexorama-stats-period") || "month",
  lastNonCustomPeriod: localStorage.getItem("flexorama-stats-last-period") || "month",
  statsStartDate: null,
  statsEndDate: null,
  todosCollapsed: localStorage.getItem("flexorama-todos-collapsed") === "true",
  conversationSearch: "",
  conversationSearchResults: null,
  conversationSearchLoading: false,
  conversationSearchLastSent: "",
  conversationPagination: {
    offset: 0,
    limit: 10,
    hasMore: true,
    isLoadingMore: false,
  },
  csrfToken: null,
};

let conversationSearchTimer = null;
let conversationSearchController = null;

// Per-conversation streaming state
// Map of conversationId -> { abortController, reader, messages: [], isStreaming: boolean }
const conversationStreams = new Map();

function setPlanForm(plan) {
  state.activePlanId = plan.id;
  localStorage.setItem("flexorama-active-plan", String(plan.id));
  document.getElementById("plan-title").value = plan.title || "";
  document.getElementById("plan-user-request").value = plan.user_request || "";
  document.getElementById("plan-markdown").value = plan.plan_markdown || "";
  renderPlanList();
}

function applyTheme(theme) {
  state.theme = theme;
  if (theme === "light") {
    document.documentElement.classList.add("light");
  } else {
    document.documentElement.classList.remove("light");
  }
  const btn = document.getElementById("mode-toggle");
  if (btn) {
    btn.textContent = theme === "light" ? "☾" : "☀";
    btn.title = theme === "light" ? "Switch to dark mode" : "Switch to light mode";
    btn.setAttribute("aria-label", btn.title);
  }
  localStorage.setItem("flexorama-theme", theme);

  // Update stats charts if on stats tab
  if (state.activeTab === "stats" && state.statsData.overview) {
    updateStatsCharts();
  }
}

function setStatus(text) {
  document.getElementById("conversation-meta").textContent = text;
}

function setConversationSearchLoading(isLoading) {
  state.conversationSearchLoading = isLoading;
  const spinner = document.getElementById("conversation-search-spinner");
  if (spinner) spinner.classList.toggle("visible", isLoading);
}

function isTodoTool(name) {
  return !!name && TODO_TOOLS.includes(name);
}

async function api(path, options = {}) {
  const opts = { headers: { "Content-Type": "application/json" }, ...options };
  if (opts.body && typeof opts.body !== "string") {
    opts.body = JSON.stringify(opts.body);
  }

  // Add CSRF token for state-changing operations
  const method = (opts.method || "GET").toUpperCase();
  if (["POST", "PUT", "DELETE"].includes(method) && state.csrfToken) {
    opts.headers["X-CSRF-Token"] = state.csrfToken;
  }

  const res = await fetch(path, opts);
  if (!res.ok) {
    const message = await res.text();
    throw new Error(message || `Request failed: ${res.status}`);
  }
  const contentType = res.headers.get("content-type") || "";
  if (contentType.includes("application/json")) {
    return res.json();
  }
  return res.text();
}

function setTodos(todos) {
  state.todos = Array.isArray(todos) ? todos : [];
  renderTodoPane();
}

async function loadTodos() {
  try {
    const suffix = state.activeConversationId
      ? `?conversation_id=${encodeURIComponent(state.activeConversationId)}`
      : "";
    const todos = await api(`/api/todos${suffix}`);
    setTodos(todos);
  } catch (_) {
    setTodos([]);
  }
}

function renderTodoPane() {
  const pane = document.getElementById("todo-pane");
  const list = document.getElementById("todo-list");
  const count = document.getElementById("todo-count");
  const toggle = document.getElementById("todo-toggle");
  if (!pane || !list || !count) return;
  const todos = Array.isArray(state.todos) ? state.todos : [];
  const hasPending = todos.some((todo) => !todo.completed);
  if (!hasPending) {
    pane.classList.remove("visible");
    return;
  }
  pane.classList.add("visible");
  pane.classList.toggle("collapsed", state.todosCollapsed);
  if (toggle) {
    toggle.textContent = state.todosCollapsed ? "Expand" : "Collapse";
  }
  count.textContent = String(todos.length);
  list.innerHTML = "";
  const pending = todos.filter((todo) => !todo.completed);
  const completed = todos.filter((todo) => todo.completed);
  const ordered = pending.concat(completed);
  const visible = ordered.slice(0, 10);
  const remaining = ordered.length - visible.length;

  visible.forEach((todo) => {
    const item = document.createElement("div");
    item.className = "todo-item";
    const check = document.createElement("span");
    check.className = "todo-check" + (todo.completed ? " complete" : "");
    check.textContent = todo.completed ? "[✓]" : "[ ]";
    const text = document.createElement("div");
    text.className = "todo-text" + (todo.completed ? " complete" : "");
    text.textContent = todo.description || "";
    item.appendChild(check);
    item.appendChild(text);
    list.appendChild(item);
  });

  if (remaining > 0) {
    const item = document.createElement("div");
    item.className = "todo-item";
    const spacer = document.createElement("span");
    spacer.className = "todo-check";
    spacer.textContent = "";
    const text = document.createElement("div");
    text.className = "todo-text";
    text.textContent = `...(${remaining} more)...`;
    item.appendChild(spacer);
    item.appendChild(text);
    list.appendChild(item);
  }
}

function getConversationSearchTerm() {
  return (state.conversationSearch || "").trim().toLowerCase();
}

function renderConversationList() {
  const list = document.getElementById("conversation-list");
  list.innerHTML = "";
  const query = getConversationSearchTerm();
  if (query && state.conversationSearchLoading && !state.conversationSearchResults) {
    const item = document.createElement("div");
    item.className = "list-item empty";
    item.textContent = "Searching...";
    list.appendChild(item);
    return;
  }
  const filtered = query ? state.conversationSearchResults || [] : state.conversations;

  if (filtered.length === 0 && !state.conversationPagination.isLoadingMore) {
    const empty = document.createElement("div");
    empty.className = "list-item empty";
    empty.textContent = query ? "No conversations match your search." : "No conversations yet.";
    list.appendChild(empty);
    return;
  }

  filtered.forEach((conv) => {
    const item = document.createElement("div");
    const isActive = String(conv.id) === String(state.activeConversationId);
    const isStreaming = conversationStreams.has(String(conv.id)) && conversationStreams.get(String(conv.id)).isStreaming;
    item.className = "list-item" + (isActive ? " active" : "");
    const streamingIndicator = isStreaming ? '<span style="color: var(--accent-neon); margin-left: 6px;" title="Streaming">●</span>' : '';
    item.innerHTML = `
      <div style="font-weight:600;">${conv.last_message ? conv.last_message.slice(0, 50) : "new chat"}${streamingIndicator}</div>
      <small>${new Date(conv.updated_at).toLocaleString()} • ${conv.model}</small>
    `;
    item.addEventListener("click", () => selectConversation(conv.id));
    list.appendChild(item);
  });

  // Add loading spinner at the bottom if loading more or if there are more to load
  if (!query && (state.conversationPagination.isLoadingMore || state.conversationPagination.hasMore)) {
    const spinner = document.createElement("div");
    spinner.className = "list-item empty";
    spinner.id = "conversation-load-more-spinner";
    if (state.conversationPagination.isLoadingMore) {
      spinner.innerHTML = `<div class="loading-spinner"></div>`;
    } else {
      spinner.textContent = "Scroll to load more...";
      spinner.style.opacity = "0.6";
    }
    list.appendChild(spinner);
  }
}

async function performConversationSearch(query) {
  const trimmed = query.trim();
  if (!trimmed) {
    state.conversationSearchResults = null;
    state.conversationSearchLastSent = "";
    setConversationSearchLoading(false);
    renderConversationList();
    return;
  }
  if (trimmed === state.conversationSearchLastSent) {
    setConversationSearchLoading(false);
    return;
  }

  if (conversationSearchController) {
    conversationSearchController.abort();
  }
  conversationSearchController = new AbortController();
  setConversationSearchLoading(true);
  state.conversationSearchResults = null;
  renderConversationList();

  try {
    const results = await api(
      `/api/conversations/search?query=${encodeURIComponent(trimmed)}`,
      { signal: conversationSearchController.signal },
    );
    if (state.conversationSearch.trim() === trimmed) {
      state.conversationSearchResults = Array.isArray(results) ? results : [];
      state.conversationSearchLastSent = trimmed;
    }
  } catch (err) {
    if (err?.name !== "AbortError") {
      console.error("Conversation search failed:", err);
      state.conversationSearchResults = [];
    }
  } finally {
    if (state.conversationSearch.trim() === trimmed) {
      setConversationSearchLoading(false);
      renderConversationList();
    }
  }
}

function formatJson(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch (err) {
    return String(value);
  }
}

function normalizeBlocks(blocks, fallback) {
  if (Array.isArray(blocks) && blocks.length) {
    return blocks.map((b) => ({
      ...b,
      type: b.type || b.block_type,
    }));
  }
  return [{ type: "text", text: fallback || "" }];
}

function renderBlock(block) {
  const blockType = block.type || "text";
  const wrapper = document.createElement("div");

  if (blockType === "tool_use") {
    wrapper.className = "tool-block tool-call";
    const head = document.createElement("div");
    head.className = "tool-head";
    const title = document.createElement("div");
    title.className = "tool-title";
    title.textContent = `🔧 ${block.name || "tool call"}`;
    const toggle = document.createElement("button");
    toggle.className = "tool-toggle";
    toggle.textContent = "Expand";
    head.appendChild(title);
    head.appendChild(toggle);
    const details = document.createElement("div");
    details.className = "tool-details tool-details-row";
    details.textContent = summarizeToolInput(block.name, block.input);
    const body = document.createElement("div");
    body.className = "tool-body";
    const pre = document.createElement("pre");
    pre.textContent = formatJson(block.input);
    body.appendChild(pre);
    const toggleOpen = () => {
      const open = wrapper.classList.toggle("open");
      body.style.display = open ? "block" : "none";
      toggle.textContent = open ? "Collapse" : "Expand";
    };
    head.addEventListener("click", toggleOpen);
    toggle.addEventListener("click", (e) => {
      e.stopPropagation();
      toggleOpen();
    });
    wrapper.appendChild(head);
    wrapper.appendChild(details);
    wrapper.appendChild(body);
    return wrapper;
  }

  if (blockType === "tool_result") {
    wrapper.className = "tool-block tool-result" + (block.is_error ? " error" : "");
    
    const head = document.createElement("div");
    head.className = "tool-head";
    const title = document.createElement("div");
    title.className = "tool-title";
    title.textContent = block.is_error ? "⚠️ Tool error" : "📤 Tool result";
    const toggle = document.createElement("button");
    toggle.className = "tool-toggle";
    toggle.textContent = "Expand";
    head.appendChild(title);
    head.appendChild(toggle);
    const body = document.createElement("div");
    body.className = "tool-body";
    const pre = document.createElement("pre");
    pre.textContent = block.content || "(empty result)";
    body.appendChild(pre);
    const toggleOpen = () => {
      const open = wrapper.classList.toggle("open");
      body.style.display = open ? "block" : "none";
      toggle.textContent = open ? "Collapse" : "Expand";
    };
    head.addEventListener("click", toggleOpen);
    toggle.addEventListener("click", (e) => {
      e.stopPropagation();
      toggleOpen();
    });
    wrapper.appendChild(head);
    wrapper.appendChild(body);
    return wrapper;
  }

  if (blockType === "permission_request") {
    wrapper.className = "permission-block";
    if (block.id) {
      wrapper.dataset.permissionId = block.id;
    }
    const title = document.createElement("div");
    title.className = "permission-title";
    title.textContent = block.title || "Permission request";
    const detail = document.createElement("div");
    detail.className = "permission-detail";
    detail.textContent = block.detail || "";
    const actions = document.createElement("div");
    actions.className = "permission-actions";
    const options = Array.isArray(block.options) ? block.options : [];
    options.forEach((option, idx) => {
      const btn = document.createElement("button");
      btn.className = "permission-option";
      btn.textContent = option;
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        submitPermissionSelection(block.id, idx, wrapper);
      });
      actions.appendChild(btn);
    });
    const status = document.createElement("div");
    status.className = "permission-status muted";
    wrapper.appendChild(title);
    if (detail.textContent) wrapper.appendChild(detail);
    wrapper.appendChild(actions);
    wrapper.appendChild(status);
    return wrapper;
  }

  if (blockType === "image") {
    wrapper.className = "image-block";
    const img = document.createElement("img");
    if (block.source) {
      const dataUrl = `data:${block.source.media_type};base64,${block.source.data}`;
      img.src = dataUrl;
      img.style.maxWidth = "400px";
      img.style.maxHeight = "400px";
      img.style.borderRadius = "8px";
      img.style.border = "1px solid var(--border-color)";
      img.style.cursor = "pointer";
      img.onclick = () => {
        // Open image in new tab when clicked
        const newTab = window.open();
        newTab.document.body.innerHTML = `<img src="${dataUrl}" style="max-width: 100%; height: auto;">`;
      };
    }
    wrapper.appendChild(img);
    return wrapper;
  }

  wrapper.className = "text-block";
  const text = block.text || block.content || "";
  wrapper.appendChild(renderTextContent(text));
  return wrapper;
}

function renderTextContent(text) {
  const container = document.createElement("div");
  const regex = /```(\w+)?\n([\s\S]*?)```/g;
  let lastIndex = 0;
  let match;
  while ((match = regex.exec(text)) !== null) {
    const preceding = text.slice(lastIndex, match.index);
    if (preceding.trim()) {
      const p = document.createElement("div");
      p.textContent = preceding;
      container.appendChild(p);
    }
    const lang = match[1] || "";
    const codeContent = match[2];
    const pre = document.createElement("pre");
    const code = document.createElement("code");
    code.className = `language-${lang || "plaintext"}`;
    code.textContent = codeContent;
    pre.appendChild(code);
    container.appendChild(pre);
    lastIndex = regex.lastIndex;
  }
  const tail = text.slice(lastIndex);
  if (tail.trim() || (!match && text)) {
    const p = document.createElement("div");
    p.textContent = tail;
    container.appendChild(p);
  }
  return container;
}

function renderMessageBubble(msg) {
  const bubble = document.createElement("div");
  bubble.className = `bubble ${msg.role}`;
  const blocks = normalizeBlocks(msg.blocks, msg.content).filter(
    (b) =>
      !(
        (b.type === "tool_use" || b.type === "tool_result") &&
        isTodoTool(b.name)
      ),
  );
  const hasToolResult = blocks.some((b) => (b.type || b.block_type) === "tool_result");
  if (!blocks.length) return null;
  const hasVisible = blocks.some(
    (b) =>
      (b.text && b.text.trim()) ||
      (b.content && b.content.trim()) ||
      b.type === "tool_use" ||
      b.type === "tool_result" ||
      b.type === "permission_request",
  );
  if (!hasVisible && !(msg.content && msg.content.trim())) {
    return null;
  }
  if (hasToolResult) {
    bubble.classList.add("tool-result-bubble");
  }
  blocks.forEach((block) => bubble.appendChild(renderBlock(block)));

  if (msg.role === "assistant") {
    const text = blocks
      .filter((b) => b.type === "text" || !b.type)
      .map((b) => b.text || b.content || "")
      .join("\n");
    checkAndAppendPlanButton(bubble, text);
  }

  return bubble;
}

function renderMessages(messages) {
  const container = document.getElementById("messages");
  container.innerHTML = "";
  messages.forEach((msg) => {
    const bubble = renderMessageBubble(msg);
    if (bubble) container.appendChild(bubble);
  });
  container.scrollTop = container.scrollHeight;
  highlightCodes(container);
}

function appendMessage(role, content, blocks = null) {
  const container = document.getElementById("messages");
  const bubble = renderMessageBubble({ role, content, blocks });
  if (bubble) {
    container.appendChild(bubble);
    container.scrollTop = container.scrollHeight;
    highlightCodes(bubble);
  }
  return bubble;
}

function createEmptyBubble(role) {
  const container = document.getElementById("messages");
  const bubble = document.createElement("div");
  bubble.className = `bubble ${role}`;
  container.appendChild(bubble);
  container.scrollTop = container.scrollHeight;
  return bubble;
}

function getActiveToolStreamBubble() {
  const container = document.getElementById("messages");
  return container.querySelector(".bubble.assistant[data-tool-stream=\"true\"]:last-child");
}

function renderPermissionRequest(request) {
  if (!request || !request.id) return;
  if (state.pendingPermissions.has(request.id)) return;
  state.pendingPermissions.add(request.id);
  appendMessage("assistant", "", [
    {
      type: "permission_request",
      id: request.id,
      title: request.title,
      detail: request.detail,
      options: request.options || [],
    },
  ]);
}

async function submitPermissionSelection(id, selection, wrapper) {
  if (!id) return;
  const status = wrapper ? wrapper.querySelector(".permission-status") : null;
  const buttons = wrapper ? wrapper.querySelectorAll(".permission-option") : [];
  let previousDisplay = null;
  if (wrapper) {
    previousDisplay = wrapper.style.display;
    wrapper.style.display = "none";
  }
  buttons.forEach((btn) => (btn.disabled = true));
  if (status) status.textContent = "Submitting response...";
  try {
    await api("/api/permissions/respond", {
      method: "POST",
      body: { id, selection },
    });
    if (status) status.textContent = "Response sent.";
    state.pendingPermissions.delete(id);
    if (wrapper) {
      const bubble = wrapper.closest(".bubble");
      wrapper.remove();
      if (bubble && bubble.children.length === 0) {
        bubble.remove();
      }
    }
  } catch (err) {
    if (wrapper) wrapper.style.display = previousDisplay || "";
    if (status) status.textContent = `Failed: ${err.message}`;
    buttons.forEach((btn) => (btn.disabled = false));
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function formatDateInput(date) {
  return date.toISOString().slice(0, 10);
}

function getDateRangeForPeriod(period) {
  const now = new Date();
  const end = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()));
  let start;

  switch (period) {
    case "day":
      start = new Date(end);
      start.setUTCDate(start.getUTCDate() - 1);
      break;
    case "week":
      start = new Date(end);
      start.setUTCDate(start.getUTCDate() - 7);
      break;
    case "month":
      start = new Date(end);
      start.setUTCDate(start.getUTCDate() - 30);
      break;
    case "lifetime":
      start = new Date(Date.UTC(2025, 0, 1));
      break;
    default:
      start = new Date(end);
      start.setUTCDate(start.getUTCDate() - 30);
      break;
  }

  return {
    startDate: formatDateInput(start),
    endDate: formatDateInput(end),
  };
}

function setCustomDatesFromPeriod(period) {
  const range = getDateRangeForPeriod(period);
  state.statsStartDate = range.startDate;
  state.statsEndDate = range.endDate;

  const startInput = document.getElementById("stats-start-date");
  const endInput = document.getElementById("stats-end-date");
  if (startInput) startInput.value = range.startDate;
  if (endInput) endInput.value = range.endDate;
}

function ensureStatsDateRange() {
  if (state.statsPeriod === "custom") {
    if (!state.statsStartDate || !state.statsEndDate) {
      setCustomDatesFromPeriod(state.lastNonCustomPeriod || "month");
    }
  } else {
    setCustomDatesFromPeriod(state.statsPeriod);
  }
}

function startPermissionPolling() {
  if (!state.activeConversationId) return null;
  const controller = { stopped: false };
  const conversationId = state.activeConversationId;
  (async () => {
    while (!controller.stopped) {
      try {
        const pending = await api(
          `/api/permissions/pending?conversation_id=${encodeURIComponent(conversationId)}`,
        );
        if (Array.isArray(pending)) {
          pending.forEach(renderPermissionRequest);
        }
      } catch (_) {
        // ignore polling errors
      }
      await delay(1000);
    }
  })();
  return controller;
}

async function loadPendingPermissions() {
  if (!state.activeConversationId) return;
  try {
    const pending = await api(
      `/api/permissions/pending?conversation_id=${encodeURIComponent(state.activeConversationId)}`,
    );
    if (Array.isArray(pending)) {
      pending.forEach(renderPermissionRequest);
    }
  } catch (_) {
    // ignore load errors
  }
}

function summarizeToolInput(name, input) {
  let parsed = input;
  if (typeof parsed === "string") {
    try {
      parsed = JSON.parse(parsed);
    } catch (_) {
      // keep string
    }
  }

  const safeVal = (v) => {
    if (v === null || v === undefined) return "";
    if (typeof v === "string") return v;
    try {
      return JSON.stringify(v);
    } catch (_) {
      return String(v);
    }
  };

  if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
    const obj = parsed;
    if (name === "search_in_files") {
      const path = obj.path || ".";
      const query = obj.query || obj.pattern || "";
      return `path=${safeVal(path)} query=${safeVal(query)}`.trim();
    }
    if (name === "read_file") {
      return `path=${safeVal(obj.path || obj.file || "")}`;
    }
    if (name === "write_file" || name === "edit_file") {
      const path = obj.path || obj.file || "";
      const desc = obj.content ? "content=…" : obj.changes ? "changes=…" : "";
      return `path=${safeVal(path)} ${desc}`.trim();
    }
    if (name === "list_directory") {
      return `path=${safeVal(obj.path || ".")}`;
    }
    const keys = Object.keys(obj);
    if (keys.length) {
      return keys
        .slice(0, 3)
        .map((k) => `${k}=${safeVal(obj[k])}`)
        .join(" ");
    }
  }
  return safeVal(parsed) || "(no input)";
}

function checkAndAppendPlanButton(bubble, text) {
  const match = /_Plan saved with ID: `(.*?)`\._/.exec(text);
  if (!match) return;
  if (bubble.querySelector(".plan-message-actions")) return;
  const planId = match[1];
  const actions = document.createElement("div");
  actions.className = "stack plan-message-actions";
  actions.style.marginTop = "8px";

  const editButton = document.createElement("button");
  editButton.className = "secondary";
  editButton.textContent = "Edit Plan";
  editButton.addEventListener("click", async () => {
    const tabBtn = document.querySelector('.top-tab[data-tab="plans"]');
    if (tabBtn) tabBtn.click();
    await loadPlans();
    const plan = state.plans.find((p) => String(p.id) === String(planId));
    if (plan) {
      setPlanForm(plan);
    }
  });

  actions.append(editButton);

  if (!state.planMode) {
    bubble.appendChild(actions);
    return;
  }

  const executeButton = document.createElement("button");
  executeButton.className = "secondary";
  executeButton.textContent = "Execute Plan";
  executeButton.addEventListener("click", async () => {
    await executeSavedPlan(planId, { newChat: false });
  });

  const executeNewChatButton = document.createElement("button");
  executeNewChatButton.className = "secondary";
  executeNewChatButton.textContent = "Execute Plan in New Chat";
  executeNewChatButton.addEventListener("click", async () => {
    await executeSavedPlan(planId, { newChat: true });
  });

  actions.append(executeButton, executeNewChatButton);
  bubble.appendChild(actions);
}

function updateBubbleContent(bubble, text) {
  let target = bubble;
  if (!target) {
    target = document.createElement("div");
    target.className = "bubble assistant";
    const container = document.getElementById("messages");
    container.appendChild(target);
  }
  target.innerHTML = "";
  target.appendChild(renderBlock({ type: "text", text }));
  checkAndAppendPlanButton(target, text);
  target.scrollIntoView({ block: "end" });
  highlightCodes(target);
}

function showTypingIndicator(bubble) {
  if (!bubble) return;
  bubble.innerHTML = "";
  const indicator = document.createElement("div");
  indicator.className = "typing-indicator";
  indicator.innerHTML = "<span></span><span></span><span></span>";
  bubble.appendChild(indicator);
}

function renderToolStreamItem(block) {
  const item = document.createElement("div");
  item.className = "tool-stream-item";
  const title = document.createElement("div");
  title.className = "tool-title";
  const detail = document.createElement("div");
  detail.className = "tool-details tool-details-row";
  const body = document.createElement("div");
  body.className = "tool-body";
  const pre = document.createElement("pre");

  if (block.type === "tool_use") {
    title.textContent = `🛠 ${block.name || "tool call"}`;
    detail.textContent = summarizeToolInput(block.name, block.input);
    pre.textContent = formatJson(block.input);
  } else if (block.type === "tool_result") {
    title.textContent = block.is_error ? "🛠 Tool error" : "🛠 Tool result";
    pre.textContent = block.content || "(empty result)";
  } else {
    title.textContent = "🛠 Tool event";
    pre.textContent = formatJson(block);
  }

  body.appendChild(pre);
  item.appendChild(title);
  if (detail.textContent) item.appendChild(detail);
  item.appendChild(body);
  return item;
}

function renderToolStreamBlock(blocks) {
  const wrapper = document.createElement("div");
  wrapper.className = "tool-block tool-stream";
  const head = document.createElement("div");
  head.className = "tool-head";
  const title = document.createElement("div");
  title.className = "tool-title";
  const toggle = document.createElement("button");
  toggle.className = "tool-toggle";
  toggle.textContent = "Expand";
  head.appendChild(title);
  head.appendChild(toggle);

  const details = document.createElement("div");
  details.className = "tool-details tool-details-row";
  const toolCallCount = blocks.filter((b) => b.type === "tool_use").length;

  const lastToolCall = [...blocks].reverse().find((b) => b.type === "tool_use");
  if (lastToolCall) {
    const countLabel = toolCallCount ? ` (${toolCallCount})` : "";
    title.textContent = `🛠 ${lastToolCall.name || "tool call"}${countLabel}`;
    details.textContent = summarizeToolInput(lastToolCall.name, lastToolCall.input);
  } else {
    const lastBlock = blocks[blocks.length - 1];
    const countLabel = toolCallCount ? ` (${toolCallCount})` : "";
    title.textContent =
      (lastBlock && lastBlock.type === "tool_result" ? "🛠 Tool result" : "🛠 Tool") + countLabel;
  }

  const body = document.createElement("div");
  body.className = "tool-body";
  body.style.display = "none";
  blocks.forEach((block) => body.appendChild(renderToolStreamItem(block)));

  const toggleOpen = () => {
    const open = wrapper.classList.toggle("open");
    body.style.display = open ? "block" : "none";
    toggle.textContent = open ? "Collapse" : "Expand";
  };
  head.addEventListener("click", toggleOpen);
  toggle.addEventListener("click", (e) => {
    e.stopPropagation();
    toggleOpen();
  });

  wrapper.appendChild(head);
  if (details.textContent) wrapper.appendChild(details);
  wrapper.appendChild(body);
  return wrapper;
}

function updateToolStreamBubble(bubble, block) {
  if (!bubble) return;
  let blocks = [];
  if (bubble.dataset.toolBlocks) {
    try {
      blocks = JSON.parse(bubble.dataset.toolBlocks);
    } catch (_) {
      blocks = [];
    }
  }
  blocks.push(block);
  bubble.dataset.toolBlocks = JSON.stringify(blocks);
  bubble.innerHTML = "";
  bubble.appendChild(renderToolStreamBlock(blocks));
  bubble.scrollIntoView({ block: "end" });
  highlightCodes(bubble);
}

function updateBubbleBlock(bubble, block) {
  if (!bubble) return;
  bubble.innerHTML = "";
  bubble.appendChild(renderBlock(block));
  bubble.scrollIntoView({ block: "end" });
  highlightCodes(bubble);
}

function highlightCodes(scope) {
  if (!scope || !window.hljs) return;
  scope.querySelectorAll("pre code").forEach((code) => {
    window.hljs.highlightElement(code);
  });
}

async function loadConversations() {
  // Reset pagination state when loading conversations
  state.conversationPagination = {
    offset: 0,
    limit: 10,
    hasMore: true,
    isLoadingMore: false,
  };
  state.conversations = [];

  const data = await api(`/api/conversations?limit=${state.conversationPagination.limit}&offset=0`);
  mergeConversations(data);

  // Update pagination state
  state.conversationPagination.offset = data.length;
  state.conversationPagination.hasMore = data.length === state.conversationPagination.limit;

  if (getConversationSearchTerm()) {
    await performConversationSearch(state.conversationSearch);
  } else {
    renderConversationList();
  }
  const hasActiveConv = state.activeConversationId &&
    data.some(c => String(c.id) === String(state.activeConversationId));
  if (!hasActiveConv && data.length > 0) {
    await selectConversation(data[0].id);
  }
}

async function loadMoreConversations() {
  if (!state.conversationPagination.hasMore || state.conversationPagination.isLoadingMore) {
    return;
  }

  state.conversationPagination.isLoadingMore = true;
  renderConversationList();

  try {
    const data = await api(
      `/api/conversations?limit=${state.conversationPagination.limit}&offset=${state.conversationPagination.offset}`
    );
    mergeConversations(data);

    // Update pagination state
    state.conversationPagination.offset += data.length;
    state.conversationPagination.hasMore = data.length === state.conversationPagination.limit;
  } catch (err) {
    console.error("Failed to load more conversations:", err);
  } finally {
    state.conversationPagination.isLoadingMore = false;
    renderConversationList();
  }
}

async function selectConversation(id) {
  state.activeConversationId = id;
  localStorage.setItem("flexorama-active-conversation", String(id));
  state.pendingPermissions.clear();
  renderConversationList();

  // Check if this conversation has active streaming
  const streamState = conversationStreams.get(String(id));
  if (streamState && streamState.isStreaming) {
    // Conversation is actively streaming - use cached messages (no API call needed)
    setStatus("Streaming response...");

    // Restore the cached messages HTML
    const messagesContainer = document.getElementById("messages");
    if (messagesContainer && streamState.cachedMessagesHtml !== undefined) {
      messagesContainer.innerHTML = streamState.cachedMessagesHtml;
      messagesContainer.scrollTop = messagesContainer.scrollHeight;
      highlightCodes(messagesContainer);
    }

    // Create a new bubble for the streaming content and populate with current text
    const bubble = createEmptyBubble("assistant");
    if (streamState.currentText) {
      updateBubbleContent(bubble, streamState.currentText);
    } else {
      showTypingIndicator(bubble);
    }

    // Store the new bubble reference so the streaming loop can update it
    streamState.activeBubble = bubble;

    const select = document.getElementById("agent-selector");
    if (select) {
      select.value = streamState.subagent || "";
    }

    await loadModels();
    await loadPendingPermissions();
    await loadTodos();
    return;
  }

  setStatus("Loading conversation...");
  const detail = await api(`/api/conversations/${id}`);
  const meta = detail.conversation;
  setStatus(`${detail.messages.length} messages`);
  renderMessages(detail.messages);
  const select = document.getElementById("agent-selector");
  if (select) {
    select.value = meta.subagent || "";
  }
  await loadModels();
  await loadPendingPermissions();
  await loadTodos();
}

async function createConversation() {
  setStatus("Creating conversation...");
  const res = await api("/api/conversations", { method: "POST", body: {} });
  const newId = res.id;
  const placeholder = {
    id: newId,
    last_message: null,
    updated_at: new Date().toISOString(),
    model: res.model || state.activeModel || state.conversations[0]?.model || "unknown",
    created_at: new Date().toISOString(),
  };
  state.activeConversationId = newId;
  localStorage.setItem("flexorama-active-conversation", String(newId));
  mergeConversations([placeholder]);
  renderConversationList();
  if (newId) {
    await selectConversation(newId);
  }
  const input = document.getElementById("message-input");
  if (input) input.focus();
  await loadModels();
}

async function sendMessage() {
  const input = document.getElementById("message-input");
  const text = input.value.trim();
  if (!text || !state.activeConversationId) return;

  appendMessage("user", text);
  updateConversationPreview(state.activeConversationId, text);
  input.value = "";

  // Capture images before clearing
  const images = state.pendingImages.length > 0 ? [...state.pendingImages] : null;
  clearPendingImages();

  await sendMessageStreaming(text, images);
}

async function sendMessageOnce(text, images = null) {
  setStatus("Waiting for response...");
  const poller = startPermissionPolling();
  try {
    const requestBody = { message: text };
    if (images && images.length > 0) {
      requestBody.images = images.map(img => ({
        media_type: img.media_type,
        data: img.data,
      }));
    }
    const result = await api(`/api/conversations/${state.activeConversationId}/message`, {
      method: "POST",
      body: requestBody,
    });
    appendMessage("assistant", result.response || "(empty response)");
    
    try {
      setStatus("Ready");
      await loadTodos();
      await loadConversations();
    } catch (refreshErr) {
      console.error("Failed to refresh chat:", refreshErr);
      setStatus("Error refreshing chat");
    }
  } catch (err) {
    appendMessage("assistant", `Error: ${err.message}`);
    setStatus("Error");
  } finally {
    if (poller) poller.stopped = true;
  }
}

async function sendMessageStreaming(text, images = null) {
  // Capture the conversation ID at the start - use this throughout the function
  const conversationId = state.activeConversationId;
  const convIdStr = String(conversationId);

  // Helper to check if this conversation is currently being viewed
  const isActiveConversation = () => String(state.activeConversationId) === convIdStr;

  // Capture the current messages HTML before we add the streaming bubble
  // This allows us to restore the conversation state when switching back without an API call
  const messagesContainer = document.getElementById("messages");
  const cachedMessagesHtml = messagesContainer ? messagesContainer.innerHTML : "";

  // Initialize per-conversation streaming state
  const streamState = {
    abortController: new AbortController(),
    reader: null,
    cachedMessagesHtml, // HTML snapshot of messages before streaming response
    isStreaming: true,
    currentText: "",
    subagent: document.getElementById("agent-selector")?.value || "",
  };

  conversationStreams.set(convIdStr, streamState);

  // Update conversation list to show streaming indicator
  renderConversationList();

  if (isActiveConversation()) {
    setStatus("Streaming response...");
  }

  // Create initial bubble if this conversation is active
  if (isActiveConversation()) {
    const bubble = createEmptyBubble("assistant");
    showTypingIndicator(bubble);
    streamState.activeBubble = bubble;
  }

  // Helper to get the current bubble for this conversation (may change if user switches away and back)
  const getActiveBubble = () => {
    if (isActiveConversation() && streamState.activeBubble && document.body.contains(streamState.activeBubble)) {
      return streamState.activeBubble;
    }
    return null;
  };

  let toolBubble = null;
  let buffer = "";
  const poller = startPermissionPolling();

  try {
    const headers = { "Content-Type": "application/json" };

    // Add CSRF token for state-changing operations
    if (state.csrfToken) {
      headers["X-CSRF-Token"] = state.csrfToken;
    }

    const requestBody = { message: text };
    if (images && images.length > 0) {
      requestBody.images = images.map(img => ({
        media_type: img.media_type,
        data: img.data,
      }));
    }

    const res = await fetch(`/api/conversations/${conversationId}/message/stream`, {
      method: "POST",
      headers: headers,
      body: JSON.stringify(requestBody),
      signal: streamState.abortController.signal,
    });

    if (!res.ok) {
      const message = await res.text();
      throw new Error(message || `Request failed: ${res.status}`);
    }
    if (!res.body) {
      throw new Error("Streaming not supported by browser");
    }

    const reader = res.body.getReader();
    streamState.reader = reader;
    const decoder = new TextDecoder();

    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });

      let newlineIndex;
      while ((newlineIndex = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, newlineIndex).trim();
        buffer = buffer.slice(newlineIndex + 1);
        if (!line) continue;

        let evt = null;
        try {
          evt = JSON.parse(line);
        } catch (err) {
          continue;
        }

        if (evt.type === "text" && typeof evt.delta === "string") {
          streamState.currentText += evt.delta;
          // Only update UI if this conversation is active
          const bubble = getActiveBubble();
          if (bubble) {
            updateBubbleContent(bubble, streamState.currentText);
          }
        } else if (evt.type === "final" && typeof evt.content === "string") {
          streamState.currentText = evt.content;
          const bubble = getActiveBubble();
          if (bubble) {
            updateBubbleContent(bubble, streamState.currentText);
          }
        } else if (evt.type === "tool_call") {
          if (isTodoTool(evt.name)) {
            if (isActiveConversation()) {
              await loadTodos();
            }
            continue;
          }
          if (isActiveConversation()) {
            if (!toolBubble || !document.body.contains(toolBubble)) {
              toolBubble = getActiveToolStreamBubble() || createEmptyBubble("assistant");
              toolBubble.dataset.toolStream = "true";
            }
            updateToolStreamBubble(toolBubble, {
              type: "tool_use",
              name: evt.name,
              id: evt.tool_use_id,
              input: evt.input,
            });
          }
        } else if (evt.type === "tool_result") {
          if (isTodoTool(evt.name)) {
            if (isActiveConversation()) {
              await loadTodos();
            }
            continue;
          }
          if (isActiveConversation()) {
            if (!toolBubble || !document.body.contains(toolBubble)) {
              toolBubble = getActiveToolStreamBubble() || createEmptyBubble("assistant");
              toolBubble.dataset.toolStream = "true";
            }
            updateToolStreamBubble(toolBubble, {
              type: "tool_result",
              tool_use_id: evt.tool_use_id,
              content: evt.content,
              is_error: !!evt.is_error,
            });
          }
        } else if (evt.type === "permission_request") {
          if (isActiveConversation()) {
            renderPermissionRequest(evt);
          }
        } else if (evt.type === "error") {
          const bubble = getActiveBubble();
          if (bubble) {
            updateBubbleContent(bubble, `Error: ${evt.error || "stream error"}`);
            setStatus("Error");
          }
        }
      }
    }

    // Streaming complete - refresh conversation
    streamState.isStreaming = false;

    try {
      if (isActiveConversation()) {
        setStatus("Refreshing chat...");
        await selectConversation(conversationId);
        setStatus("Ready");
      }
      await loadConversations();
    } catch (refreshErr) {
      console.error("Failed to refresh chat:", refreshErr);
      if (isActiveConversation()) {
        setStatus("Error refreshing chat");
      }
    }
  } catch (err) {
    streamState.isStreaming = false;
    if (err.name === "AbortError") {
      return;
    }
    const bubble = getActiveBubble();
    if (bubble) {
      if (streamState.currentText) {
        updateBubbleContent(bubble, streamState.currentText + `\n\n**Error:** ${err.message}`);
      } else {
        updateBubbleContent(bubble, `Error: ${err.message}`);
      }
      setStatus("Error");
    }
  } finally {
    if (poller) poller.stopped = true;
    // Clean up streaming state for this conversation
    streamState.isStreaming = false;
    conversationStreams.delete(convIdStr);
    // Update conversation list to remove streaming indicator
    renderConversationList();
  }
}

// Plans
async function loadPlans() {
  const plans = await api("/api/plans");
  state.plans = plans;
  renderPlanList();
  if (state.activePlanId) {
    const current = state.plans.find((p) => String(p.id) === String(state.activePlanId));
    if (current) setPlanForm(current);
  }
}

function renderPlanList() {
  const list = document.getElementById("plan-list");
  list.innerHTML = "";
  state.plans.forEach((plan) => {
    const item = document.createElement("div");
    const isActive = String(plan.id) === String(state.activePlanId);
    item.className = "list-item" + (isActive ? " active" : "");
    item.innerHTML = `
      <div style="font-weight:600;">${plan.title || "Untitled plan"}</div>
      <small>${new Date(plan.created_at).toLocaleString()}</small>
    `;
    item.addEventListener("click", () => {
      setPlanForm(plan);
    });
    list.appendChild(item);
  });
}

function mergeConversations(incoming) {
  const map = new Map();
  incoming.forEach((c) => map.set(c.id, c));
  state.conversations.forEach((c) => {
    if (!map.has(c.id)) map.set(c.id, c);
  });
  const merged = Array.from(map.values());
  merged.sort(
    (a, b) =>
      new Date(b.updated_at || b.created_at || 0) - new Date(a.updated_at || a.created_at || 0),
  );
  state.conversations = merged;
}

function updateConversationPreview(id, lastMessage) {
  if (!id) return;
  const existing = state.conversations.find((c) => String(c.id) === String(id));
  const now = new Date().toISOString();
  if (existing) {
    if (!existing.last_message) {
      existing.last_message = lastMessage;
    }
    existing.updated_at = now;
  } else {
    state.conversations.unshift({
      id,
      last_message: lastMessage,
      updated_at: now,
      created_at: now,
      model: state.activeModel || "unknown",
    });
  }
  renderConversationList();
}

function resetPlanForm() {
  state.activePlanId = null;
  localStorage.removeItem("flexorama-active-plan");
  document.getElementById("plan-title").value = "";
  document.getElementById("plan-user-request").value = "";
  document.getElementById("plan-markdown").value = "";
}

function resetMcpForm() {
  state.activeServer = null;
  localStorage.removeItem("flexorama-active-mcp");
  document.getElementById("mcp-name").value = "";
  document.getElementById("mcp-command").value = "";
  document.getElementById("mcp-args").value = "";
  document.getElementById("mcp-url").value = "";
  document.getElementById("mcp-env").value = "";
  document.getElementById("mcp-enabled").value = "true";
  renderMcpList();
  document.getElementById("connect-mcp-detail").style.display = "none";
  document.getElementById("disconnect-mcp-detail").style.display = "none";
  document.getElementById("delete-mcp-detail").style.display = "none";
}

function setMcpForm(server) {
  if (!server) {
    resetMcpForm();
    return;
  }
  state.activeServer = server.name;
  localStorage.setItem("flexorama-active-mcp", server.name);
  document.getElementById("mcp-name").value = server.name;
  document.getElementById("mcp-command").value = server.config.command || "";
  document.getElementById("mcp-args").value = (server.config.args || []).join(" ");
  document.getElementById("mcp-url").value = server.config.url || "";
  const env = server.config.env || {};
  document.getElementById("mcp-env").value = Object.entries(env)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
  document.getElementById("mcp-enabled").value = String(server.config.enabled);
  document.getElementById("connect-mcp-detail").style.display = "";
  document.getElementById("disconnect-mcp-detail").style.display = "";
  document.getElementById("delete-mcp-detail").style.display = "";
}
async function savePlan() {
  if (!state.activePlanId) return;
  const payload = {
    title: document.getElementById("plan-title").value,
    user_request: document.getElementById("plan-user-request").value,
    plan_markdown: document.getElementById("plan-markdown").value,
  };
  await api(`/api/plans/${state.activePlanId}`, { method: "PUT", body: payload });
  await loadPlans();
}

async function createPlan() {
  resetPlanForm();
  const payload = {
    title: "",
    user_request: "",
    plan_markdown: "",
    conversation_id: state.activeConversationId,
  };
  const res = await api("/api/plans", { method: "POST", body: payload });
  state.activePlanId = res.id;
  localStorage.setItem("flexorama-active-plan", String(res.id));
  await loadPlans();
}

async function deletePlan() {
  if (!state.activePlanId) return;
  await api(`/api/plans/${state.activePlanId}`, { method: "DELETE" });
  resetPlanForm();
  await loadPlans();
  selectFirstPlan();
}

async function executePlanMessageInCurrentChat(message) {
  const input = document.getElementById("message-input");
  if (input) {
    input.value = message;
    await sendMessage();
  }
}

async function executePlanMessageInNewChat(message) {
  setStatus("Creating conversation for plan execution...");
  const res = await api("/api/conversations", { method: "POST", body: {} });
  const newId = res.id;

  const placeholder = {
    id: newId,
    last_message: null,
    updated_at: new Date().toISOString(),
    model: res.model || state.activeModel || state.conversations[0]?.model || "unknown",
    created_at: new Date().toISOString(),
  };
  state.activeConversationId = newId;
  localStorage.setItem("flexorama-active-conversation", String(newId));
  mergeConversations([placeholder]);
  renderConversationList();

  const tabBtn = document.querySelector('.top-tab[data-tab="chats"]');
  if (tabBtn) tabBtn.click();

  await selectConversation(newId);
  await executePlanMessageInCurrentChat(message);
}

async function executeSavedPlan(planId, { newChat }) {
  if (!planId) return;
  setStatus("Loading plan for execution...");
  const plan = await api(`/api/plans/${planId}`);
  const planMarkdown = plan.plan_markdown || "";
  const planTitle = plan.title || "Saved plan";
  const message = `Execute the following saved plan (id: ${planId} - title: ${planTitle}):\n\n${planMarkdown}`;
  if (state.planMode) {
    await setPlanMode(false);
  }
  if (newChat) {
    await executePlanMessageInNewChat(message);
  } else {
    await executePlanMessageInCurrentChat(message);
  }
}

async function executePlan() {
  if (!state.activePlanId) return;

  const planMarkdown = document.getElementById("plan-markdown").value;
  const planTitle = document.getElementById("plan-title").value || "Untitled plan";

  const message = `Execute the following plan (${planTitle}):\n\n${planMarkdown}`;
  if (state.planMode) {
    await setPlanMode(false);
  }
  await executePlanMessageInNewChat(message);
}

// MCP
async function loadMcp() {
  state.mcpServers = await api("/api/mcp/servers");
  renderMcpList();
}

function renderMcpList() {
  const list = document.getElementById("mcp-list");
  list.innerHTML = "";
  state.mcpServers.forEach((server) => {
    const item = document.createElement("div");
    item.className = "list-item" + (server.name === state.activeServer ? " active" : "");
    const status = server.connected ? "Connected" : server.config.enabled ? "Ready" : "Disabled";
    item.innerHTML = `
      <div class="flex-between">
        <div>
          <div style="font-weight:700;">${server.name}</div>
          <small class="muted">${status}</small>
        </div>
      </div>
      <div class="muted" style="margin-top:6px;">${server.config.url || server.config.command || "No endpoint configured"}</div>
    `;
    item.addEventListener("click", () => {
      setMcpForm(server);
      renderMcpList();
    });
    list.appendChild(item);
  });
}

async function saveMcpServer() {
  const name = document.getElementById("mcp-name").value.trim();
  if (!name) return;
  const payload = {
    name,
    command: document.getElementById("mcp-command").value.trim() || null,
    args: document
      .getElementById("mcp-args")
      .value.trim()
      .split(" ")
      .filter(Boolean),
    url: document.getElementById("mcp-url").value.trim() || null,
    env: parseEnv(document.getElementById("mcp-env").value),
    enabled: document.getElementById("mcp-enabled").value === "true",
  };
  await api(`/api/mcp/servers/${name}`, { method: "PUT", body: payload });
  await loadMcp();
}

function parseEnv(text) {
  const env = {};
  text
    .replace(/\r?\n/g, ",")
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean)
    .forEach((pair) => {
      const idx = pair.indexOf("=");
      if (idx <= 0) return;
      const key = pair.slice(0, idx).trim();
      const value = pair.slice(idx + 1).trim();
      if (key) env[key] = value;
    });
  return env;
}

// Agents
async function loadAgents() {
  state.agents = await api("/api/agents");
  const active = await api("/api/agents/active");
  state.activeAgent = active.active;
  renderAgents();
  renderAgentSelector();
  await loadModels();
}

function renderAgents() {
  const list = document.getElementById("agent-list");
  list.innerHTML = "";
  state.agents.forEach((agent) => {
    const item = document.createElement("div");
    item.className = "list-item" + (agent.name === state.activeAgentEditing ? " active" : "");
    item.innerHTML = `
      <div style="font-weight:700;">${agent.name}</div>
      <small class="muted">${agent.model || "model inherits"} • ${agent.allowed_tools.length} allowed</small>
    `;
    item.addEventListener("click", () => {
      setAgentForm(agent);
      renderAgents();
    });
    list.appendChild(item);
  });
}

function resetAgentForm() {
  state.activeAgentEditing = null;
  localStorage.removeItem("flexorama-active-agent-edit");
  document.getElementById("agent-name").value = "";
  document.getElementById("agent-model").value = "";
  document.getElementById("agent-temp").value = "";
  document.getElementById("agent-max-tokens").value = "";
  document.getElementById("agent-allowed").value = READONLY_TOOLS.join(", ");
  document.getElementById("agent-denied").value = "";
  document.getElementById("agent-prompt").value = "";
  const deleteBtn = document.getElementById("delete-agent");
  if (deleteBtn) deleteBtn.style.display = "none";
  renderAgents();
}

function setAgentForm(agent) {
  state.activeAgentEditing = agent.name;
  localStorage.setItem("flexorama-active-agent-edit", agent.name);
  document.getElementById("agent-name").value = agent.name;
  document.getElementById("agent-model").value = agent.model || "";
  document.getElementById("agent-temp").value = agent.temperature ?? "";
  document.getElementById("agent-max-tokens").value = agent.max_tokens ?? "";
  document.getElementById("agent-allowed").value = agent.allowed_tools.join(", ");
  document.getElementById("agent-denied").value = agent.denied_tools.join(", ");
  document.getElementById("agent-prompt").value = agent.system_prompt;
  const deleteBtn = document.getElementById("delete-agent");
  if (deleteBtn) deleteBtn.style.display = "";
}

function selectFirstConversation() {
  if (state.conversations.length === 0) return;
  const saved = state.conversations.find((c) => String(c.id) === String(state.activeConversationId));
  const target = saved ? saved.id : state.conversations[0].id;
  selectConversation(target);
}

function selectFirstPlan() {
  if (state.plans.length === 0) {
    resetPlanForm();
    return;
  }
  const saved = state.plans.find((p) => String(p.id) === String(state.activePlanId));
  const target = saved || state.plans[0];
  setPlanForm(target);
}

function selectFirstMcp() {
  if (state.mcpServers.length === 0) {
    resetMcpForm();
    return;
  }
  const saved = state.mcpServers.find((s) => s.name === state.activeServer);
  const target = saved || state.mcpServers[0];
  setMcpForm(target);
  renderMcpList();
}

function selectFirstAgent() {
  if (state.agents.length === 0) {
    resetAgentForm();
    return;
  }
  const saved = state.agents.find((a) => a.name === state.activeAgentEditing);
  const target = saved || state.agents[0];
  setAgentForm(target);
  renderAgents();
}

async function saveAgent() {
  const payload = {
    system_prompt: document.getElementById("agent-prompt").value,
    allowed_tools: splitList(document.getElementById("agent-allowed").value),
    denied_tools: splitList(document.getElementById("agent-denied").value),
    max_tokens: numberOrNull(document.getElementById("agent-max-tokens").value),
    temperature: numberOrNull(document.getElementById("agent-temp").value),
    model: document.getElementById("agent-model").value || null,
  };
  const name = document.getElementById("agent-name").value.trim();
  if (!name) return;

  if (state.agents.some((a) => a.name === name)) {
    await api(`/api/agents/${name}`, { method: "PUT", body: payload });
  } else {
    await api("/api/agents", { method: "POST", body: { ...payload, name } });
  }
  state.activeAgentEditing = name;
  await loadAgents();
}

async function activateAgent(name) {
  const res = await api("/api/agents/active", { method: "POST", body: { name } });
  await loadAgents();
  if (res.conversation_id) {
    state.activeConversationId = res.conversation_id;
    await selectConversation(res.conversation_id);
  }
}

async function deleteAgent() {
  const name = document.getElementById("agent-name").value.trim();
  if (!name) return;
  const idx = state.agents.findIndex((a) => a.name === name);
  await api(`/api/agents/${name}`, { method: "DELETE" });
  state.activeAgentEditing = null;
  await loadAgents();
  if (state.agents.length > 0) {
    const next = state.agents[Math.min(Math.max(idx, 0), state.agents.length - 1)];
    setAgentForm(next);
    renderAgents();
  } else {
    resetAgentForm();
  }
}

function splitList(text) {
  return text
    .split(",")
    .map((v) => v.trim())
    .filter(Boolean);
}

function numberOrNull(val) {
  const num = parseFloat(val);
  return isNaN(num) ? null : num;
}

function renderAgentSelector() {
  const select = document.getElementById("agent-selector");
  if (!select) return;
  select.innerHTML = "";
  const optDefault = document.createElement("option");
  optDefault.value = "";
  optDefault.textContent = "Default agent";
  select.appendChild(optDefault);
  state.agents.forEach((agent) => {
    const opt = document.createElement("option");
    opt.value = agent.name;
    opt.textContent = agent.name;
    select.appendChild(opt);
  });
  select.value = state.activeAgent || "";
}

// Skills
async function loadSkills() {
  state.skills = await api("/api/skills");
  renderSkills();
}

function renderSkills() {
  const list = document.getElementById("skill-list");
  list.innerHTML = "";
  state.skills.forEach((skill) => {
    const item = document.createElement("div");
    const activeIndicator = skill.active ? "🟢 " : "";
    item.className = "list-item" + (skill.name === state.activeSkillEditing ? " active" : "");
    item.innerHTML = `
      <div style="font-weight:700;">${activeIndicator}${skill.name}</div>
      <small class="muted">${skill.description || "No description"}</small>
    `;
    item.addEventListener("click", () => {
      setSkillForm(skill);
      renderSkills();
    });
    list.appendChild(item);
  });
}

function resetSkillForm() {
  state.activeSkillEditing = null;
  localStorage.removeItem("flexorama-active-skill-edit");
  document.getElementById("skill-name").value = "";
  document.getElementById("skill-description").value = "";
  document.getElementById("skill-model").value = "";
  document.getElementById("skill-temp").value = "";
  document.getElementById("skill-max-tokens").value = "";
  document.getElementById("skill-allowed").value = "";
  document.getElementById("skill-denied").value = "";
  document.getElementById("skill-tags").value = "";
  document.getElementById("skill-content").value = "";
  document.getElementById("skill-active").checked = false;
  const deleteBtn = document.getElementById("delete-skill");
  const activateBtn = document.getElementById("toggle-skill-activation");
  if (deleteBtn) deleteBtn.style.display = "none";
  if (activateBtn) activateBtn.style.display = "none";
  renderSkills();
}

function setSkillForm(skill) {
  state.activeSkillEditing = skill.name;
  localStorage.setItem("flexorama-active-skill-edit", skill.name);
  document.getElementById("skill-name").value = skill.name;
  document.getElementById("skill-description").value = skill.description || "";
  document.getElementById("skill-model").value = skill.model || "";
  document.getElementById("skill-temp").value = skill.temperature ?? "";
  document.getElementById("skill-max-tokens").value = skill.max_tokens ?? "";
  document.getElementById("skill-allowed").value = skill.allowed_tools.join(", ");
  document.getElementById("skill-denied").value = skill.denied_tools.join(", ");
  document.getElementById("skill-tags").value = skill.tags.join(", ");
  document.getElementById("skill-content").value = skill.content || "";
  document.getElementById("skill-active").checked = skill.active;

  const deleteBtn = document.getElementById("delete-skill");
  const activateBtn = document.getElementById("toggle-skill-activation");
  if (deleteBtn) deleteBtn.style.display = "inline-block";
  if (activateBtn) {
    activateBtn.style.display = "inline-block";
    activateBtn.textContent = skill.active ? "Deactivate" : "Activate";
  }
  renderSkills();
}

function selectFirstSkill() {
  if (state.skills.length === 0) {
    resetSkillForm();
    return;
  }
  const saved = state.skills.find((s) => s.name === state.activeSkillEditing);
  const target = saved || state.skills[0];
  setSkillForm(target);
  renderSkills();
}

async function saveSkill() {
  const name = document.getElementById("skill-name").value.trim();
  if (!name) return;

  const payload = {
    description: document.getElementById("skill-description").value.trim(),
    content: document.getElementById("skill-content").value.trim(),
    allowed_tools: splitList(document.getElementById("skill-allowed").value),
    denied_tools: splitList(document.getElementById("skill-denied").value),
    tags: splitList(document.getElementById("skill-tags").value),
    max_tokens: numberOrNull(document.getElementById("skill-max-tokens").value),
    temperature: numberOrNull(document.getElementById("skill-temp").value),
    model: document.getElementById("skill-model").value || null,
  };

  if (state.skills.some((s) => s.name === name)) {
    await api(`/api/skills/${name}`, { method: "PUT", body: payload });
  } else {
    await api("/api/skills", { method: "POST", body: { ...payload, name } });
  }
  state.activeSkillEditing = name;
  await loadSkills();

  // Restore form to show updated skill
  const skill = state.skills.find((s) => s.name === name);
  if (skill) {
    setSkillForm(skill);
  }
}

async function toggleSkillActivation() {
  const name = document.getElementById("skill-name").value.trim();
  if (!name) return;

  const skill = state.skills.find((s) => s.name === name);
  if (!skill) return;

  if (skill.active) {
    await api(`/api/skills/${name}/deactivate`, { method: "POST" });
  } else {
    await api(`/api/skills/${name}/activate`, { method: "POST" });
  }

  await loadSkills();

  // Restore form to show updated skill
  const updatedSkill = state.skills.find((s) => s.name === name);
  if (updatedSkill) {
    setSkillForm(updatedSkill);
  }
}

async function deleteSkill() {
  const name = document.getElementById("skill-name").value.trim();
  if (!name) return;
  const idx = state.skills.findIndex((s) => s.name === name);
  await api(`/api/skills/${name}`, { method: "DELETE" });
  state.activeSkillEditing = null;
  await loadSkills();
  if (state.skills.length > 0) {
    const next = state.skills[Math.min(Math.max(idx, 0), state.skills.length - 1)];
    setSkillForm(next);
    renderSkills();
  } else {
    resetSkillForm();
  }
}

// Commands
async function loadCommands() {
  state.commands = await api("/api/commands");
  renderCommands();
}

function renderCommands() {
  const list = document.getElementById("command-list");
  if (!list) return;
  list.innerHTML = "";
  state.commands.forEach((command) => {
    const item = document.createElement("div");
    item.className =
      "list-item" + (command.name === state.activeCommandEditing ? " active" : "");
    const hint = command.argument_hint ? ` ${command.argument_hint}` : "";
    item.innerHTML = `
      <div style="font-weight:700;">/${command.name}${hint}</div>
      <small class="muted">${command.description || "No description"}</small>
    `;
    item.addEventListener("click", () => {
      setCommandForm(command);
      renderCommands();
    });
    list.appendChild(item);
  });
}

function resetCommandForm() {
  state.activeCommandEditing = null;
  localStorage.removeItem("flexorama-active-command-edit");
  document.getElementById("command-name").value = "";
  document.getElementById("command-description").value = "";
  document.getElementById("command-argument-hint").value = "";
  document.getElementById("command-model").value = "";
  document.getElementById("command-allowed-tools").value = "";
  document.getElementById("command-content").value = "";
  const deleteBtn = document.getElementById("delete-command");
  if (deleteBtn) deleteBtn.style.display = "none";
  renderCommands();
}

function setCommandForm(command) {
  state.activeCommandEditing = command.name;
  localStorage.setItem("flexorama-active-command-edit", command.name);
  document.getElementById("command-name").value = command.name;
  document.getElementById("command-description").value = command.description || "";
  document.getElementById("command-argument-hint").value = command.argument_hint || "";
  document.getElementById("command-model").value = command.model || "";
  document.getElementById("command-allowed-tools").value =
    (command.allowed_tools || []).join(", ");
  document.getElementById("command-content").value = command.content || "";
  const deleteBtn = document.getElementById("delete-command");
  if (deleteBtn) deleteBtn.style.display = "inline-block";
  renderCommands();
}

function selectFirstCommand() {
  if (state.commands.length === 0) {
    resetCommandForm();
    return;
  }
  const saved = state.commands.find((c) => c.name === state.activeCommandEditing);
  const target = saved || state.commands[0];
  setCommandForm(target);
  renderCommands();
}

async function saveCommand() {
  const rawName = document.getElementById("command-name").value.trim();
  const name = rawName.startsWith("/") ? rawName.slice(1) : rawName;
  if (!name) return;

  const payload = {
    description: document.getElementById("command-description").value.trim(),
    argument_hint: document.getElementById("command-argument-hint").value.trim() || null,
    allowed_tools: splitList(document.getElementById("command-allowed-tools").value),
    model: document.getElementById("command-model").value.trim() || null,
    content: document.getElementById("command-content").value,
  };

  if (state.commands.some((c) => c.name === name)) {
    await api(`/api/commands/${name}`, { method: "PUT", body: payload });
  } else {
    await api("/api/commands", { method: "POST", body: { ...payload, name } });
  }
  state.activeCommandEditing = name;
  await loadCommands();

  const command = state.commands.find((c) => c.name === name);
  if (command) {
    setCommandForm(command);
  }
}

async function deleteCommand() {
  const rawName = document.getElementById("command-name").value.trim();
  const name = rawName.startsWith("/") ? rawName.slice(1) : rawName;
  if (!name) return;
  const idx = state.commands.findIndex((c) => c.name === name);
  await api(`/api/commands/${name}`, { method: "DELETE" });
  state.activeCommandEditing = null;
  await loadCommands();
  if (state.commands.length > 0) {
    const next = state.commands[Math.min(Math.max(idx, 0), state.commands.length - 1)];
    setCommandForm(next);
    renderCommands();
  } else {
    resetCommandForm();
  }
}

async function loadModels() {
  const data = await api("/api/models");
  state.provider = data.provider;
  state.models = Array.isArray(data.models) ? data.models : [];
  state.activeModel = data.active_model;
  renderModelSelector();
}

// Plan Mode
async function loadPlanMode() {
  try {
    const data = await api("/api/plan-mode");
    state.planMode = data.enabled;
    renderPlanModeButton();
  } catch (err) {
    console.error("Failed to load plan mode:", err);
  }
}

async function setPlanMode(enabled) {
  if (state.planMode === enabled) return;
  try {
    await api("/api/plan-mode", {
      method: "POST",
      body: { enabled },
    });
    state.planMode = enabled;
    renderPlanModeButton();
    setStatus(enabled ? "Plan mode enabled" : "Plan mode disabled");
  } catch (err) {
    setStatus(`Failed to toggle plan mode: ${err.message}`);
  }
}

async function togglePlanMode() {
  await setPlanMode(!state.planMode);
}

function renderPlanModeButton() {
  const btn = document.getElementById("plan-mode-toggle");
  if (!btn) return;
  if (state.planMode) {
    btn.classList.add("plan-mode-active");
    btn.textContent = "📋 Plan: ON";
  } else {
    btn.classList.remove("plan-mode-active");
    btn.textContent = "📋 Plan: OFF";
  }
}

function renderModelSelector() {
  const select = document.getElementById("model-selector");
  if (!select) return;
  select.innerHTML = "";
  const current = state.activeModel;
  if (current && !state.models.includes(current)) {
    const optCurrent = document.createElement("option");
    optCurrent.value = current;
    optCurrent.textContent = `${current} (current)`;
    select.appendChild(optCurrent);
  }
  state.models.forEach((model) => {
    const opt = document.createElement("option");
    opt.value = model;
    opt.textContent = model;
    select.appendChild(opt);
  });
  if (current) {
    select.value = current;
  } else if (state.models.length) {
    select.value = state.models[0];
  }
}

async function restoreSelections() {
  const savedConv = localStorage.getItem("flexorama-active-conversation");
  if (savedConv && state.conversations.some((c) => String(c.id) === String(savedConv))) {
    await selectConversation(savedConv);
  }

  const savedPlan = localStorage.getItem("flexorama-active-plan");
  const planMatch = savedPlan && state.plans.find((p) => String(p.id) === String(savedPlan));
  if (planMatch) {
    setPlanForm(planMatch);
  }

  const savedMcp = localStorage.getItem("flexorama-active-mcp");
  const mcpMatch = savedMcp && state.mcpServers.find((s) => s.name === savedMcp);
  if (mcpMatch) {
    setMcpForm(mcpMatch);
  }

  const savedAgentEdit = localStorage.getItem("flexorama-active-agent-edit");
  const agentMatch = savedAgentEdit && state.agents.find((a) => a.name === savedAgentEdit);
  if (agentMatch) {
    setAgentForm(agentMatch);
  }

  const savedSkillEdit = localStorage.getItem("flexorama-active-skill-edit");
  const skillMatch = savedSkillEdit && state.skills.find((s) => s.name === savedSkillEdit);
  if (skillMatch) {
    setSkillForm(skillMatch);
  }

  const savedCommandEdit = localStorage.getItem("flexorama-active-command-edit");
  const commandMatch =
    savedCommandEdit && state.commands.find((c) => c.name === savedCommandEdit);
  if (commandMatch) {
    setCommandForm(commandMatch);
  }
}

// Tabs
function initTabs() {
  document.querySelectorAll(".top-tab").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const target = btn.dataset.tab;
      state.activeTab = target;
      const url = new URL(window.location);
      url.searchParams.set("tab", target);
      window.history.replaceState({}, "", url);
      document.querySelectorAll(".top-tab").forEach((b) => b.classList.remove("active"));
      document.querySelectorAll(".tab-content").forEach((tab) => tab.classList.remove("active"));
      btn.classList.add("active");
      document.getElementById(`tab-${target}`).classList.add("active");

      switch (target) {
        case "plans":
          await loadPlans();
          selectFirstPlan();
          break;
        case "mcp":
          await loadMcp();
          selectFirstMcp();
          break;
        case "agents":
          await loadAgents();
          selectFirstAgent();
          break;
        case "skills":
          await loadSkills();
          selectFirstSkill();
          break;
        case "commands":
          await loadCommands();
          selectFirstCommand();
          break;
        case "stats":
          loadStats();
          break;
        default:
          await loadConversations();
          selectFirstConversation();
          break;
      }
    });
  });
}

function initTheme() {
  const stored = localStorage.getItem("flexorama-theme");
  const initial = stored === "light" || stored === "dark" ? stored : "dark";
  applyTheme(initial);
  const toggle = document.getElementById("mode-toggle");
  if (toggle) {
    toggle.addEventListener("click", () => {
      applyTheme(state.theme === "light" ? "dark" : "light");
    });
  }
}

// File autocomplete functionality
let autocompleteState = {
  isVisible: false,
  selectedIndex: -1,
  files: [],
  atPosition: -1,
  prefix: "",
};

async function fetchFileAutocomplete(prefix) {
  try {
    const response = await fetch(`/api/file-autocomplete?prefix=${encodeURIComponent(prefix)}`);
    if (!response.ok) return [];
    const data = await response.json();
    return data.files || [];
  } catch (err) {
    console.error("File autocomplete error:", err);
    return [];
  }
}

function showAutocomplete(files, atPos, prefix) {
  const dropdown = document.getElementById("file-autocomplete-dropdown");
  if (!dropdown) return;

  autocompleteState.isVisible = true;
  autocompleteState.files = files;
  autocompleteState.selectedIndex = 0;
  autocompleteState.atPosition = atPos;
  autocompleteState.prefix = prefix;

  dropdown.innerHTML = "";

  if (files.length === 0) {
    dropdown.classList.remove("visible");
    autocompleteState.isVisible = false;
    return;
  }

  files.forEach((file, index) => {
    const item = document.createElement("div");
    item.className = "autocomplete-item" + (file.is_directory ? " directory" : "");
    if (index === 0) item.classList.add("selected");

    const icon = document.createElement("span");
    icon.className = "file-icon";
    icon.textContent = file.is_directory ? "📁" : "📄";

    const path = document.createElement("span");
    path.className = "file-path";
    path.textContent = file.path;

    item.appendChild(icon);
    item.appendChild(path);

    item.addEventListener("click", () => {
      selectAutocompleteItem(index);
    });

    item.addEventListener("mouseenter", () => {
      updateAutocompleteSelection(index);
    });

    dropdown.appendChild(item);
  });

  dropdown.classList.add("visible");
}

function hideAutocomplete() {
  const dropdown = document.getElementById("file-autocomplete-dropdown");
  if (dropdown) {
    dropdown.classList.remove("visible");
  }
  autocompleteState.isVisible = false;
  autocompleteState.selectedIndex = -1;
  autocompleteState.files = [];
  autocompleteState.atPosition = -1;
  autocompleteState.prefix = "";
}

function updateAutocompleteSelection(newIndex) {
  if (newIndex < 0 || newIndex >= autocompleteState.files.length) return;

  const dropdown = document.getElementById("file-autocomplete-dropdown");
  if (!dropdown) return;

  const items = dropdown.querySelectorAll(".autocomplete-item");
  items.forEach((item, i) => {
    if (i === newIndex) {
      item.classList.add("selected");
      item.scrollIntoView({ block: "nearest" });
    } else {
      item.classList.remove("selected");
    }
  });

  autocompleteState.selectedIndex = newIndex;
}

function selectAutocompleteItem(index) {
  if (index < 0 || index >= autocompleteState.files.length) return;

  const file = autocompleteState.files[index];
  const input = document.getElementById("message-input");
  if (!input) return;

  const text = input.value;
  const beforeAt = text.substring(0, autocompleteState.atPosition);
  const afterPrefix = text.substring(autocompleteState.atPosition + 1 + autocompleteState.prefix.length);

  // Insert the file path
  input.value = beforeAt + "@" + file.path + (file.is_directory ? "/" : " ") + afterPrefix;

  // Set cursor position
  const cursorPos = beforeAt.length + 1 + file.path.length + (file.is_directory ? 1 : 1);
  input.setSelectionRange(cursorPos, cursorPos);

  hideAutocomplete();
  input.focus();
}

async function handleAutocompleteInput() {
  const input = document.getElementById("message-input");
  if (!input) return;

  const text = input.value;
  const cursorPos = input.selectionStart;

  // Find the last @ before cursor
  let atPos = -1;
  for (let i = cursorPos - 1; i >= 0; i--) {
    if (text[i] === "@") {
      atPos = i;
      break;
    }
    // Stop if we hit whitespace before finding @
    if (text[i] === " " || text[i] === "\n") {
      break;
    }
  }

  if (atPos === -1) {
    hideAutocomplete();
    return;
  }

  // Extract the prefix after @
  const prefix = text.substring(atPos + 1, cursorPos);

  // Only show autocomplete if @ is at start or preceded by whitespace
  if (atPos > 0 && text[atPos - 1] !== " " && text[atPos - 1] !== "\n") {
    hideAutocomplete();
    return;
  }

  // Fetch and show autocomplete
  const files = await fetchFileAutocomplete(prefix);
  showAutocomplete(files, atPos, prefix);
}

// Image handling functions
async function handleImagePaste(file) {
  const reader = new FileReader();
  reader.onload = (e) => {
    const base64Data = e.target.result.split(",")[1]; // Remove data:image/...;base64, prefix
    const mediaType = file.type;

    state.pendingImages.push({
      media_type: mediaType,
      data: base64Data,
      preview: e.target.result, // Keep full data URL for preview
    });

    renderImageThumbnails();
  };
  reader.readAsDataURL(file);
}

function renderImageThumbnails() {
  const container = document.getElementById("image-thumbnails");
  if (!container) return;

  // Clear and rebuild thumbnails
  container.innerHTML = "";

  if (state.pendingImages.length === 0) {
    container.classList.remove("visible");
    return;
  }

  container.classList.add("visible");

  state.pendingImages.forEach((img, index) => {
    const thumbWrapper = document.createElement("div");
    thumbWrapper.className = "image-thumb-wrapper";

    const thumb = document.createElement("img");
    thumb.src = img.preview;
    thumb.className = "image-thumb";

    const removeBtn = document.createElement("button");
    removeBtn.innerHTML = "×";
    removeBtn.className = "image-thumb-remove";
    removeBtn.onclick = () => {
      state.pendingImages.splice(index, 1);
      renderImageThumbnails();
    };

    thumbWrapper.appendChild(thumb);
    thumbWrapper.appendChild(removeBtn);
    container.appendChild(thumbWrapper);
  });
}

function clearPendingImages() {
  state.pendingImages = [];
  renderImageThumbnails();
}

function bindEvents() {
  document.getElementById("send-message").addEventListener("click", sendMessage);

  const messageInput = document.getElementById("message-input");

  // Handle keyboard events
  messageInput.addEventListener("keydown", (e) => {
    // Handle autocomplete navigation
    if (autocompleteState.isVisible) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        const newIndex = (autocompleteState.selectedIndex + 1) % autocompleteState.files.length;
        updateAutocompleteSelection(newIndex);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        const newIndex = (autocompleteState.selectedIndex - 1 + autocompleteState.files.length) % autocompleteState.files.length;
        updateAutocompleteSelection(newIndex);
        return;
      }
      if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
        e.preventDefault();
        selectAutocompleteItem(autocompleteState.selectedIndex);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        hideAutocomplete();
        return;
      }
    }

    // Send message on Enter (without Shift)
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  });

  // Handle input changes for autocomplete
  messageInput.addEventListener("input", () => {
    handleAutocompleteInput();
  });

  // Handle image paste
  messageInput.addEventListener("paste", (e) => {
    const items = e.clipboardData?.items;
    if (!items) return;

    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      if (item.type.startsWith("image/")) {
        e.preventDefault();
        const file = item.getAsFile();
        if (file) {
          handleImagePaste(file);
        }
      }
    }
  });

  // Hide autocomplete when clicking outside
  document.addEventListener("click", (e) => {
    if (!e.target.closest(".composer")) {
      hideAutocomplete();
    }
  });
  const conversationSearch = document.getElementById("conversation-search");
  if (conversationSearch) {
    conversationSearch.addEventListener("input", (e) => {
      state.conversationSearch = e.target.value;
      if (conversationSearchTimer) clearTimeout(conversationSearchTimer);
      const term = state.conversationSearch;
      if (!term.trim()) {
        if (conversationSearchController) conversationSearchController.abort();
        state.conversationSearchResults = null;
        state.conversationSearchLastSent = "";
        setConversationSearchLoading(false);
        renderConversationList();
        return;
      }
      setConversationSearchLoading(true);
      state.conversationSearchResults = null;
      renderConversationList();
      conversationSearchTimer = setTimeout(() => {
        performConversationSearch(term);
      }, 350);
    });
    conversationSearch.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        conversationSearch.value = "";
        state.conversationSearch = "";
        if (conversationSearchTimer) clearTimeout(conversationSearchTimer);
        if (conversationSearchController) conversationSearchController.abort();
        state.conversationSearchResults = null;
        state.conversationSearchLastSent = "";
        setConversationSearchLoading(false);
        renderConversationList();
      }
    });
    const searchClear = document.getElementById("conversation-search-clear");
    if (searchClear) {
      searchClear.addEventListener("click", () => {
        conversationSearch.value = "";
        state.conversationSearch = "";
        if (conversationSearchTimer) clearTimeout(conversationSearchTimer);
        if (conversationSearchController) conversationSearchController.abort();
        state.conversationSearchResults = null;
        state.conversationSearchLastSent = "";
        setConversationSearchLoading(false);
        renderConversationList();
        conversationSearch.focus();
      });
    }
  }
  document.getElementById("new-conversation").addEventListener("click", createConversation);

  // Add scroll detection for lazy loading conversations
  const conversationList = document.getElementById("conversation-list");
  if (conversationList) {
    conversationList.addEventListener("scroll", () => {
      // Check if user has scrolled near the bottom
      const scrollHeight = conversationList.scrollHeight;
      const scrollTop = conversationList.scrollTop;
      const clientHeight = conversationList.clientHeight;
      const scrolledToBottom = scrollHeight - scrollTop - clientHeight < 100;

      if (scrolledToBottom && !getConversationSearchTerm()) {
        loadMoreConversations();
      }
    });
  }

  const todoToggle = document.getElementById("todo-toggle");
  if (todoToggle) {
    todoToggle.addEventListener("click", () => {
      state.todosCollapsed = !state.todosCollapsed;
      localStorage.setItem("flexorama-todos-collapsed", String(state.todosCollapsed));
      renderTodoPane();
    });
  }

  document.getElementById("save-plan").addEventListener("click", savePlan);
  const createPlanBtn = document.getElementById("create-plan");
  if (createPlanBtn) createPlanBtn.addEventListener("click", createPlan);
  document.getElementById("create-plan-sidebar").addEventListener("click", createPlan);
  document.getElementById("delete-plan").addEventListener("click", deletePlan);
  document.getElementById("execute-plan").addEventListener("click", executePlan);

  document.getElementById("new-mcp").addEventListener("click", resetMcpForm);
  document.getElementById("save-mcp-detail").addEventListener("click", saveMcpServer);
  document.getElementById("connect-mcp-detail").addEventListener("click", async () => {
    const name = document.getElementById("mcp-name").value.trim();
    if (!name) return;
    await api(`/api/mcp/servers/${name}/connect`, { method: "POST" });
    await loadMcp();
  });
  document.getElementById("disconnect-mcp-detail").addEventListener("click", async () => {
    const name = document.getElementById("mcp-name").value.trim();
    if (!name) return;
    await api(`/api/mcp/servers/${name}/disconnect`, { method: "POST" });
    await loadMcp();
  });
  const deleteMcpBtn = document.getElementById("delete-mcp-detail");
  if (deleteMcpBtn) {
    deleteMcpBtn.addEventListener("click", async () => {
      const name = document.getElementById("mcp-name").value.trim();
      if (!name) return;
      const idx = state.mcpServers.findIndex((s) => s.name === name);
      await api(`/api/mcp/servers/${name}`, { method: "DELETE" });
      await loadMcp();
      if (state.mcpServers.length > 0) {
        const next = state.mcpServers[Math.min(Math.max(idx, 0), state.mcpServers.length - 1)];
        setMcpForm(next);
        renderMcpList();
      } else {
        resetMcpForm();
      }
    });
  }

  document.getElementById("save-agent").addEventListener("click", saveAgent);
  const deleteAgentBtn = document.getElementById("delete-agent");
  if (deleteAgentBtn) deleteAgentBtn.addEventListener("click", deleteAgent);
  document.getElementById("new-agent").addEventListener("click", resetAgentForm);
  document.getElementById("agent-selector").addEventListener("change", (e) => {
    const name = e.target.value || null;
    activateAgent(name);
  });

  document.getElementById("save-skill").addEventListener("click", saveSkill);
  const deleteSkillBtn = document.getElementById("delete-skill");
  if (deleteSkillBtn) deleteSkillBtn.addEventListener("click", deleteSkill);
  const toggleSkillBtn = document.getElementById("toggle-skill-activation");
  if (toggleSkillBtn) toggleSkillBtn.addEventListener("click", toggleSkillActivation);
  const skillActiveCheckbox = document.getElementById("skill-active");
  if (skillActiveCheckbox) skillActiveCheckbox.addEventListener("change", toggleSkillActivation);
  document.getElementById("new-skill").addEventListener("click", resetSkillForm);

  const saveCommandBtn = document.getElementById("save-command");
  if (saveCommandBtn) saveCommandBtn.addEventListener("click", saveCommand);
  const deleteCommandBtn = document.getElementById("delete-command");
  if (deleteCommandBtn) deleteCommandBtn.addEventListener("click", deleteCommand);
  const newCommandBtn = document.getElementById("new-command");
  if (newCommandBtn) newCommandBtn.addEventListener("click", resetCommandForm);

  document.getElementById("model-selector").addEventListener("change", async (e) => {
    const model = e.target.value;
    if (!model) return;
    try {
      await api("/api/models", { method: "POST", body: { model } });
      state.activeModel = model;
      await loadConversations();
      if (state.activeConversationId) {
        await selectConversation(state.activeConversationId);
      }
      setStatus(`Model updated to ${model}`);
    } catch (err) {
      setStatus(`Model update failed: ${err.message}`);
    }
  });
  document.getElementById("show-context").addEventListener("click", showContextModal);
  document.getElementById("close-context").addEventListener("click", closeContextModal);
  document.getElementById("context-modal").addEventListener("click", (e) => {
    if (e.target.id === "context-modal") closeContextModal();
  });
  const planModeBtn = document.getElementById("plan-mode-toggle");
  if (planModeBtn) {
    planModeBtn.addEventListener("click", togglePlanMode);
  }

  // Stats event handlers
  const statsPeriodSelect = document.getElementById("stats-period");
  const statsStartDate = document.getElementById("stats-start-date");
  const statsEndDate = document.getElementById("stats-end-date");

  if (statsPeriodSelect) {
    statsPeriodSelect.addEventListener("change", (e) => {
      const previousPeriod = state.statsPeriod;
      state.statsPeriod = e.target.value;
      localStorage.setItem("flexorama-stats-period", state.statsPeriod);

      if (state.statsPeriod === "custom") {
        const basePeriod =
          previousPeriod === "custom" ? state.lastNonCustomPeriod : previousPeriod;
        setCustomDatesFromPeriod(basePeriod || "month");
        loadStats();
      } else {
        state.lastNonCustomPeriod = state.statsPeriod;
        localStorage.setItem("flexorama-stats-last-period", state.lastNonCustomPeriod);
        setCustomDatesFromPeriod(state.statsPeriod);
        loadStats();
      }
    });
  }

  if (statsStartDate) {
    statsStartDate.addEventListener("change", (e) => {
      state.statsStartDate = e.target.value;
      if (state.statsPeriod !== "custom") {
        state.statsPeriod = "custom";
        localStorage.setItem("flexorama-stats-period", state.statsPeriod);
        if (statsPeriodSelect) statsPeriodSelect.value = "custom";
      }
      if (state.statsStartDate && state.statsEndDate) loadStats();
    });
  }

  if (statsEndDate) {
    statsEndDate.addEventListener("change", (e) => {
      state.statsEndDate = e.target.value;
      if (state.statsPeriod !== "custom") {
        state.statsPeriod = "custom";
        localStorage.setItem("flexorama-stats-period", state.statsPeriod);
        if (statsPeriodSelect) statsPeriodSelect.value = "custom";
      }
      if (state.statsStartDate && state.statsEndDate) loadStats();
    });
  }

  const refreshStatsBtn = document.getElementById("refresh-stats");
  if (refreshStatsBtn) {
    refreshStatsBtn.addEventListener("click", () => {
      loadStats();
    });
  }
}

// Stats functions
function createTokenUsageChart(ctx, data) {
  if (!data || data.length === 0) return null;
  return new Chart(ctx, {
    type: 'line',
    data: {
      labels: data.map(d => d.date),
      datasets: [
        {
          label: 'Input Tokens',
          data: data.map(d => d.total_input_tokens),
          borderColor: CHART_COLORS.blue,
          backgroundColor: 'rgba(107, 124, 255, 0.1)',
          fill: true,
          tension: 0.4,
        },
        {
          label: 'Output Tokens',
          data: data.map(d => d.total_output_tokens),
          borderColor: CHART_COLORS.neonGreen,
          backgroundColor: 'rgba(57, 255, 20, 0.1)',
          fill: true,
          tension: 0.4,
        },
        {
          label: 'Total Tokens',
          data: data.map(d => d.total_tokens),
          borderColor: CHART_COLORS.pink,
          backgroundColor: 'rgba(255, 77, 216, 0.1)',
          fill: false,
          tension: 0.4,
          borderDash: [5, 5],
        },
      ],
    },
    options: { ...getChartDefaults() },
  });
}

function createRequestsChart(ctx, data) {
  if (!data || data.length === 0) return null;
  return new Chart(ctx, {
    type: 'line',
    data: {
      labels: data.map(d => d.date),
      datasets: [
        {
          label: 'Requests',
          data: data.map(d => d.total_requests),
          borderColor: CHART_COLORS.cyan,
          backgroundColor: 'rgba(124, 255, 178, 0.1)',
          fill: true,
          tension: 0.4,
        },
      ],
    },
    options: { ...getChartDefaults() },
  });
}

function createConversationsChart(ctx, data) {
  if (!data || data.length === 0) return null;
  return new Chart(ctx, {
    type: 'bar',
    data: {
      labels: data.map(d => d.date),
      datasets: [{
        label: 'Conversations',
        data: data.map(d => d.count),
        backgroundColor: CHART_COLORS.neonGreen,
        borderColor: CHART_COLORS.neonGreen,
        borderWidth: 1,
      }],
    },
    options: { ...getChartDefaults() },
  });
}

function createModelsChart(ctx, data) {
  if (!data || data.length === 0) return null;
  const colors = Object.values(CHART_COLORS);
  return new Chart(ctx, {
    type: 'pie',
    data: {
      labels: data.map(d => d.model),
      datasets: [{
        data: data.map(d => d.total_tokens),
        backgroundColor: data.map((_, i) => colors[i % colors.length]),
        borderColor: '#0b101a',
        borderWidth: 2,
      }],
    },
    options: {
      ...getChartDefaults(),
      scales: undefined,
    },
  });
}

function createProvidersChart(ctx, data) {
  if (!data || data.length === 0) return null;
  const colors = Object.values(CHART_COLORS);
  return new Chart(ctx, {
    type: 'doughnut',
    data: {
      labels: data.map(d => d.provider),
      datasets: [{
        data: data.map(d => d.total_tokens),
        backgroundColor: data.map((_, i) => colors[i % colors.length]),
        borderColor: '#0b101a',
        borderWidth: 2,
      }],
    },
    options: {
      ...getChartDefaults(),
      scales: undefined,
      cutout: '60%',
    },
  });
}

function createConversationsByProviderChart(ctx, data) {
  if (!data || data.length === 0) return null;

  // Aggregate data by provider
  const providerMap = new Map();
  data.forEach(item => {
    const current = providerMap.get(item.provider) || 0;
    providerMap.set(item.provider, current + item.count);
  });

  const providers = Array.from(providerMap.keys());
  const counts = Array.from(providerMap.values());
  const colors = Object.values(CHART_COLORS);

  return new Chart(ctx, {
    type: 'bar',
    data: {
      labels: providers,
      datasets: [{
        label: 'Conversations',
        data: counts,
        backgroundColor: providers.map((_, i) => colors[i % colors.length]),
        borderColor: providers.map((_, i) => colors[i % colors.length]),
        borderWidth: 1,
      }],
    },
    options: { ...getChartDefaults() },
  });
}

function createConversationsTimeByProviderChart(ctx, data) {
  if (!data || data.length === 0) return null;

  // Get unique dates and providers
  const dates = [...new Set(data.map(d => d.date))].sort();
  const providers = [...new Set(data.map(d => d.provider))];

  // Create datasets for each provider
  const colors = Object.values(CHART_COLORS);
  const datasets = providers.map((provider, idx) => {
    const providerData = dates.map(date => {
      const entry = data.find(d => d.date === date && d.provider === provider);
      return entry ? entry.count : 0;
    });

    return {
      label: provider,
      data: providerData,
      borderColor: colors[idx % colors.length],
      backgroundColor: colors[idx % colors.length] + '20',
      fill: false,
      tension: 0.4,
    };
  });

  return new Chart(ctx, {
    type: 'line',
    data: {
      labels: dates,
      datasets: datasets,
    },
    options: { ...getChartDefaults() },
  });
}

function createConversationsBySubagentChart(ctx, data) {
  if (!data || data.length === 0) return null;

  const subagentMap = new Map();
  data.forEach(item => {
    const current = subagentMap.get(item.subagent) || 0;
    subagentMap.set(item.subagent, current + item.count);
  });

  const subagents = Array.from(subagentMap.keys());
  const counts = Array.from(subagentMap.values());
  const colors = Object.values(CHART_COLORS);

  return new Chart(ctx, {
    type: 'bar',
    data: {
      labels: subagents,
      datasets: [{
        label: 'Conversations',
        data: counts,
        backgroundColor: subagents.map((_, i) => colors[i % colors.length]),
        borderColor: subagents.map((_, i) => colors[i % colors.length]),
        borderWidth: 1,
      }],
    },
    options: { ...getChartDefaults() },
  });
}

function createConversationsTimeBySubagentChart(ctx, data) {
  if (!data || data.length === 0) return null;

  const dates = [...new Set(data.map(d => d.date))].sort();
  const subagents = [...new Set(data.map(d => d.subagent))];
  const colors = [CHART_COLORS.neonGreen, CHART_COLORS.blue, CHART_COLORS.pink, CHART_COLORS.orange, CHART_COLORS.cyan, CHART_COLORS.purple, CHART_COLORS.yellow];

  const datasets = subagents.map((subagent, idx) => {
    const subagentData = dates.map(date => {
      const entry = data.find(d => d.date === date && d.subagent === subagent);
      return entry ? entry.count : 0;
    });

    return {
      label: subagent,
      data: subagentData,
      borderColor: colors[idx % colors.length],
      backgroundColor: colors[idx % colors.length] + '20',
      fill: false,
      tension: 0.4,
    };
  });

  return new Chart(ctx, {
    type: 'line',
    data: {
      labels: dates,
      datasets: datasets,
    },
    options: { ...getChartDefaults() },
  });
}

function aggregateByProvider(modelStats) {
  const providerMap = new Map();

  modelStats.forEach(stat => {
    const provider = stat.provider;
    if (!providerMap.has(provider)) {
      providerMap.set(provider, { provider, total_tokens: 0 });
    }
    providerMap.get(provider).total_tokens += stat.total_tokens;
  });

  return Array.from(providerMap.values());
}

function extractProvider(model) {
  const lower = model.toLowerCase();
  if (lower.includes('claude')) return 'Anthropic';
  if (lower.includes('gpt')) return 'OpenAI';
  if (lower.includes('gemini')) return 'Gemini';
  if (lower.includes('mistral')) return 'Mistral';
  if (lower.includes('glm')) return 'Z.AI';
  return 'Other';
}

function updateStatCards(overview) {
  document.getElementById('stat-conversations').textContent = overview.total_conversations.toLocaleString();
  document.getElementById('stat-messages').textContent = overview.total_messages.toLocaleString();
  document.getElementById('stat-tokens').textContent = overview.total_tokens.toLocaleString();
  document.getElementById('stat-requests').textContent = overview.total_requests.toLocaleString();
}

function updateStatsCharts() {
  // Destroy existing charts
  Object.values(state.statsCharts).forEach(chart => {
    if (chart) chart.destroy();
  });

  // Create new charts if we have data
  if (state.statsData.usage) {
    state.statsCharts.tokens = createTokenUsageChart(
      document.getElementById('chart-tokens')?.getContext('2d'),
      state.statsData.usage
    );
    state.statsCharts.requests = createRequestsChart(
      document.getElementById('chart-requests')?.getContext('2d'),
      state.statsData.usage
    );
  }

  if (state.statsData.conversations) {
    state.statsCharts.conversations = createConversationsChart(
      document.getElementById('chart-conversations')?.getContext('2d'),
      state.statsData.conversations
    );
  }

  if (state.statsData.models) {
    state.statsCharts.models = createModelsChart(
      document.getElementById('chart-models')?.getContext('2d'),
      state.statsData.models
    );

    // Aggregate by provider
    const providerData = aggregateByProvider(state.statsData.models);
    state.statsCharts.providers = createProvidersChart(
      document.getElementById('chart-providers')?.getContext('2d'),
      providerData
    );
  }

  if (state.statsData.conversationsByProvider) {
    state.statsCharts.conversationsByProvider = createConversationsByProviderChart(
      document.getElementById('chart-conversations-by-provider')?.getContext('2d'),
      state.statsData.conversationsByProvider
    );

    state.statsCharts.conversationsTimeByProvider = createConversationsTimeByProviderChart(
      document.getElementById('chart-conversations-time-by-provider')?.getContext('2d'),
      state.statsData.conversationsByProvider
    );
  }

  if (state.statsData.conversationsBySubagent) {
    state.statsCharts.subagents = createConversationsBySubagentChart(
      document.getElementById('chart-conversations-by-subagent')?.getContext('2d'),
      state.statsData.conversationsBySubagent
    );

    state.statsCharts.conversationsTimeBySubagent = createConversationsTimeBySubagentChart(
      document.getElementById('chart-conversations-time-by-subagent')?.getContext('2d'),
      state.statsData.conversationsBySubagent
    );
  }
}

async function loadStats() {
  const period = state.statsPeriod;

  try {
    // Load overview
    const overview = await api('/api/stats/overview');
    state.statsData.overview = overview;
    updateStatCards(overview);

    // Build query string based on period or custom dates
    let queryParams;
    if (period === 'custom' && state.statsStartDate && state.statsEndDate) {
      queryParams = `start_date=${state.statsStartDate}&end_date=${state.statsEndDate}`;
    } else {
      queryParams = `period=${period}`;
    }

    // Load usage data
    const usage = await api(`/api/stats/usage?${queryParams}`);
    state.statsData.usage = usage.data;

    // Load model stats
    const models = await api(`/api/stats/models?${queryParams}`);
    state.statsData.models = models.data;

    // Load conversation counts
    const conversations = await api(`/api/stats/conversations?${queryParams}`);
    state.statsData.conversations = conversations.data;

    // Load conversations by provider
    const conversationsByProvider = await api(`/api/stats/conversations-by-provider?${queryParams}`);
    state.statsData.conversationsByProvider = conversationsByProvider.data;

    // Load conversations by subagent
    const conversationsBySubagent = await api(`/api/stats/conversations-by-subagent?${queryParams}`);
    state.statsData.conversationsBySubagent = conversationsBySubagent.data;

    // Update charts
    updateStatsCharts();
  } catch (err) {
    console.error('Failed to load stats:', err);
  }
}

async function bootstrap() {
  // Load CSRF token from injected global variable
  state.csrfToken = window.FLEXORAMA_CSRF_TOKEN || null;
  if (!state.csrfToken) {
    console.error("CSRF token not found in page");
  }

  // Restore tab from URL
  const url = new URL(window.location);
  const tabFromUrl = url.searchParams.get("tab");
  if (tabFromUrl && document.querySelector(`.top-tab[data-tab=\"${tabFromUrl}\"]`)) {
    state.activeTab = tabFromUrl;
    document.querySelectorAll(".top-tab").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".tab-content").forEach((tab) => tab.classList.remove("active"));
    document.querySelector(`.top-tab[data-tab=\"${tabFromUrl}\"]`).classList.add("active");
    document.getElementById(`tab-${tabFromUrl}`).classList.add("active");
  }

  initTabs();
  initTheme();
  bindEvents();
  ensureStatsDateRange();
  const statsPeriodSelect = document.getElementById("stats-period");
  if (statsPeriodSelect) statsPeriodSelect.value = state.statsPeriod;
  try {
    await loadConversations();
    await loadPlans();
    await loadMcp();
    await loadAgents();
    await loadSkills();
    await loadCommands();
    await loadPlanMode();
    await loadTodos();
    await restoreSelections();
    switch (state.activeTab) {
      case "plans":
        selectFirstPlan();
        break;
      case "mcp":
        selectFirstMcp();
        break;
      case "agents":
        selectFirstAgent();
        break;
      case "skills":
        selectFirstSkill();
        break;
      case "commands":
        selectFirstCommand();
        break;
      case "stats":
        await loadStats();
        break;
      default:
        selectFirstConversation();
        break;
    }
    setStatus("Ready");
  } catch (err) {
    setStatus(`Startup failed: ${err.message}`);
  }
}

bootstrap();

async function showContextModal() {
  if (!state.activeConversationId) return;
  try {
    setStatus("Loading context...");
    const detail = await api(`/api/conversations/${state.activeConversationId}`);
    const meta = detail.conversation;
    const lines = [];
    lines.push("Current Conversation Context");
    lines.push("-".repeat(50));
    lines.push("");

    const files = detail.context_files || [];
    if (files.length > 0) {
      lines.push("Context files:");
      files.forEach((f) => lines.push(`- ${f}`));
      lines.push("");
    }

    if (meta.system_prompt) {
      lines.push("System Prompt:");
      lines.push(`  ${meta.system_prompt}`);
      lines.push("");
    }

    if (!detail.messages.length) {
      lines.push("No context yet. Start a conversation to see context here.");
    } else {
      detail.messages.forEach((m, idx) => {
        const role = m.role.toUpperCase();
        const preview = m.content.replace(/\n/g, " ");
        const truncated = preview.length > 100 ? `${preview.slice(0, 100)}...` : preview;
        lines.push(`[${idx + 1}] ${role}: (1 content block)`);
        lines.push(`  ▶ Block 1: Text ${truncated}`);
        lines.push("");
      });
    }

    document.getElementById("context-content").textContent = lines.join("\n");
    document.getElementById("context-modal").classList.add("open");
  } catch (err) {
    setStatus(`Failed to load context: ${err.message}`);
  } finally {
    setStatus("Ready");
  }
}

function closeContextModal() {
  document.getElementById("context-modal").classList.remove("open");
}
