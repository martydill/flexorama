const READONLY_TOOLS = ["search_in_files", "glob"];

const state = {
  conversations: [],
  activeConversationId: localStorage.getItem("aixplosion-active-conversation"),
  plans: [],
  activePlanId: localStorage.getItem("aixplosion-active-plan"),
  mcpServers: [],
  activeServer: localStorage.getItem("aixplosion-active-mcp"),
  agents: [],
  activeAgent: null,
  activeAgentEditing: localStorage.getItem("aixplosion-active-agent-edit"),
  provider: null,
  models: [],
  activeModel: null,
  theme: "dark",
  activeTab: "chats",
  streaming: localStorage.getItem("aixplosion-stream") === "true",
  pendingPermissions: new Set(),
};

function setPlanForm(plan) {
  state.activePlanId = plan.id;
  localStorage.setItem("aixplosion-active-plan", String(plan.id));
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
  localStorage.setItem("aixplosion-theme", theme);
}

function setStatus(text) {
  document.getElementById("conversation-meta").textContent = text;
}

async function api(path, options = {}) {
  const opts = { headers: { "Content-Type": "application/json" }, ...options };
  if (opts.body && typeof opts.body !== "string") {
    opts.body = JSON.stringify(opts.body);
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

function renderConversationList() {
  const list = document.getElementById("conversation-list");
  list.innerHTML = "";
  state.conversations.forEach((conv) => {
    const item = document.createElement("div");
    const isActive = String(conv.id) === String(state.activeConversationId);
    item.className = "list-item" + (isActive ? " active" : "");
    item.innerHTML = `
      <div style="font-weight:600;">${conv.last_message ? conv.last_message.slice(0, 50) : "new chat"}</div>
      <small>${new Date(conv.updated_at).toLocaleString()} • ${conv.model}</small>
    `;
    item.addEventListener("click", () => selectConversation(conv.id));
    list.appendChild(item);
  });
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
  const blocks = normalizeBlocks(msg.blocks, msg.content);
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
  buttons.forEach((btn) => (btn.disabled = true));
  if (status) status.textContent = "Submitting response...";
  try {
    await api("/api/permissions/respond", {
      method: "POST",
      body: { id, selection },
    });
    if (status) status.textContent = "Response sent.";
    state.pendingPermissions.delete(id);
  } catch (err) {
    if (status) status.textContent = `Failed: ${err.message}`;
    buttons.forEach((btn) => (btn.disabled = false));
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
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
  const data = await api("/api/conversations");
  mergeConversations(data);
  renderConversationList();
  if (!state.activeConversationId && data.length > 0) {
    await selectConversation(data[0].id);
  }
}

async function selectConversation(id) {
  state.activeConversationId = id;
  localStorage.setItem("aixplosion-active-conversation", String(id));
  state.pendingPermissions.clear();
  renderConversationList();
  setStatus("Loading conversation...");
  const detail = await api(`/api/conversations/${id}`);
  const meta = detail.conversation;
  setStatus(`${detail.messages.length} messages • ${meta.model}`);
  renderMessages(detail.messages);
  const select = document.getElementById("agent-selector");
  if (select) {
    select.value = meta.subagent || "";
  }
  await loadModels();
  await loadPendingPermissions();
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
  mergeConversations([placeholder]);
  if (newId) {
    await selectConversation(newId);
  }
  const input = document.getElementById("message-input");
  if (input) input.focus();
  await loadConversations();
  await loadModels();
}

async function sendMessage() {
  const input = document.getElementById("message-input");
  const text = input.value.trim();
  if (!text || !state.activeConversationId) return;

  appendMessage("user", text);
  updateConversationPreview(state.activeConversationId, text);
  input.value = "";
  if (state.streaming) {
    await sendMessageStreaming(text);
  } else {
    await sendMessageOnce(text);
  }
}

async function sendMessageOnce(text) {
  setStatus("Waiting for response...");
  const poller = startPermissionPolling();
  try {
    const result = await api(`/api/conversations/${state.activeConversationId}/message`, {
      method: "POST",
      body: { message: text },
    });
    appendMessage("assistant", result.response || "(empty response)");
    setStatus("Ready");
    await loadConversations();
  } catch (err) {
    appendMessage("assistant", `Error: ${err.message}`);
    setStatus("Error");
  } finally {
    if (poller) poller.stopped = true;
  }
}

async function sendMessageStreaming(text) {
  setStatus("Streaming response...");
  const bubble = createEmptyBubble("assistant");
  showTypingIndicator(bubble);
  let toolBubble = null;
  let buffer = "";
  let currentText = "";
  const poller = startPermissionPolling();

  try {
    const res = await fetch(`/api/conversations/${state.activeConversationId}/message/stream`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message: text }),
    });

    if (!res.ok) {
      const message = await res.text();
      throw new Error(message || `Request failed: ${res.status}`);
    }
    if (!res.body) {
      throw new Error("Streaming not supported by browser");
    }

    const reader = res.body.getReader();
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
          currentText += evt.delta;
          updateBubbleContent(bubble, currentText);
        } else if (evt.type === "final" && typeof evt.content === "string") {
          currentText = evt.content;
          updateBubbleContent(bubble, currentText);
        } else if (evt.type === "tool_call") {
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
        } else if (evt.type === "tool_result") {
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
        } else if (evt.type === "permission_request") {
          renderPermissionRequest(evt);
        } else if (evt.type === "error") {
          updateBubbleContent(bubble, `Error: ${evt.error || "stream error"}`);
          setStatus("Error");
        }
      }
    }

    setStatus("Refreshing chat...");
    await selectConversation(state.activeConversationId);
    await loadConversations();
    setStatus("Ready");
  } catch (err) {
    updateBubbleContent(bubble, `Error: ${err.message}`);
    setStatus("Error");
  } finally {
    if (poller) poller.stopped = true;
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
  localStorage.removeItem("aixplosion-active-plan");
  document.getElementById("plan-title").value = "";
  document.getElementById("plan-user-request").value = "";
  document.getElementById("plan-markdown").value = "";
}

function resetMcpForm() {
  state.activeServer = null;
  localStorage.removeItem("aixplosion-active-mcp");
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
  localStorage.setItem("aixplosion-active-mcp", server.name);
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
  localStorage.setItem("aixplosion-active-plan", String(res.id));
  await loadPlans();
}

async function deletePlan() {
  if (!state.activePlanId) return;
  await api(`/api/plans/${state.activePlanId}`, { method: "DELETE" });
  resetPlanForm();
  await loadPlans();
  selectFirstPlan();
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
  localStorage.removeItem("aixplosion-active-agent-edit");
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
  localStorage.setItem("aixplosion-active-agent-edit", agent.name);
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

async function loadModels() {
  const data = await api("/api/models");
  state.provider = data.provider;
  state.models = Array.isArray(data.models) ? data.models : [];
  state.activeModel = data.active_model;
  renderModelSelector();
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
  const savedConv = localStorage.getItem("aixplosion-active-conversation");
  if (savedConv && state.conversations.some((c) => String(c.id) === String(savedConv))) {
    await selectConversation(savedConv);
  }

  const savedPlan = localStorage.getItem("aixplosion-active-plan");
  const planMatch = savedPlan && state.plans.find((p) => String(p.id) === String(savedPlan));
  if (planMatch) {
    setPlanForm(planMatch);
  }

  const savedMcp = localStorage.getItem("aixplosion-active-mcp");
  const mcpMatch = savedMcp && state.mcpServers.find((s) => s.name === savedMcp);
  if (mcpMatch) {
    setMcpForm(mcpMatch);
  }

  const savedAgentEdit = localStorage.getItem("aixplosion-active-agent-edit");
  const agentMatch = savedAgentEdit && state.agents.find((a) => a.name === savedAgentEdit);
  if (agentMatch) {
    setAgentForm(agentMatch);
  }
}

// Tabs
function initTabs() {
  document.querySelectorAll(".top-tab").forEach((btn) => {
    btn.addEventListener("click", () => {
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
          selectFirstPlan();
          break;
        case "mcp":
          selectFirstMcp();
          break;
        case "agents":
          selectFirstAgent();
          break;
        default:
          selectFirstConversation();
          break;
      }
    });
  });
}

function initTheme() {
  const stored = localStorage.getItem("aixplosion-theme");
  const initial = stored === "light" || stored === "dark" ? stored : "dark";
  applyTheme(initial);
  const toggle = document.getElementById("mode-toggle");
  if (toggle) {
    toggle.addEventListener("click", () => {
      applyTheme(state.theme === "light" ? "dark" : "light");
    });
  }
}

function bindEvents() {
  document.getElementById("send-message").addEventListener("click", sendMessage);
  document.getElementById("message-input").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  });
  const streamToggle = document.getElementById("stream-toggle");
  if (streamToggle) {
    streamToggle.checked = state.streaming;
    streamToggle.addEventListener("change", (e) => {
      state.streaming = e.target.checked;
      localStorage.setItem("aixplosion-stream", String(state.streaming));
    });
  }
  document.getElementById("new-conversation").addEventListener("click", createConversation);

  document.getElementById("save-plan").addEventListener("click", savePlan);
  const createPlanBtn = document.getElementById("create-plan");
  if (createPlanBtn) createPlanBtn.addEventListener("click", createPlan);
  document.getElementById("create-plan-sidebar").addEventListener("click", createPlan);
  document.getElementById("delete-plan").addEventListener("click", deletePlan);

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
      setStatus(`Model set to ${model}`);
    } catch (err) {
      setStatus(`Model update failed: ${err.message}`);
    }
  });
  document.getElementById("show-context").addEventListener("click", showContextModal);
  document.getElementById("close-context").addEventListener("click", closeContextModal);
  document.getElementById("context-modal").addEventListener("click", (e) => {
    if (e.target.id === "context-modal") closeContextModal();
  });
}

async function bootstrap() {
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
  try {
    await loadConversations();
    await loadPlans();
    await loadMcp();
    await loadAgents();
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



