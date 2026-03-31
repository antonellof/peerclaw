import { useCallback, useEffect, useState } from "react"
import {
  Bot,
  Check,
  ChevronRight,
  Cloud,
  Download,
  HardDrive,
  Loader2,
  Monitor,
  Plug,
  Plus,
  Sparkles,
  Trash2,
  X,
} from "lucide-react"

import {
  downloadGgufModel,
  fetchInferenceSettings,
  fetchMcpStatus,
  fetchOnboarding,
  fetchOpenAiModels,
  putInferenceSettings,
  putMcpConfig,
  type GgufPresetRow,
  type InferenceSettingsResponse,
  type McpConfigJson,
  type OnboardingStep,
} from "@/lib/api"
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

/** Popular GGUF models with sizes for the download UI. */
const MODEL_INFO: Record<string, { label: string; size: string; desc: string }> = {
  "llama-3.2-1b": { label: "Llama 3.2 1B", size: "~770 MB", desc: "Fast, good for testing" },
  "llama-3.2-3b": { label: "Llama 3.2 3B", size: "~2 GB", desc: "Great balance of speed and quality" },
  "phi-3-mini": { label: "Phi-3 Mini 4K", size: "~2.4 GB", desc: "Microsoft, strong reasoning" },
  "qwen2.5-0.5b": { label: "Qwen 2.5 0.5B", size: "~400 MB", desc: "Tiny, very fast" },
  "qwen2.5-1.5b": { label: "Qwen 2.5 1.5B", size: "~1 GB", desc: "Small and capable" },
  "qwen2.5-3b": { label: "Qwen 2.5 3B", size: "~2 GB", desc: "Multilingual, strong coding" },
  "gemma-2-2b": { label: "Gemma 2 2B", size: "~1.6 GB", desc: "Google, efficient" },
  "tinyllama-1.1b": { label: "TinyLlama 1.1B", size: "~640 MB", desc: "Ultra-light, fast" },
}

/** Common MCP server presets. */
const MCP_PRESETS = [
  { name: "filesystem", label: "Filesystem", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"], desc: "Read/write files" },
  { name: "brave-search", label: "Brave Search", command: "npx", args: ["-y", "@anthropic/mcp-server-brave-search"], desc: "Web search (needs BRAVE_API_KEY)" },
  { name: "github", label: "GitHub", command: "npx", args: ["-y", "@modelcontextprotocol/server-github"], desc: "GitHub repos/issues (needs GITHUB_TOKEN)" },
  { name: "sqlite", label: "SQLite", command: "npx", args: ["-y", "@anthropic/mcp-server-sqlite", "/tmp/data.db"], desc: "Query SQLite databases" },
]

export function SetupPage({ onFinish }: { onFinish: () => void }) {
  const [step, setStep] = useState<Step>("welcome")
  const [settings, setSettings] = useState<InferenceSettingsResponse | null>(null)
  const [onboarding, setOnboarding] = useState<OnboardingStep[]>([])
  const [models, setModels] = useState<string[]>([])
  const [ggufPresets, setGgufPresets] = useState<GgufPresetRow[]>([])
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Inference form
  const [useOllama, setUseOllama] = useState(true)
  const [ollamaUrl, setOllamaUrl] = useState("http://localhost:11434")
  const [useLocalGguf, setUseLocalGguf] = useState(false)
  const [useRemoteApi, setUseRemoteApi] = useState(false)
  const [remoteApiUrl, setRemoteApiUrl] = useState("")
  const [remoteApiModel, setRemoteApiModel] = useState("")
  const [remoteApiKey, setRemoteApiKey] = useState("")

  // Model downloads
  const [downloading, setDownloading] = useState<string | null>(null)
  const [downloadMsg, setDownloadMsg] = useState<string | null>(null)

  // MCP
  const [mcpEnabled, setMcpEnabled] = useState(false)
  const [mcpServers, setMcpServers] = useState<McpConfigJson["servers"]>([])
  const [mcpSaving, setMcpSaving] = useState(false)

  const loadAll = useCallback(async () => {
    try {
      const s = await fetchInferenceSettings()
      setSettings(s)
      setUseOllama(s.use_ollama)
      setOllamaUrl(s.ollama_url || "http://localhost:11434")
      setUseLocalGguf(s.use_local_gguf)
      setUseRemoteApi(s.remote_api_enabled)
      setRemoteApiUrl(s.remote_api_base_url)
      setRemoteApiModel(s.remote_api_model)
      setGgufPresets(s.gguf_presets || [])
    } catch { /* defaults */ }
    try {
      const o = await fetchOnboarding()
      setOnboarding(o.steps)
    } catch { /* ignore */ }
    try {
      const m = await fetchOpenAiModels()
      setModels(m.map((x) => x.id))
    } catch { setModels([]) }
    try {
      const mcp = await fetchMcpStatus()
      setMcpEnabled(mcp.config.enabled)
      setMcpServers(mcp.config.servers || [])
    } catch { /* ignore */ }
  }, [])

  useEffect(() => { void loadAll() }, [loadAll])

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
      setStep("models")
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to save")
    } finally { setSaving(false) }
  }

  const handleDownload = async (presetId: string) => {
    setDownloading(presetId)
    setDownloadMsg(null)
    try {
      const r = await downloadGgufModel({ preset: presetId, quant: "Q4_K_M" })
      if (r.success) {
        setDownloadMsg(`Downloaded ${presetId} (${r.bytes ? (r.bytes / 1e6).toFixed(0) + " MB" : "done"})`)
        const m = await fetchOpenAiModels()
        setModels(m.map((x) => x.id))
      } else {
        setDownloadMsg(r.error ?? "Download failed")
      }
    } catch (e) {
      setDownloadMsg(e instanceof Error ? e.message : "Download error")
    } finally { setDownloading(null) }
  }

  const saveMcp = async () => {
    setMcpSaving(true)
    try {
      await putMcpConfig({ enabled: mcpEnabled, timeout_secs: 30, auto_reconnect: true, servers: mcpServers })
    } catch { /* ignore */ }
    setMcpSaving(false)
    setStep("done")
  }

  const addMcpPreset = (preset: typeof MCP_PRESETS[number]) => {
    if (mcpServers.some((s) => s.name === preset.name)) return
    setMcpServers([...mcpServers, { name: preset.name, url: "", command: preset.command, args: preset.args }])
    setMcpEnabled(true)
  }

  const removeMcpServer = (name: string) => {
    setMcpServers(mcpServers.filter((s) => s.name !== name))
  }

  const finish = () => { markSetupDone(); onFinish() }

  const stepIdx = STEPS.indexOf(step)

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <div className="w-full max-w-xl space-y-6">
        {/* Progress */}
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

        {/* ── Welcome ──────────────────────────────────── */}
        {step === "welcome" && (
          <div className="space-y-5 text-center">
            <div className="mx-auto flex size-14 items-center justify-center rounded-2xl bg-primary/10">
              <Sparkles className="size-7 text-primary" />
            </div>
            <div>
              <h1 className="text-2xl font-bold">Welcome to PeerClaw</h1>
              <p className="mt-1.5 text-sm text-muted-foreground">
                Decentralized P2P AI agent network. Let's set up your node in a few steps.
              </p>
            </div>
            <div className="space-y-1.5 text-left">
              {onboarding.map((s) => (
                <div key={s.id} className="flex items-center gap-2.5 rounded-lg bg-muted/20 px-3 py-2 text-sm">
                  <span className={s.ok ? "text-emerald-500" : "text-muted-foreground/40"}>
                    {s.ok ? <Check className="size-4" /> : <span className="inline-block size-4 rounded-full border-2" />}
                  </span>
                  <span className={cn("capitalize", s.ok && "text-foreground")}>{s.id.replace(/_/g, " ")}</span>
                </div>
              ))}
            </div>
            <Button size="lg" className="w-full gap-2" onClick={() => setStep("inference")}>
              Get started <ChevronRight className="size-4" />
            </Button>
            <button type="button" className="text-xs text-muted-foreground hover:text-foreground" onClick={finish}>
              Skip setup — I'll configure later
            </button>
          </div>
        )}

        {/* ── Inference ────────────────────────────────── */}
        {step === "inference" && (
          <div className="space-y-4">
            <div>
              <h2 className="text-xl font-bold">Configure AI Models</h2>
              <p className="mt-1 text-sm text-muted-foreground">
                Choose one or more inference backends. You can change these anytime in Settings.
              </p>
            </div>

            {/* Ollama */}
            <ProviderCard
              checked={useOllama}
              onChange={setUseOllama}
              icon={<Monitor className="size-4" />}
              title="Ollama (local)"
              desc="Run open-source models locally. Install from ollama.com."
            >
              <div className="mt-3 pl-7">
                <Label className="text-xs">Ollama URL</Label>
                <Input className="mt-1 h-8 text-xs" value={ollamaUrl} onChange={(e) => setOllamaUrl(e.target.value)} />
                <p className="mt-1 text-[10px] text-muted-foreground">
                  Docker: <code>http://host.docker.internal:11434</code>
                </p>
              </div>
            </ProviderCard>

            {/* Local GGUF */}
            <ProviderCard
              checked={useLocalGguf}
              onChange={setUseLocalGguf}
              icon={<HardDrive className="size-4" />}
              title="Local GGUF models"
              desc="Download and run GGUF files directly (no Ollama needed)."
            >
              {settings?.models_directory && (
                <p className="mt-2 pl-7 text-[10px] text-muted-foreground">
                  Models dir: <code>{settings.models_directory}</code>
                </p>
              )}
            </ProviderCard>

            {/* Remote API */}
            <ProviderCard
              checked={useRemoteApi}
              onChange={setUseRemoteApi}
              icon={<Cloud className="size-4" />}
              title="Remote API (OpenAI-compatible)"
              desc="OpenAI, Anthropic, Groq, Together, OpenRouter, etc."
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

        {/* ── Models ───────────────────────────────────── */}
        {step === "models" && (
          <div className="space-y-4">
            <div>
              <h2 className="text-xl font-bold">Models</h2>
              <p className="mt-1 text-sm text-muted-foreground">
                {models.length > 0
                  ? `${models.length} model(s) available. Download more or continue.`
                  : "No models detected yet. Download a GGUF model or pull one in Ollama."}
              </p>
            </div>

            {/* Detected models */}
            {models.length > 0 && (
              <div className="max-h-32 space-y-1 overflow-y-auto rounded-xl border border-border p-2">
                {models.map((m) => (
                  <div key={m} className="flex items-center gap-2 rounded-lg bg-emerald-500/5 px-3 py-1.5 text-xs">
                    <Check className="size-3.5 text-emerald-500" />
                    <span className="font-mono">{m}</span>
                  </div>
                ))}
              </div>
            )}

            {/* Download GGUF models */}
            <div>
              <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                Download from HuggingFace
              </h3>
              <div className="space-y-1.5">
                {ggufPresets.map((p) => {
                  const info = MODEL_INFO[p.id]
                  return (
                    <div key={p.id} className="flex items-center gap-3 rounded-lg border border-border/60 px-3 py-2">
                      <div className="min-w-0 flex-1">
                        <p className="text-sm font-medium">{info?.label ?? p.id}</p>
                        <p className="text-[11px] text-muted-foreground">
                          {info?.size ?? ""} {info?.desc ? `— ${info.desc}` : ""} <span className="opacity-50">{p.repo}</span>
                        </p>
                      </div>
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-7 gap-1 text-xs"
                        disabled={downloading !== null}
                        onClick={() => void handleDownload(p.id)}
                      >
                        {downloading === p.id ? <Loader2 className="size-3 animate-spin" /> : <Download className="size-3" />}
                        {downloading === p.id ? "Downloading…" : "Download"}
                      </Button>
                    </div>
                  )
                })}
              </div>
              {downloadMsg && (
                <p className={cn("mt-2 text-xs", downloadMsg.includes("fail") || downloadMsg.includes("error") ? "text-destructive" : "text-emerald-500")}>
                  {downloadMsg}
                </p>
              )}
            </div>

            <div className="flex gap-2">
              <Button variant="outline" onClick={() => setStep("inference")}>Back</Button>
              <Button className="flex-1 gap-2" onClick={() => setStep("mcp")}>
                Continue <ChevronRight className="size-4" />
              </Button>
            </div>
          </div>
        )}

        {/* ── MCP ──────────────────────────────────────── */}
        {step === "mcp" && (
          <div className="space-y-4">
            <div>
              <h2 className="text-xl font-bold">MCP Servers (optional)</h2>
              <p className="mt-1 text-sm text-muted-foreground">
                Connect Model Context Protocol servers to give agents external tools — file access, web search, databases, etc.
              </p>
            </div>

            <label className="flex cursor-pointer items-center gap-2 text-sm">
              <input type="checkbox" className="size-4 accent-primary" checked={mcpEnabled} onChange={(e) => setMcpEnabled(e.target.checked)} />
              <span className="font-medium">Enable MCP</span>
            </label>

            {/* Quick-add presets */}
            <div>
              <p className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">Quick add</p>
              <div className="flex flex-wrap gap-1.5">
                {MCP_PRESETS.map((p) => (
                  <Button
                    key={p.name}
                    variant="outline"
                    size="sm"
                    className="h-7 gap-1 text-xs"
                    disabled={mcpServers.some((s) => s.name === p.name)}
                    onClick={() => addMcpPreset(p)}
                  >
                    <Plus className="size-3" />
                    {p.label}
                  </Button>
                ))}
              </div>
            </div>

            {/* Configured servers */}
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
                          {s.command} {(s.args ?? []).join(" ")} {preset?.desc ? `— ${preset.desc}` : ""}
                        </p>
                      </div>
                      <button
                        type="button"
                        className="shrink-0 text-muted-foreground hover:text-destructive"
                        onClick={() => removeMcpServer(s.name)}
                      >
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

        {/* ── Done ─────────────────────────────────────── */}
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
              <StatCard label="Ollama" value={useOllama ? "Connected" : "Off"} ok={useOllama} />
              <StatCard label="Remote API" value={useRemoteApi ? "Configured" : "Off"} ok={useRemoteApi} />
              <StatCard label="MCP" value={mcpEnabled ? `${mcpServers.length} server(s)` : "Off"} ok={mcpEnabled && mcpServers.length > 0} />
            </div>
            <Button size="lg" className="w-full gap-2" onClick={finish}>
              Open PeerClaw <ChevronRight className="size-4" />
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}

/* ── Shared sub-components ──────────────────────────────────────── */

function ProviderCard({
  checked, onChange, icon, title, desc, children,
}: {
  checked: boolean; onChange: (v: boolean) => void
  icon: React.ReactNode; title: string; desc: string
  children?: React.ReactNode
}) {
  return (
    <div className={cn("rounded-xl border p-3.5 transition-colors", checked ? "border-primary/40 bg-primary/5" : "border-border")}>
      <label className="flex cursor-pointer items-start gap-3">
        <input type="checkbox" className="mt-0.5 size-4 accent-primary" checked={checked} onChange={(e) => onChange(e.target.checked)} />
        <div className="flex-1">
          <div className="flex items-center gap-2 text-sm font-medium">{icon}{title}</div>
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
