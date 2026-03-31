import { useCallback, useEffect, useState } from "react"
import {
  Check,
  ChevronRight,
  Circle,
  Cloud,
  Download,
  ExternalLink,
  HardDrive,
  Loader2,
  Monitor,
  Plug,
  Plus,
  Sparkles,
  X,
} from "lucide-react"

import {
  downloadGgufModel,
  fetchInferenceSettings,
  fetchMcpStatus,
  fetchOllamaModels,
  fetchOpenAiModels,
  pullOllamaModel,
  putInferenceSettings,
  putMcpConfig,
  type GgufPresetRow,
  type InferenceSettingsResponse,
  type McpConfigJson,
  type OllamaModel,
} from "@/lib/api"
import { useControlWebSocket } from "@/hooks/useControlWebSocket"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { cn } from "@/lib/utils"

const LS_SETUP_DONE = "peerclaw_setup_done"

export function isSetupDone(): boolean {
  try {
    return localStorage.getItem(LS_SETUP_DONE) === "1"
  } catch {
    return false
  }
}

export function markSetupDone() {
  try {
    localStorage.setItem(LS_SETUP_DONE, "1")
  } catch {
    /* ignore */
  }
}

type Step = "welcome" | "inference" | "models" | "mcp" | "done"
const STEPS: Step[] = ["welcome", "inference", "models", "mcp", "done"]
const STEP_LABELS: Record<Step, string> = {
  welcome: "Welcome",
  inference: "AI Setup",
  models: "Models",
  mcp: "MCP",
  done: "Done",
}

// Model info now comes from the server via gguf_presets (loaded from templates/models/gguf-presets.json)

const MCP_PRESETS = [
  { name: "filesystem", label: "Filesystem", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"], desc: "Read/write files" },
  { name: "brave-search", label: "Brave Search", command: "npx", args: ["-y", "@anthropic/mcp-server-brave-search"], desc: "Web search (needs BRAVE_API_KEY)" },
  { name: "github", label: "GitHub", command: "npx", args: ["-y", "@modelcontextprotocol/server-github"], desc: "GitHub repos/issues (needs GITHUB_TOKEN)" },
  { name: "sqlite", label: "SQLite", command: "npx", args: ["-y", "@anthropic/mcp-server-sqlite", "/tmp/data.db"], desc: "Query SQLite databases" },
]

function formatSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(0)} MB`
  return `${bytes} B`
}

/* ── Status icon (uniform) ──────────────────────────────────────── */
function StatusIcon({ ok }: { ok: boolean }) {
  return ok ? (
    <div className="flex size-5 items-center justify-center rounded-full bg-emerald-500/15">
      <Check className="size-3 text-emerald-500" />
    </div>
  ) : (
    <div className="flex size-5 items-center justify-center rounded-full bg-muted">
      <Circle className="size-2.5 text-muted-foreground/40" />
    </div>
  )
}

/* ── Main component ─────────────────────────────────────────────── */

export function SetupPage({ onFinish }: { onFinish: () => void }) {
  const [step, setStep] = useState<Step>("welcome")
  const [settings, setSettings] = useState<InferenceSettingsResponse | null>(null)
  const [models, setModels] = useState<string[]>([])
  const [ggufPresets, setGgufPresets] = useState<GgufPresetRow[]>([])
  const [ollamaModels, setOllamaModels] = useState<OllamaModel[]>([])
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Inference form
  const [useLocalGguf, setUseLocalGguf] = useState(true) // default ON
  const [useOllama, setUseOllama] = useState(false)
  const [ollamaUrl, setOllamaUrl] = useState("http://localhost:11434")
  const [useRemoteApi, setUseRemoteApi] = useState(false)
  const [remoteApiUrl, setRemoteApiUrl] = useState("")
  const [remoteApiModel, setRemoteApiModel] = useState("")
  const [remoteApiKey, setRemoteApiKey] = useState("")

  // Downloads
  const [downloading, setDownloading] = useState<string | null>(null)
  const [downloadMsg, setDownloadMsg] = useState<string | null>(null)
  const [downloadPct, setDownloadPct] = useState<number | null>(null)
  const [customUrl, setCustomUrl] = useState("")
  const [ollamaPullName, setOllamaPullName] = useState("")
  const [ollamaPulling, setOllamaPulling] = useState(false)
  const [ollamaPullMsg, setOllamaPullMsg] = useState<string | null>(null)

  useControlWebSocket({
    onDownloadProgress: (ev) => {
      if (ev.percent != null) setDownloadPct(ev.percent)
    },
  })

  // MCP
  const [mcpEnabled, setMcpEnabled] = useState(false)
  const [mcpServers, setMcpServers] = useState<McpConfigJson["servers"]>([])
  const [mcpSaving, setMcpSaving] = useState(false)

  const loadAll = useCallback(async () => {
    try {
      const s = await fetchInferenceSettings()
      setSettings(s)
      setUseLocalGguf(s.use_local_gguf || !s.use_ollama)
      setUseOllama(s.use_ollama)
      setOllamaUrl(s.ollama_url || "http://localhost:11434")
      setUseRemoteApi(s.remote_api_enabled)
      setRemoteApiUrl(s.remote_api_base_url)
      setRemoteApiModel(s.remote_api_model)
      setGgufPresets(s.gguf_presets || [])
    } catch { /* defaults */ }
    try { setModels((await fetchOpenAiModels()).map((x) => x.id)) } catch { setModels([]) }
    try { setOllamaModels(await fetchOllamaModels()) } catch { setOllamaModels([]) }
    try {
      const mcp = await fetchMcpStatus()
      setMcpEnabled(mcp.config.enabled)
      setMcpServers((mcp.config.servers || []).map((s) => ({ ...s, command: s.command ?? undefined })))
    } catch { /* */ }
  }, [])

  useEffect(() => { void loadAll() }, [loadAll])

  // Refresh Ollama models when URL changes
  const refreshOllama = useCallback(async () => {
    try { setOllamaModels(await fetchOllamaModels()) } catch { setOllamaModels([]) }
  }, [])

  const handleOllamaPull = async () => {
    const name = ollamaPullName.trim()
    if (!name) return
    setOllamaPulling(true)
    setOllamaPullMsg(null)
    try {
      const r = await pullOllamaModel(name)
      if (r.success) {
        setOllamaPullMsg(`Pulled ${name}`)
        setOllamaPullName("")
        await refreshOllama()
      } else {
        setOllamaPullMsg(r.error ?? "Pull failed")
      }
    } catch (e) {
      setOllamaPullMsg(e instanceof Error ? e.message : "Error")
    } finally { setOllamaPulling(false) }
  }

  const saveInference = async () => {
    setSaving(true)
    setError(null)
    try {
      await putInferenceSettings({
        use_local_gguf: useLocalGguf,
        use_ollama: useOllama,
        ollama_url: ollamaUrl,
        remote_api_enabled: useRemoteApi,
        remote_api_base_url: remoteApiUrl,
        remote_api_model: remoteApiModel,
        ...(remoteApiKey ? { remote_api_key: remoteApiKey } : {}),
      })
      const m = await fetchOpenAiModels()
      setModels(m.map((x) => x.id))
      if (useOllama) await refreshOllama()
      setStep("models")
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to save")
    } finally { setSaving(false) }
  }

  const handleDownload = async (presetId: string) => {
    setDownloading(presetId)
    setDownloadMsg(null)
    setDownloadPct(0)
    try {
      const r = await downloadGgufModel({ preset: presetId, quant: "Q4_K_M" })
      if (r.success) {
        setDownloadMsg(`Downloaded ${presetId}`)
        setModels((await fetchOpenAiModels()).map((x) => x.id))
      } else {
        setDownloadMsg(r.error ?? "Download failed")
      }
    } catch (e) {
      setDownloadMsg(e instanceof Error ? e.message : "Error")
    } finally { setDownloading(null); setDownloadPct(null) }
  }

  const handleUrlDownload = async () => {
    const url = customUrl.trim()
    if (!url) return
    setDownloading("custom")
    setDownloadMsg(null)
    setDownloadPct(0)
    try {
      const r = await downloadGgufModel({ url })
      if (r.success) {
        setDownloadMsg(`Downloaded to ${r.path ?? "models dir"}`)
        setModels((await fetchOpenAiModels()).map((x) => x.id))
        setCustomUrl("")
      } else {
        setDownloadMsg(r.error ?? "Download failed")
      }
    } catch (e) {
      setDownloadMsg(e instanceof Error ? e.message : "Error")
    } finally { setDownloading(null); setDownloadPct(null) }
  }

  const saveMcp = async () => {
    setMcpSaving(true)
    try { await putMcpConfig({ enabled: mcpEnabled, timeout_secs: 30, auto_reconnect: true, servers: mcpServers }) } catch { /* */ }
    setMcpSaving(false)
    setStep("done")
  }

  const addMcpPreset = (p: typeof MCP_PRESETS[number]) => {
    if (mcpServers.some((s) => s.name === p.name)) return
    setMcpServers([...mcpServers, { name: p.name, url: "", command: p.command, args: p.args }])
    setMcpEnabled(true)
  }

  const finish = () => { markSetupDone(); onFinish() }
  const stepIdx = STEPS.indexOf(step)

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <div className="w-full max-w-xl space-y-6">
        {/* ── Progress bar ─────────────────────────────── */}
        <div className="flex items-center justify-center gap-1.5">
          {STEPS.map((s, i) => (
            <div key={s} className="flex items-center gap-1.5">
              <div className={cn(
                "flex size-7 items-center justify-center rounded-full text-[11px] font-semibold transition-colors",
                stepIdx === i ? "bg-primary text-primary-foreground"
                  : stepIdx > i ? "bg-primary/20 text-primary"
                    : "bg-muted text-muted-foreground",
              )}>
                {stepIdx > i ? <Check className="size-3.5" /> : i + 1}
              </div>
              {i < STEPS.length - 1 && <div className="h-px w-6 bg-border" />}
            </div>
          ))}
        </div>
        <p className="text-center text-xs text-muted-foreground">{STEP_LABELS[step]}</p>

        {/* ══════════════════════════════════════════════════ */}
        {/* WELCOME                                           */}
        {/* ══════════════════════════════════════════════════ */}
        {step === "welcome" && (
          <div className="space-y-5 text-center">
            <div className="mx-auto flex size-14 items-center justify-center rounded-2xl bg-primary/10">
              <Sparkles className="size-7 text-primary" />
            </div>
            <div>
              <h1 className="text-2xl font-bold">Welcome to PeerClaw</h1>
              <p className="mt-1.5 text-sm text-muted-foreground">
                Decentralized P2P AI agent network. Let's set up your node.
              </p>
            </div>
            <Button size="lg" className="w-full gap-2" onClick={() => setStep("inference")}>
              Get started <ChevronRight className="size-4" />
            </Button>
            <button type="button" className="text-xs text-muted-foreground hover:text-foreground" onClick={finish}>
              Skip setup
            </button>
          </div>
        )}

        {/* ══════════════════════════════════════════════════ */}
        {/* INFERENCE                                         */}
        {/* ══════════════════════════════════════════════════ */}
        {step === "inference" && (
          <div className="space-y-4">
            <div>
              <h2 className="text-xl font-bold">Configure AI Models</h2>
              <p className="mt-1 text-sm text-muted-foreground">
                Choose how PeerClaw runs inference. Enable one or more backends.
              </p>
            </div>

            {/* Local GGUF — default */}
            <ProviderCard
              checked={useLocalGguf}
              onChange={setUseLocalGguf}
              icon={<HardDrive className="size-4" />}
              title="Local GGUF models (recommended)"
              desc="Download and run GGUF model files directly. No extra software needed."
              recommended
            >
              {settings?.models_directory && (
                <p className="mt-2 pl-7 text-[10px] text-muted-foreground">
                  Models directory: <code>{settings.models_directory}</code>
                </p>
              )}
            </ProviderCard>

            {/* Ollama */}
            <ProviderCard
              checked={useOllama}
              onChange={(v) => { setUseOllama(v); if (v) void refreshOllama() }}
              icon={<Monitor className="size-4" />}
              title="Ollama"
              desc="Run models via Ollama. Manages downloads and GPU acceleration for you."
            >
              <div className="mt-3 space-y-2 pl-7">
                <div>
                  <Label className="text-xs">Ollama URL</Label>
                  <Input
                    className="mt-1 h-8 text-xs"
                    value={ollamaUrl}
                    onChange={(e) => setOllamaUrl(e.target.value)}
                    onBlur={() => void refreshOllama()}
                  />
                </div>
                <a
                  href="https://ollama.com/download"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
                >
                  Download Ollama <ExternalLink className="size-3" />
                </a>
                <span className="mx-2 text-[10px] text-muted-foreground">|</span>
                <a
                  href="https://ollama.com/library"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
                >
                  Browse models <ExternalLink className="size-3" />
                </a>
                {ollamaModels.length > 0 && (
                  <div className="mt-2">
                    <p className="mb-1 text-[10px] font-medium text-muted-foreground">
                      {ollamaModels.length} model(s) in Ollama:
                    </p>
                    <div className="max-h-28 space-y-0.5 overflow-y-auto rounded-md border border-border/40 p-1">
                      {ollamaModels.map((m) => (
                        <div key={m.name} className="flex items-center justify-between rounded px-2 py-1 text-xs">
                          <span className="font-mono">{m.name}</span>
                          <span className="text-[10px] text-muted-foreground">
                            {formatSize(m.size)}
                            {m.details?.parameter_size ? ` · ${m.details.parameter_size}` : ""}
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
                {useOllama && (
                  <div className="mt-2 space-y-1.5">
                    <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Pull a model</p>
                    <div className="flex gap-2">
                      <Input
                        className="h-8 flex-1 font-mono text-[11px]"
                        placeholder="e.g. llama3.2, mistral, gemma2"
                        value={ollamaPullName}
                        onChange={(e) => setOllamaPullName(e.target.value)}
                        onKeyDown={(e) => { if (e.key === "Enter") void handleOllamaPull() }}
                      />
                      <Button
                        size="sm" variant="outline" className="h-8 gap-1 text-xs"
                        disabled={ollamaPulling || !ollamaPullName.trim()}
                        onClick={() => void handleOllamaPull()}
                      >
                        {ollamaPulling ? <Loader2 className="size-3 animate-spin" /> : <Download className="size-3" />}
                        {ollamaPulling ? "Pulling…" : "Pull"}
                      </Button>
                    </div>
                    {ollamaPullMsg && (
                      <p className={cn("text-[10px]", ollamaPullMsg.toLowerCase().includes("error") || ollamaPullMsg.toLowerCase().includes("fail") ? "text-destructive" : "text-emerald-500")}>
                        {ollamaPullMsg}
                      </p>
                    )}
                    {ollamaModels.length === 0 && !ollamaPulling && !ollamaPullMsg && (
                      <p className="text-[10px] text-amber-500">
                        No models found. Pull one above or run <code>ollama pull llama3.2</code> in terminal.
                      </p>
                    )}
                  </div>
                )}
              </div>
            </ProviderCard>

            {/* Remote API */}
            <ProviderCard
              checked={useRemoteApi}
              onChange={setUseRemoteApi}
              icon={<Cloud className="size-4" />}
              title="Remote API (OpenAI-compatible)"
              desc="OpenAI, Anthropic, Groq, Together, OpenRouter, or any compatible endpoint."
            >
              <div className="mt-3 space-y-2 pl-7">
                <div>
                  <Label className="text-xs">Base URL</Label>
                  <Input className="mt-1 h-8 text-xs" value={remoteApiUrl} onChange={(e) => setRemoteApiUrl(e.target.value)} placeholder="https://api.openai.com/v1" />
                </div>
                <div>
                  <Label className="text-xs">Model</Label>
                  <Input className="mt-1 h-8 text-xs" value={remoteApiModel} onChange={(e) => setRemoteApiModel(e.target.value)} placeholder="gpt-4o-mini" />
                </div>
                <div>
                  <Label className="text-xs">API Key</Label>
                  <Input type="password" className="mt-1 h-8 text-xs" value={remoteApiKey} onChange={(e) => setRemoteApiKey(e.target.value)} placeholder="sk-..." />
                </div>
              </div>
            </ProviderCard>

            {error && <p className="text-sm text-destructive">{error}</p>}
            <div className="flex gap-2">
              <Button variant="outline" onClick={() => setStep("welcome")}>Back</Button>
              <Button className="flex-1 gap-2" disabled={saving} onClick={() => void saveInference()}>
                {saving && <Loader2 className="size-3.5 animate-spin" />} Save & continue
              </Button>
            </div>
          </div>
        )}

        {/* ══════════════════════════════════════════════════ */}
        {/* MODELS                                            */}
        {/* ══════════════════════════════════════════════════ */}
        {step === "models" && (
          <div className="space-y-4">
            <div>
              <h2 className="text-xl font-bold">Models</h2>
              <p className="mt-1 text-sm text-muted-foreground">
                {models.length > 0
                  ? `${models.length} model(s) available. Download more or continue.`
                  : "No models detected. Download a GGUF model below or pull one in Ollama."}
              </p>
            </div>

            {models.length > 0 && (
              <div className="max-h-28 space-y-0.5 overflow-y-auto rounded-xl border border-border p-1.5">
                {models.map((m) => (
                  <div key={m} className="flex items-center gap-2 rounded-lg bg-emerald-500/5 px-3 py-1.5 text-xs">
                    <StatusIcon ok />
                    <span className="font-mono">{m}</span>
                  </div>
                ))}
              </div>
            )}

            {/* GGUF downloads */}
            {ggufPresets.length > 0 && (
              <div>
                <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                  Download GGUF from HuggingFace
                </h3>
                <div className="max-h-48 space-y-1.5 overflow-y-auto">
                  {ggufPresets.map((p) => (
                      <div key={p.id} className="flex items-center gap-3 rounded-lg border border-border/60 px-3 py-2">
                        <div className="min-w-0 flex-1">
                          <p className="flex items-center gap-2 text-sm font-medium">
                            {p.label || p.id}
                            {p.recommended && (
                              <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[9px] font-semibold text-primary">Recommended</span>
                            )}
                          </p>
                          <p className="text-[11px] text-muted-foreground">
                            {p.size ?? ""}{p.desc ? ` — ${p.desc}` : ""}
                          </p>
                        </div>
                        <Button
                          size="sm" variant="outline" className="h-7 gap-1 text-xs"
                          disabled={downloading !== null}
                          onClick={() => void handleDownload(p.id)}
                        >
                          {downloading === p.id ? <Loader2 className="size-3 animate-spin" /> : <Download className="size-3" />}
                          {downloading === p.id
                            ? (downloadPct != null ? `${downloadPct}%` : "Starting…")
                            : "Download"}
                        </Button>
                      </div>
                  ))}
                </div>
                {/* Custom URL */}
                <div className="mt-3 space-y-1.5">
                  <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Or paste a .gguf URL</p>
                  <div className="flex gap-2">
                    <Input
                      className="h-8 flex-1 font-mono text-[11px]"
                      placeholder="https://huggingface.co/…/resolve/main/model.gguf"
                      value={customUrl}
                      onChange={(e) => setCustomUrl(e.target.value)}
                    />
                    <Button
                      size="sm" variant="outline" className="h-8 gap-1 text-xs"
                      disabled={downloading !== null || !customUrl.trim()}
                      onClick={() => void handleUrlDownload()}
                    >
                      {downloading === "custom" ? <Loader2 className="size-3 animate-spin" /> : <Download className="size-3" />}
                      {downloading === "custom" ? (downloadPct != null ? `${downloadPct}%` : "…") : "Download"}
                    </Button>
                  </div>
                </div>

                {downloading && downloadPct != null && (
                  <div className="mt-2">
                    <div className="h-2 overflow-hidden rounded-full bg-muted">
                      <div
                        className="h-full rounded-full bg-primary transition-all duration-300"
                        style={{ width: `${Math.min(downloadPct, 100)}%` }}
                      />
                    </div>
                    <p className="mt-1 text-[10px] text-muted-foreground">{downloadPct}% downloaded</p>
                  </div>
                )}
                {downloadMsg && (
                  <p className={cn("mt-2 text-xs", downloadMsg.toLowerCase().includes("fail") || downloadMsg.toLowerCase().includes("error") ? "text-destructive" : "text-emerald-500")}>
                    {downloadMsg}
                  </p>
                )}
              </div>
            )}

            <div className="flex gap-2">
              <Button variant="outline" onClick={() => setStep("inference")}>Back</Button>
              <Button className="flex-1 gap-2" onClick={() => setStep("mcp")}>
                Continue <ChevronRight className="size-4" />
              </Button>
            </div>
          </div>
        )}

        {/* ══════════════════════════════════════════════════ */}
        {/* MCP                                               */}
        {/* ══════════════════════════════════════════════════ */}
        {step === "mcp" && (
          <div className="space-y-4">
            <div>
              <h2 className="text-xl font-bold">MCP Servers (optional)</h2>
              <p className="mt-1 text-sm text-muted-foreground">
                Connect MCP servers to give agents external tools — file access, web search, databases, and more.
              </p>
            </div>

            <label className="flex cursor-pointer items-center gap-2 text-sm">
              <input type="checkbox" className="size-4 accent-primary" checked={mcpEnabled} onChange={(e) => setMcpEnabled(e.target.checked)} />
              <span className="font-medium">Enable MCP</span>
            </label>

            <div>
              <p className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">Quick add</p>
              <div className="flex flex-wrap gap-1.5">
                {MCP_PRESETS.map((p) => (
                  <Button
                    key={p.name} variant="outline" size="sm" className="h-7 gap-1 text-xs"
                    disabled={mcpServers.some((s) => s.name === p.name)}
                    onClick={() => addMcpPreset(p)}
                  >
                    <Plus className="size-3" /> {p.label}
                  </Button>
                ))}
              </div>
            </div>

            {mcpServers.length > 0 && (
              <div className="space-y-1.5">
                {mcpServers.map((s) => {
                  const preset = MCP_PRESETS.find((p) => p.name === s.name)
                  return (
                    <div key={s.name} className="flex items-center gap-2 rounded-lg border border-border/60 px-3 py-2">
                      <Plug className="size-4 shrink-0 text-primary" />
                      <div className="min-w-0 flex-1">
                        <p className="text-sm font-medium">{s.name}</p>
                        <p className="truncate text-[10px] text-muted-foreground">
                          {s.command} {(s.args ?? []).join(" ")}{preset?.desc ? ` — ${preset.desc}` : ""}
                        </p>
                      </div>
                      <button type="button" className="shrink-0 text-muted-foreground hover:text-destructive" onClick={() => setMcpServers(mcpServers.filter((x) => x.name !== s.name))}>
                        <X className="size-4" />
                      </button>
                    </div>
                  )
                })}
              </div>
            )}

            <div className="flex gap-2">
              <Button variant="outline" onClick={() => setStep("models")}>Back</Button>
              <Button className="flex-1 gap-2" disabled={mcpSaving} onClick={() => void saveMcp()}>
                {mcpSaving && <Loader2 className="size-3.5 animate-spin" />}
                {mcpServers.length > 0 ? "Save & finish" : "Skip & finish"}
              </Button>
            </div>
          </div>
        )}

        {/* ══════════════════════════════════════════════════ */}
        {/* DONE                                              */}
        {/* ══════════════════════════════════════════════════ */}
        {step === "done" && (
          <div className="space-y-5 text-center">
            <div className="mx-auto flex size-14 items-center justify-center rounded-2xl bg-emerald-500/10">
              <Check className="size-7 text-emerald-500" />
            </div>
            <div>
              <h2 className="text-xl font-bold">You're all set!</h2>
              <p className="mt-1.5 text-sm text-muted-foreground">
                PeerClaw is configured. Change settings anytime from the sidebar.
              </p>
            </div>
            <div className="grid grid-cols-2 gap-2 text-left text-xs">
              <StatCard label="Models" value={models.length > 0 ? `${models.length} available` : "None yet"} ok={models.length > 0} />
              <StatCard label="Local GGUF" value={useLocalGguf ? "Enabled" : "Off"} ok={useLocalGguf} />
              <StatCard label="Ollama" value={useOllama ? `${ollamaModels.length} models` : "Off"} ok={useOllama && ollamaModels.length > 0} />
              <StatCard label="Remote API" value={useRemoteApi ? "Configured" : "Off"} ok={useRemoteApi} />
              <StatCard label="MCP" value={mcpEnabled ? `${mcpServers.length} server(s)` : "Off"} ok={mcpEnabled && mcpServers.length > 0} />
            </div>
            <Button size="lg" className="w-full gap-2" onClick={finish}>
              Open PeerClaw <ChevronRight className="size-4" />
            </Button>
            <div className="flex justify-center gap-3">
              <button type="button" className="text-xs text-muted-foreground hover:text-foreground" onClick={() => setStep("inference")}>
                Change AI setup
              </button>
              <span className="text-xs text-muted-foreground/40">|</span>
              <button type="button" className="text-xs text-muted-foreground hover:text-foreground" onClick={() => setStep("models")}>
                Models
              </button>
              <span className="text-xs text-muted-foreground/40">|</span>
              <button type="button" className="text-xs text-muted-foreground hover:text-foreground" onClick={() => setStep("mcp")}>
                MCP
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

/* ── Sub-components ─────────────────────────────────────────────── */

function ProviderCard({
  checked, onChange, icon, title, desc, recommended, children,
}: {
  checked: boolean; onChange: (v: boolean) => void
  icon: React.ReactNode; title: string; desc: string
  recommended?: boolean; children?: React.ReactNode
}) {
  return (
    <div className={cn("rounded-xl border p-3.5 transition-colors", checked ? "border-primary/40 bg-primary/5" : "border-border")}>
      <label className="flex cursor-pointer items-start gap-3">
        <input type="checkbox" className="mt-0.5 size-4 accent-primary" checked={checked} onChange={(e) => onChange(e.target.checked)} />
        <div className="flex-1">
          <div className="flex items-center gap-2 text-sm font-medium">
            {icon} {title}
            {recommended && <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[9px] font-semibold text-primary">Recommended</span>}
          </div>
          <p className="mt-0.5 text-[11px] text-muted-foreground">{desc}</p>
        </div>
      </label>
      {checked && children}
    </div>
  )
}

function StatCard({ label, value, ok }: { label: string; value: string; ok: boolean }) {
  return (
    <div className="rounded-lg bg-muted/20 px-3 py-2">
      <p className="text-[10px] uppercase tracking-wider text-muted-foreground">{label}</p>
      <p className={cn("mt-0.5 font-medium", ok ? "text-emerald-500" : "text-muted-foreground")}>{value}</p>
    </div>
  )
}
