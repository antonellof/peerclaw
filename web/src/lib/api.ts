/** Typed wrappers for PeerClaw HTTP APIs (same paths as embedded HTML). */

function isLocalDevLoopbackHost(hostname: string): boolean {
  const h = hostname.toLowerCase()
  return (
    h === "localhost" ||
    h === "127.0.0.1" ||
    h === "::1" ||
    h === "[::1]" ||
    h.endsWith(".localhost")
  )
}

/** IPv6 literals need brackets in URLs (`http://[::1]:8080`). */
function hostForApiUrl(hostname: string): string {
  if (hostname.includes(":") && !hostname.startsWith("[")) {
    return `[${hostname}]`
  }
  return hostname
}

/**
 * Base URL for PeerClaw HTTP (no trailing slash).
 *
 * - `VITE_PEERCLAW_API` — explicit override (any host, production builds, LAN, etc.).
 * - **Vite dev on loopback:** if unset, defaults to `VITE_PEERCLAW_DEV_API`, or
 *   `http://<same-hostname-as-page>:<VITE_PEERCLAW_DEV_PORT||8080>` so the API host matches the UI
 *   (`localhost` vs `127.0.0.1`) and hits the node directly (avoids proxy HTML 404s).
 * - **Production / `peerclaw serve --web`:** empty → same-origin relative paths.
 */
function peerclawApiBase(): string {
  const raw = import.meta.env.VITE_PEERCLAW_API as string | undefined
  const explicit = raw?.trim()
  if (explicit) return explicit.replace(/\/$/, "")

  if (import.meta.env.DEV && typeof window !== "undefined") {
    const h = window.location.hostname
    if (isLocalDevLoopbackHost(h)) {
      const dev = (import.meta.env.VITE_PEERCLAW_DEV_API as string | undefined)?.trim()
      if (dev) return dev.replace(/\/$/, "")
      const port = (import.meta.env.VITE_PEERCLAW_DEV_PORT as string | undefined)?.trim() || "8080"
      const host = hostForApiUrl(h)
      // PeerClaw’s default listen is HTTP; avoid copying `https:` from an HTTPS Vite dev URL.
      return `http://${host}:${port}`.replace(/\/$/, "")
    }
  }

  return ""
}

export function apiUrl(path: string): string {
  const p = path.startsWith("/") ? path : `/${path}`
  const b = peerclawApiBase()
  return b ? `${b}${p}` : p
}

/** WebSocket URL for control channel; follows `VITE_PEERCLAW_API` host when set. */
export function peerclawWsUrl(): string {
  const b = peerclawApiBase()
  if (b) {
    let u: URL
    try {
      u = new URL(b)
    } catch {
      return `${window.location.protocol === "https:" ? "wss:" : "ws:"}//${window.location.host}/ws`
    }
    const wsProto = u.protocol === "https:" ? "wss:" : "ws:"
    return `${wsProto}//${u.host}/ws`
  }
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:"
  return `${proto}//${window.location.host}/ws`
}

function apiFetch(path: string, init?: RequestInit): Promise<Response> {
  return fetch(apiUrl(path), init)
}

export type StatusResponse = {
  peer_id: string
  connected_peers: number
  balance: number
  cpu_usage: number
  ram_used_mb: number
  ram_total_mb: number
  gpu_usage: number | null
  active_jobs: number
  completed_jobs: number
  active_inference: number
  active_web: number
  active_wasm: number
}

export async function fetchStatus(): Promise<StatusResponse> {
  const r = await apiFetch("/api/status")
  if (!r.ok) throw new Error(`status ${r.status}`)
  return r.json()
}

export type PeerInfo = { id: string; connected: boolean }

export async function fetchPeers(): Promise<PeerInfo[]> {
  const r = await apiFetch("/api/peers")
  if (!r.ok) throw new Error(`peers ${r.status}`)
  return r.json()
}

export type OnboardingStep = { id: string; ok: boolean; detail: string }

export type OnboardingResponse = { peer_id: string; steps: OnboardingStep[] }

export async function fetchOnboarding(): Promise<OnboardingResponse> {
  const r = await apiFetch("/api/onboarding")
  if (!r.ok) throw new Error(`onboarding ${r.status}`)
  return r.json()
}

export type WebJobInfo = {
  id: string
  job_type: string
  status: string
  provider: string | null
  requester: string
  price_micro: number
  created_at: number
  location: string | null
}

export async function fetchJobs(): Promise<WebJobInfo[]> {
  const r = await apiFetch("/api/jobs")
  if (!r.ok) throw new Error(`jobs ${r.status}`)
  return r.json()
}

export async function submitJob(payload: {
  job_type: string
  budget: number
  payload: string
}): Promise<{ success: boolean; job_id?: string; error?: string }> {
  const r = await apiFetch("/api/jobs/submit", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  })
  return r.json()
}

export type WebTask = {
  id: string
  task_type: string
  description: string
  status: string
  created_at: string
  completed_at: string | null
  result: string | null
  logs: string[]
  model: string | null
  budget: number
  tokens_used: number
  iterations: number
}

export async function fetchTasks(): Promise<WebTask[]> {
  const r = await apiFetch("/api/tasks")
  if (!r.ok) throw new Error(`tasks ${r.status}`)
  return r.json()
}

/** Request cooperative stop for an in-flight web agentic task (honoured between ReAct steps). */
export async function stopWebTask(id: string): Promise<{ ok: boolean; message?: string }> {
  const tid = id.trim()
  if (!tid) return { ok: false, message: "Missing task id." }
  const r = await apiFetch(`/api/tasks/${encodeURIComponent(tid)}/stop`, { method: "POST" })
  let data: unknown = {}
  try {
    data = await r.json()
  } catch {
    /* non-JSON */
  }
  const o = data as Record<string, unknown>
  const message = typeof o.message === "string" ? o.message : undefined
  const okFlag = o.ok === true
  if (!r.ok) {
    return { ok: false, message: message ?? `HTTP ${r.status}` }
  }
  return { ok: okFlag, message }
}

/** Normalized result from `GET /api/tasks/:id` (handles HTML/error bodies and shape quirks). */
export type TaskDetailResult =
  | { ok: true; task: WebTask }
  | { ok: false; message: string }

/** Parse JSON body from the task detail endpoint into a `WebTask` or a clear error. */
export function parseTaskDetailJson(data: unknown): TaskDetailResult {
  if (data == null || typeof data !== "object" || Array.isArray(data)) {
    return { ok: false, message: "Empty or invalid JSON (expected a task object)." }
  }
  const o = data as Record<string, unknown>

  // Server error payloads: { error: "..." , message?: "..." }
  if (typeof o.error === "string" && o.error.length > 0) {
    const extra = typeof o.message === "string" ? o.message : ""
    const msg =
      o.error === "Task not found"
        ? "Task not found (cleared from server or wrong id)."
        : o.error === "task_serialize_failed"
          ? `Task could not be serialized for display.${extra ? " " + extra : ""}`
          : extra || o.error
    return { ok: false, message: msg }
  }

  if (typeof o.id !== "string" || typeof o.status !== "string") {
    return { ok: false, message: "Response missing task id or status (got non-task JSON)." }
  }

  const logsRaw = o.logs
  const logs = Array.isArray(logsRaw) ? logsRaw.map((x) => (typeof x === "string" ? x : String(x))) : []

  const task: WebTask = {
    id: o.id,
    task_type: typeof o.task_type === "string" ? o.task_type : "general",
    description: typeof o.description === "string" ? o.description : "",
    status: o.status,
    created_at: typeof o.created_at === "string" ? o.created_at : "",
    completed_at: o.completed_at === null || typeof o.completed_at === "string" ? (o.completed_at as string | null) : null,
    result:
      o.result === null || typeof o.result === "string"
        ? (o.result as string | null)
        : o.result !== undefined
          ? String(o.result)
          : null,
    logs,
    model: o.model === null || typeof o.model === "string" ? (o.model as string | null) : null,
    budget: typeof o.budget === "number" && Number.isFinite(o.budget) ? o.budget : Number(o.budget) || 0,
    tokens_used: typeof o.tokens_used === "number" && Number.isFinite(o.tokens_used) ? o.tokens_used : 0,
    iterations: typeof o.iterations === "number" && Number.isFinite(o.iterations) ? o.iterations : 0,
  }

  return { ok: true, task }
}

function looksLikeHtml(s: string): boolean {
  const t = s.trim().toLowerCase()
  return t.startsWith("<!doctype") || t.startsWith("<html")
}

export async function fetchTaskDetail(id: string): Promise<TaskDetailResult> {
  const tid = id.trim().replace(/\/+$/, "")
  if (!tid) {
    return { ok: false, message: "Missing task id." }
  }

  // Single path segment; avoid trailing slash so Axum matchit routes `/api/tasks/{id}` reliably.
  const r = await apiFetch(`/api/tasks/${encodeURIComponent(tid)}`)
  const text = await r.text()
  const trimmed = text.trim()

  if (!r.ok) {
    if (looksLikeHtml(text)) {
      return {
        ok: false,
        message:
          `HTTP ${r.status}: response is HTML, not the PeerClaw API. ` +
          `Use \`npm run dev\` (proxy), open the app from \`peerclaw serve --web\`, or set VITE_PEERCLAW_API to your node URL.`,
      }
    }
    try {
      const data = JSON.parse(trimmed) as unknown
      const parsed = parseTaskDetailJson(data)
      if (!parsed.ok) return parsed
    } catch {
      /* fall through */
    }
    const snippet = trimmed.slice(0, 120)
    return {
      ok: false,
      message: `HTTP ${r.status}${snippet ? `: ${snippet}` : ""}`,
    }
  }

  if (!trimmed) {
    return { ok: false, message: "Empty response from /api/tasks/:id." }
  }

  let data: unknown
  try {
    data = JSON.parse(trimmed) as unknown
  } catch {
    return {
      ok: false,
      message: looksLikeHtml(text)
        ? "Server returned HTML instead of JSON — the UI is not reaching PeerClaw’s /api (check dev proxy or VITE_PEERCLAW_API)."
        : "Server returned non-JSON (check proxy / route).",
    }
  }

  return parseTaskDetailJson(data)
}

export async function createTask(payload: {
  task_type: string
  description: string
  model?: string | null
  budget?: number
  use_mcp?: boolean
}): Promise<{ success: boolean; task_id?: string; error?: string }> {
  const r = await apiFetch("/api/tasks", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  })
  return r.json()
}

export type SwarmAgentInfo = {
  id: string
  name: string
  state: string
  is_local: boolean
  action_count: number
  jobs_completed: number
  jobs_failed: number
  success_rate: number
  created_at: string
  last_active_at: string
}

export type SwarmAgentsResponse = { agents: SwarmAgentInfo[]; total: number }

export type TopologyNode = {
  id: string
  name: string
  state: string
  is_local: boolean
  action_count: number
  success_rate: number
}

export type TopologyEdge = { source: string; target: string }

export type SwarmTopologyResponse = {
  nodes: TopologyNode[]
  edges: TopologyEdge[]
  timestamp: string
}

export type SwarmActionInfo = {
  id: string
  agent_id: string
  agent_name: string
  action_type: string
  details: string
  timestamp: string
}

export type SwarmTimelineResponse = {
  actions: SwarmActionInfo[]
  total: number
  has_more: boolean
}

export async function fetchSwarmAgents(): Promise<SwarmAgentsResponse> {
  const r = await apiFetch("/api/swarm/agents")
  if (!r.ok) throw new Error(`swarm agents ${r.status}`)
  return r.json()
}

export async function fetchSwarmTopology(): Promise<SwarmTopologyResponse> {
  const r = await apiFetch("/api/swarm/topology")
  if (!r.ok) throw new Error(`swarm topology ${r.status}`)
  return r.json()
}

export async function fetchSwarmTimeline(): Promise<SwarmTimelineResponse> {
  const r = await apiFetch("/api/swarm/timeline")
  if (!r.ok) throw new Error(`swarm timeline ${r.status}`)
  return r.json()
}

export type ProviderModelInfo = {
  model_name: string
  context_size: number
  price_per_1k_tokens: number
  backend: string
}

export type ProviderInfo = {
  peer_id: string
  models: ProviderModelInfo[]
  max_requests_per_hour: number
  max_tokens_per_day: number
}

export type ProviderConfigResponse = {
  enabled: boolean
  max_requests_per_hour: number
  max_tokens_per_day: number
  max_concurrent_requests: number
  price_multiplier: number
}

export async function fetchProviders(): Promise<ProviderInfo[]> {
  const r = await apiFetch("/api/providers")
  if (!r.ok) throw new Error(`providers ${r.status}`)
  return r.json()
}

export async function fetchProviderConfig(): Promise<ProviderConfigResponse> {
  const r = await apiFetch("/api/providers/config")
  if (!r.ok) throw new Error(`provider config ${r.status}`)
  return r.json()
}

export async function setProviderConfig(payload: {
  enabled?: boolean
  price_multiplier?: number
  max_requests_per_hour?: number
  max_tokens_per_day?: number
  max_concurrent_requests?: number
}): Promise<{ success?: boolean; error?: string }> {
  const r = await apiFetch("/api/providers/config", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  })
  return r.json()
}

export type NodeDetailResponse = {
  id: string
  is_local: boolean
  name: string
  state: string
  tasks: WebTask[]
  models: ProviderModelInfo[]
  action_count: number
  success_rate: number
}

export async function fetchNodeDetail(nodeId: string): Promise<NodeDetailResponse> {
  const r = await apiFetch(`/api/nodes/${encodeURIComponent(nodeId)}`)
  if (!r.ok) throw new Error(`node ${r.status}`)
  return r.json()
}

export type SkillInfo = {
  name: string
  version: string
  description: string
  trust: string
  available: boolean
  provider: string
  price: number
}

export async function fetchSkillsLocal(): Promise<SkillInfo[]> {
  const r = await apiFetch("/api/skills/local")
  if (!r.ok) throw new Error(`skills local ${r.status}`)
  return r.json()
}

export async function fetchSkillsNetwork(): Promise<SkillInfo[]> {
  const r = await apiFetch("/api/skills/network")
  if (!r.ok) throw new Error(`skills network ${r.status}`)
  return r.json()
}

export type SkillsMetaResponse = {
  skills_dir: string
  config_path: string
  registry_attached: boolean
  scan_cli: string
  list_cli: string
  directory_toml_snippet: string
}

export async function fetchSkillsMeta(): Promise<SkillsMetaResponse> {
  const r = await apiFetch("/api/skills/meta")
  if (!r.ok) throw new Error(`skills meta ${r.status}`)
  return r.json()
}

export async function scanSkills(): Promise<{ ok: boolean; loaded?: number; error?: string }> {
  const r = await apiFetch("/api/skills/scan", { method: "POST" })
  return r.json()
}

export type SkillStudioListEntry = { slug: string; layout: string }

export async function fetchSkillStudioList(): Promise<SkillStudioListEntry[]> {
  const r = await apiFetch("/api/skills/studio")
  if (!r.ok) throw new Error(`skill studio list ${r.status}`)
  return r.json()
}

export type SkillStudioGetResponse = { slug: string; content: string; layout: string }

export async function fetchSkillStudio(slug: string): Promise<SkillStudioGetResponse> {
  const r = await apiFetch(`/api/skills/studio/${encodeURIComponent(slug)}`)
  if (!r.ok) {
    const j = (await r.json().catch(() => null)) as { error?: string } | null
    throw new Error(j?.error ?? `load ${r.status}`)
  }
  return r.json()
}

export async function saveSkillStudio(
  slug: string,
  content: string,
): Promise<{ ok: boolean; slug?: string; path?: string }> {
  const r = await apiFetch(`/api/skills/studio/${encodeURIComponent(slug)}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ content }),
  })
  const j = (await r.json()) as { ok?: boolean; error?: string; slug?: string; path?: string }
  if (!r.ok) throw new Error(j.error ?? `save ${r.status}`)
  return { ok: !!j.ok, slug: j.slug, path: j.path }
}

export type SkillStudioAiResponse = { text: string; tokens: number }

export async function skillStudioAi(payload: {
  content: string
  instruction: string
  model?: string
  max_tokens?: number
  temperature?: number
}): Promise<SkillStudioAiResponse> {
  const r = await apiFetch("/api/skills/studio/ai", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  })
  const j = (await r.json()) as SkillStudioAiResponse & { error?: string }
  if (!r.ok) throw new Error(j.error ?? `ai ${r.status}`)
  return { text: j.text, tokens: j.tokens }
}

export type McpToolListItem = { id: string; description: string | null }

export type McpStatusResponse = {
  mode: string
  in_core: boolean
  config: {
    enabled: boolean
    servers: Array<{
      name: string
      url: string
      env?: Record<string, string>
      command?: string | null
      args?: string[]
    }>
    timeout_secs: number
    auto_reconnect: boolean
  }
  config_path: string
  connected_servers: string[]
  tool_count: number
  tools: McpToolListItem[]
  mcp_toml_snippet: string
  hint: string
  spec_url: string
}

export type McpConfigJson = {
  enabled: boolean
  timeout_secs: number
  auto_reconnect: boolean
  servers: Array<{
    name: string
    url: string
    command?: string
    args?: string[]
    env?: Record<string, string>
  }>
}

export async function fetchMcpStatus(): Promise<McpStatusResponse> {
  const r = await apiFetch("/api/mcp/status")
  if (!r.ok) throw new Error(`mcp ${r.status}`)
  return r.json()
}

export async function putMcpConfig(cfg: McpConfigJson): Promise<{ ok: boolean }> {
  const r = await apiFetch("/api/mcp/config", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(cfg),
  })
  const j = (await r.json().catch(() => null)) as { ok?: boolean; error?: string } | null
  if (!r.ok) throw new Error(j?.error ?? `mcp config ${r.status}`)
  return { ok: !!j?.ok }
}

export type ChatResponse = {
  response: string
  tokens: number
  tokens_per_second: number
  location: string
  provider_peer_id: string | null
}

export async function postChat(payload: {
  message: string
  model?: string
  max_tokens?: number
  temperature?: number
  session_id?: string | null
  agentic?: boolean
  use_mcp?: boolean
}): Promise<ChatResponse> {
  const r = await apiFetch("/api/chat", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      message: payload.message,
      model: payload.model,
      max_tokens: payload.max_tokens,
      temperature: payload.temperature,
      ...(payload.session_id ? { session_id: payload.session_id } : {}),
      ...(payload.agentic !== undefined ? { agentic: payload.agentic } : {}),
      ...(payload.use_mcp !== undefined ? { use_mcp: payload.use_mcp } : {}),
    }),
  })
  return r.json()
}

/** SSE (`text/event-stream`): `delta` chunks then a final `done` object matching {@link ChatResponse}. */
export async function postChatStream(
  payload: {
    message: string
    model?: string
    max_tokens?: number
    temperature?: number
    session_id?: string | null
    agentic?: boolean
    use_mcp?: boolean
  },
  onDelta: (text: string) => void,
  signal?: AbortSignal,
): Promise<ChatResponse> {
  const body = JSON.stringify({
    message: payload.message,
    model: payload.model,
    max_tokens: payload.max_tokens,
    temperature: payload.temperature,
    ...(payload.session_id ? { session_id: payload.session_id } : {}),
    ...(payload.agentic !== undefined ? { agentic: payload.agentic } : {}),
    ...(payload.use_mcp !== undefined ? { use_mcp: payload.use_mcp } : {}),
  })

  const r = await apiFetch("/api/chat/stream", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "text/event-stream",
    },
    body,
    signal,
  })

  // 405/404 often means the request hit the static SPA layer (old binary or proxy quirk) — fall back to non-streaming API.
  if (r.status === 405 || r.status === 404) {
    const j = await postChat({
      message: payload.message,
      model: payload.model,
      max_tokens: payload.max_tokens,
      temperature: payload.temperature,
      session_id: payload.session_id,
      agentic: payload.agentic,
      use_mcp: payload.use_mcp,
    })
    if (j.response) onDelta(j.response)
    return j
  }

  if (!r.ok) {
    const errBody = (await r.json().catch(() => null)) as Partial<ChatResponse> | null
    if (errBody?.response) throw new Error(errBody.response)
    throw new Error(`chat stream failed (${r.status})`)
  }
  const reader = r.body?.getReader()
  if (!reader) throw new Error("No response body")

  const decoder = new TextDecoder()
  let buffer = ""
  let donePayload: ChatResponse | null = null

  while (true) {
    const { value, done } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    for (;;) {
      const nl = buffer.indexOf("\n")
      if (nl < 0) break
      const line = buffer.slice(0, nl).replace(/\r$/, "")
      buffer = buffer.slice(nl + 1)
      if (!line.startsWith("data: ")) continue
      const raw = line.slice(6).trim()
      if (raw === "[DONE]") continue
      let j: {
        type?: string
        text?: string
        response?: string
        tokens?: number
        tokens_per_second?: number
        location?: string
        provider_peer_id?: string | null
      }
      try {
        j = JSON.parse(raw) as typeof j
      } catch {
        continue
      }
      if (j.type === "delta" && typeof j.text === "string") onDelta(j.text)
      if (j.type === "done") {
        donePayload = {
          response: j.response ?? "",
          tokens: j.tokens ?? 0,
          tokens_per_second: j.tokens_per_second ?? 0,
          location: j.location ?? "",
          provider_peer_id: j.provider_peer_id ?? null,
        }
      }
    }
  }

  if (!donePayload) throw new Error("Stream ended without completion")
  return donePayload
}

export type OpenAiModel = { id: string; object?: string }

export type ModelsListResponse = { data?: OpenAiModel[] }

export async function fetchOpenAiModels(): Promise<OpenAiModel[]> {
  const r = await apiFetch("/v1/models")
  if (!r.ok) throw new Error(`models ${r.status}`)
  const j: ModelsListResponse = await r.json()
  return j.data ?? []
}
