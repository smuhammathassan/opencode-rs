const state = {
  sessions: [],
  selectedId: null,
  messages: [],
  busy: false,
};

const element = (id) => document.getElementById(id);

function setError(message = "") {
  element("error").textContent = message;
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    ...options,
    headers: { "Accept": "application/json", ...(options.headers || {}) },
  });
  const raw = await response.text();
  let body = null;
  if (raw) {
    try { body = JSON.parse(raw); } catch { body = raw; }
  }
  if (!response.ok) {
    const detail = body && typeof body === "object" ? (body.message || body.error) : body;
    throw new Error(detail || `${response.status} ${response.statusText}`);
  }
  return body;
}

function sessionId(session) {
  return session && (session.id || session.sessionID);
}

function sessionTitle(session) {
  return (session && session.title) || "Untitled session";
}

function textFrom(value) {
  if (!value) return "";
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(textFrom).filter(Boolean).join("\n");
  if (typeof value !== "object") return "";
  for (const key of ["text", "content", "parts", "message", "prompt", "data"]) {
    const text = textFrom(value[key]);
    if (text) return text;
  }
  return "";
}

function renderMessages() {
  const container = element("messages");
  container.replaceChildren();
  if (!state.selectedId) {
    container.innerHTML = '<p class="muted">Choose a session or start a new conversation.</p>';
    element("session-title").textContent = "No session selected";
    return;
  }
  const session = state.sessions.find((item) => sessionId(item) === state.selectedId);
  element("session-title").textContent = sessionTitle(session);
  if (!state.messages.length) {
    container.innerHTML = '<p class="muted">No messages in this session yet.</p>';
    return;
  }
  for (const message of state.messages) {
    const card = document.createElement("article");
    const role = message.role || message.type || "message";
    card.className = `message ${role === "user" ? "user" : "assistant"}`;
    const label = document.createElement("div");
    label.className = "message-role";
    label.textContent = role;
    const text = document.createElement("div");
    text.className = "message-text";
    text.textContent = textFrom(message) || "(empty message)";
    card.append(label, text);
    container.append(card);
  }
  container.lastElementChild?.scrollIntoView({ block: "nearest" });
}

function renderSessions() {
  const select = element("session-select");
  select.replaceChildren();
  for (const session of state.sessions) {
    const option = document.createElement("option");
    option.value = sessionId(session);
    option.textContent = sessionTitle(session);
    option.selected = option.value === state.selectedId;
    select.append(option);
  }
  element("session-empty").hidden = state.sessions.length > 0;
  if (!state.sessions.length) state.selectedId = null;
  renderMessages();
}

async function loadHealth() {
  const health = await api("/global/health");
  element("health").textContent = health?.healthy
    ? `Healthy${health.version ? ` · v${health.version}` : ""}`
    : "Server unavailable";
}

async function loadMessages() {
  if (!state.selectedId) { state.messages = []; renderMessages(); return; }
  const body = await api(`/session/${encodeURIComponent(state.selectedId)}/message`);
  state.messages = Array.isArray(body) ? body : (body?.data || body?.messages || []);
  renderMessages();
}

async function loadSessions(selectFirst = true) {
  const body = await api("/session");
  state.sessions = Array.isArray(body) ? body : (body?.data || []);
  if (!state.sessions.some((item) => sessionId(item) === state.selectedId)) {
    state.selectedId = selectFirst ? (sessionId(state.sessions[0]) || null) : null;
  }
  renderSessions();
  await loadMessages();
}

async function createSession() {
  const created = await api("/session", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ title: "Web Session" }),
  });
  const session = created?.data || created;
  state.selectedId = sessionId(session);
  await loadSessions(false);
  if (!state.selectedId) state.selectedId = sessionId(session);
  renderSessions();
  await loadMessages();
}

async function submitPrompt(event) {
  event.preventDefault();
  const input = element("prompt");
  const text = input.value.trim();
  if (!text || state.busy) return;
  state.busy = true;
  element("send").disabled = true;
  setError();
  try {
    if (!state.selectedId) await createSession();
    await api(`/session/${encodeURIComponent(state.selectedId)}/message`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ prompt: text, parts: [{ type: "text", text }] }),
    });
    input.value = "";
    await loadSessions(false);
    await loadMessages();
  } catch (error) {
    setError(error.message || String(error));
  } finally {
    state.busy = false;
    element("send").disabled = false;
  }
}

async function refresh() {
  setError();
  try { await Promise.all([loadHealth(), loadSessions(false)]); }
  catch (error) { setError(error.message || String(error)); }
}

element("session-select").addEventListener("change", async (event) => {
  state.selectedId = event.target.value || null;
  setError();
  try { await loadMessages(); } catch (error) { setError(error.message || String(error)); }
});
element("new-session").addEventListener("click", async () => {
  setError();
  try { await createSession(); } catch (error) { setError(error.message || String(error)); }
});
element("refresh").addEventListener("click", refresh);
element("prompt-form").addEventListener("submit", submitPrompt);

refresh();
setInterval(() => {
  if (!state.busy && state.selectedId) loadMessages().catch(() => {});
}, 2000);
