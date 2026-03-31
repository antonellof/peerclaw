import { useCallback, useEffect, useState } from "react"
import {
  Check,
  ChevronRight,
  Cloud,
  Download,
  ExternalLink,
  HardDrive,
  Loader2,
  Monitor,
  Terminal,
} from "lucide-react"

import { DEFAULT_MODEL } from "@/lib/defaults"
import {
  downloadGgufModel,
  fetchInferenceSettings,
  fetchOllamaModels,
  fetchOpenAiModels,
  putInferenceSettings,
  type InferenceSettingsResponse,
  type InferenceSettingsPut,
  type OllamaModel,
} from "@/lib/api"
import { useControlWebSocket } from "@/hooks/useControlWebSocket"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { SLASH_COMMANDS } from "@/pages/chat/slashCommands"
import { cn } from "@/lib/utils"
import type { WorkspaceChatPreferences } from "@/workspace/workspacePreferences"
import type { WorkspaceView } from "@/workspace/views"

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  chatPreferences: WorkspaceChatPreferences
  setChatPreferences: (u: Partial<WorkspaceChatPreferences>) => void
  onNavigate: (view: WorkspaceView, hash?: string) => void
  onModelsChanged?: () => void
}

function formatSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(0)} MB`
  return `${bytes} B`
}

export function WorkspaceSettingsDialog({
  open,
  onOpenChange,
  chatPreferences,
  setChatPreferences,
  onNavigate,
  onModelsChanged,
}: Props) {
  const [models, setModels] = useState<string[]>([])
  const [inf, setInf] = useState<InferenceSettingsResponse | null>(null)
  const [infErr, setInfErr] = useState<string | null>(null)
  const [infSaving, setInfSaving] = useState(false)
  const [remoteKeyDraft, setRemoteKeyDraft] = useState("")
  const [ollamaModels, setOllamaModels] = useState<OllamaModel[]>([])

  // Downloads
  const [dlBusy, setDlBusy] = useState(false)
  const [dlMsg, setDlMsg] = useState<string | null>(null)
  const [dlPct, setDlPct] = useState<number | null>(null)

  useControlWebSocket({
    onDownloadProgress: (ev) => {
      if (ev.percent != null) setDlPct(ev.percent)
    },
  })

  const loadInf = useCallback(async () => {
    setInfErr(null)
    try {
      setInf(await fetchInferenceSettings())
      setRemoteKeyDraft("")
    } catch (e) {
      setInf(null)
      setInfErr(e instanceof Error ? e.message : "Inference API unavailable.")
    }
  }, [])

  const loadModels = useCallback(async () => {
    try {
      const list = await fetchOpenAiModels()
      const ids = list.map((m) => m.id).filter(Boolean)
      setModels(ids.length ? ids : [DEFAULT_MODEL])
    } catch {
      setModels([DEFAULT_MODEL])
    }
  }, [])

  const refreshOllama = useCallback(async () => {
    try { setOllamaModels(await fetchOllamaModels()) } catch { setOllamaModels([]) }
  }, [])

  useEffect(() => {
    if (open) {
      void loadModels()
      void loadInf()
      void refreshOllama()
    }
  }, [open, loadModels, loadInf, refreshOllama])

  const saveInference = async () => {
    if (!inf) return
    setInfSaving(true)
    setInfErr(null)
    try {
      const body: InferenceSettingsPut = {
        use_local_gguf: inf.use_local_gguf,
        use_ollama: inf.use_ollama,
        ollama_url: inf.ollama_url,
        remote_api_enabled: inf.remote_api_enabled,
        remote_api_base_url: inf.remote_api_base_url,
        remote_api_model: inf.remote_api_model,
      }
      if (remoteKeyDraft.trim()) body.remote_api_key = remoteKeyDraft.trim()
      const next = await putInferenceSettings(body)
      setInf(next)
      setRemoteKeyDraft("")
      void loadModels()
      void refreshOllama()
      onModelsChanged?.()
    } catch (e) {
      setInfErr(e instanceof Error ? e.message : "Save failed")
    } finally {
      setInfSaving(false)
    }
  }

  const handleDownload = async (presetId: string) => {
    setDlBusy(true)
    setDlMsg(null)
    setDlPct(0)
    try {
      const r = await downloadGgufModel({ preset: presetId, quant: "Q4_K_M" })
      if (r.success) setDlMsg(`Downloaded ${presetId}`)
      else setDlMsg(r.error ?? "Download failed")
    } catch (e) {
      setDlMsg(e instanceof Error ? e.message : "Download failed")
    } finally {
      setDlBusy(false)
      setDlPct(null)
      void loadInf()
      void loadModels()
      onModelsChanged?.()
    }
  }

  const go = (v: WorkspaceView, hash?: string) => {
    onNavigate(v, hash)
    onOpenChange(false)
  }

  const byCat = SLASH_COMMANDS.reduce<Record<string, typeof SLASH_COMMANDS>>((acc, c) => {
    ;(acc[c.category] ??= []).push(c)
    return acc
  }, {})

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[min(90dvh,720px)] w-[min(100%,560px)] max-w-[calc(100vw-1.5rem)] flex-col gap-0 overflow-hidden p-0">
        <DialogHeader className="shrink-0 border-b border-border px-5 py-4 pr-12 text-left">
          <DialogTitle className="text-base">Settings</DialogTitle>
          <DialogDescription className="text-xs">
            Configure AI models, chat preferences, and workspace.
          </DialogDescription>
        </DialogHeader>

        <Tabs defaultValue="inference" className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="shrink-0 border-b border-border px-4 pt-1">
            <TabsList className="h-auto w-full flex-wrap justify-start gap-1 bg-transparent p-0 pb-2">
              <TabsTrigger value="inference" className="text-xs">Models</TabsTrigger>
              <TabsTrigger value="chat" className="text-xs">Chat</TabsTrigger>
              <TabsTrigger value="workspace" className="text-xs">Workspace</TabsTrigger>
              <TabsTrigger value="reference" className="text-xs">Commands</TabsTrigger>
            </TabsList>
          </div>

          {/* ══ Models / Inference ══════════════════════════════ */}
          <TabsContent value="inference" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-3 px-5 py-4 pr-3">
                {infErr && (
                  <p className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">{infErr}</p>
                )}
                {inf && (
                  <>
                    {/* Local GGUF */}
                    <ProviderCard
                      checked={inf.use_local_gguf}
                      onChange={(v) => setInf({ ...inf, use_local_gguf: v })}
                      icon={<HardDrive className="size-4" />}
                      title="Local GGUF"
                      desc={inf.models_directory ? `Models: ${inf.models_directory}` : "Direct GGUF model files"}
                      recommended
                    />

                    {/* Ollama */}
                    <ProviderCard
                      checked={inf.use_ollama}
                      onChange={(v) => { setInf({ ...inf, use_ollama: v }); if (v) void refreshOllama() }}
                      icon={<Monitor className="size-4" />}
                      title="Ollama"
                      desc="Local model server with GPU acceleration"
                    >
                      <div className="mt-2 space-y-2 pl-7">
                        <div>
                          <Label className="text-xs">URL</Label>
                          <Input className="mt-1 h-8 text-xs" value={inf.ollama_url} onChange={(e) => setInf({ ...inf, ollama_url: e.target.value })} />
                        </div>
                        <div className="flex gap-3">
                          <a href="https://ollama.com/download" target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-xs text-primary hover:underline">
                            Install <ExternalLink className="size-3" />
                          </a>
                          <a href="https://ollama.com/library" target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-xs text-primary hover:underline">
                            Models <ExternalLink className="size-3" />
                          </a>
                        </div>
                        {ollamaModels.length > 0 && (
                          <div className="max-h-24 space-y-0.5 overflow-y-auto rounded-md border border-border/40 p-1">
                            {ollamaModels.map((m) => (
                              <div key={m.name} className="flex items-center justify-between rounded px-2 py-1 text-[11px]">
                                <span className="font-mono">{m.name}</span>
                                <span className="text-[10px] text-muted-foreground">{formatSize(m.size)}</span>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    </ProviderCard>

                    {/* Remote API */}
                    <ProviderCard
                      checked={inf.remote_api_enabled}
                      onChange={(v) => setInf({ ...inf, remote_api_enabled: v })}
                      icon={<Cloud className="size-4" />}
                      title="Remote API"
                      desc="OpenAI, Anthropic, Groq, Together, etc."
                    >
                      <div className="mt-2 space-y-2 pl-7">
                        <div>
                          <Label className="text-xs">Base URL</Label>
                          <Input className="mt-1 h-8 text-xs" value={inf.remote_api_base_url} onChange={(e) => setInf({ ...inf, remote_api_base_url: e.target.value })} placeholder="https://api.openai.com/v1" />
                        </div>
                        <div>
                          <Label className="text-xs">Model</Label>
                          <Input className="mt-1 h-8 text-xs" value={inf.remote_api_model} onChange={(e) => setInf({ ...inf, remote_api_model: e.target.value })} placeholder="gpt-4o-mini" />
                        </div>
                        <div>
                          <Label className="text-xs">API Key {inf.api_key_configured && <span className="text-muted-foreground">(configured)</span>}</Label>
                          <Input type="password" className="mt-1 h-8 text-xs" value={remoteKeyDraft} onChange={(e) => setRemoteKeyDraft(e.target.value)} placeholder={inf.api_key_configured ? "••••••••" : "sk-…"} />
                        </div>
                      </div>
                    </ProviderCard>

                    <Button size="sm" className="w-full" disabled={infSaving} onClick={() => void saveInference()}>
                      {infSaving ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : null}
                      Save
                    </Button>

                    {/* Download models */}
                    {inf.gguf_presets.length > 0 && (
                      <div className="space-y-2 border-t border-border pt-3">
                        <p className="text-xs font-semibold">Download GGUF Models</p>
                        <div className="space-y-1.5">
                          {inf.gguf_presets.map((p) => (
                            <div key={p.id} className="flex items-center gap-2 rounded-lg border border-border/50 px-3 py-1.5">
                              <div className="min-w-0 flex-1">
                                <p className="flex items-center gap-1.5 text-xs font-medium">
                                  {p.label || p.id}
                                  {p.recommended && <span className="rounded bg-primary/10 px-1 py-0.5 text-[8px] font-semibold text-primary">Rec</span>}
                                </p>
                                <p className="text-[10px] text-muted-foreground">{p.size}{p.desc ? ` — ${p.desc}` : ""}</p>
                              </div>
                              <Button size="sm" variant="outline" className="h-6 gap-1 text-[10px]" disabled={dlBusy} onClick={() => void handleDownload(p.id)}>
                                {dlBusy ? <Loader2 className="size-3 animate-spin" /> : <Download className="size-3" />}
                                {dlBusy ? (dlPct != null ? `${dlPct}%` : "…") : "Get"}
                              </Button>
                            </div>
                          ))}
                        </div>
                        {dlBusy && dlPct != null && (
                          <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                            <div className="h-full rounded-full bg-primary transition-all duration-300" style={{ width: `${Math.min(dlPct, 100)}%` }} />
                          </div>
                        )}
                        {dlMsg && (
                          <p className={cn("text-xs", dlMsg.toLowerCase().includes("fail") || dlMsg.toLowerCase().includes("error") ? "text-destructive" : "text-emerald-500")}>{dlMsg}</p>
                        )}
                      </div>
                    )}
                  </>
                )}
              </div>
            </ScrollArea>
          </TabsContent>

          {/* ══ Chat ════════════════════════════════════════════ */}
          <TabsContent value="chat" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-4 px-5 py-4 pr-3">
                <div className="space-y-2">
                  <Label className="text-xs">Model</Label>
                  <select
                    className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                    value={chatPreferences.model}
                    onChange={(e) => setChatPreferences({ model: e.target.value })}
                  >
                    {models.map((m) => (
                      <option key={m} value={m}>{m}</option>
                    ))}
                  </select>
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <div className="space-y-1.5">
                    <Label className="text-xs">Temperature</Label>
                    <Input type="number" step={0.1} min={0} max={2} className="h-9" value={chatPreferences.temperature} onChange={(e) => setChatPreferences({ temperature: parseFloat(e.target.value) || 0.7 })} />
                  </div>
                  <div className="space-y-1.5">
                    <Label className="text-xs">Max tokens</Label>
                    <Input type="number" min={16} max={32000} className="h-9" value={chatPreferences.maxTokens} onChange={(e) => setChatPreferences({ maxTokens: parseInt(e.target.value, 10) || 500 })} />
                  </div>
                </div>
                <div className="space-y-2">
                  <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Features</p>
                  <FeatureToggle checked={chatPreferences.useAgentic} onChange={(v) => setChatPreferences({ useAgentic: v })} label="Agentic chat (tools)" />
                  <FeatureToggle checked={chatPreferences.useMcp} onChange={(v) => setChatPreferences({ useMcp: v })} label="MCP tools" />
                  <FeatureToggle checked={chatPreferences.distributed} onChange={(v) => setChatPreferences({ distributed: v })} label="P2P distributed inference" />
                </div>
              </div>
            </ScrollArea>
          </TabsContent>

          {/* ══ Workspace ══════════════════════════════════════ */}
          <TabsContent value="workspace" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-2 px-5 py-4 pr-3">
                {([
                  { view: "chat" as WorkspaceView, label: "Chat", desc: "AI assistant" },
                  { view: "workflows" as WorkspaceView, label: "Agents", desc: "Build multi-step agents" },
                  { view: "overview" as WorkspaceView, label: "P2P Network", desc: "Peers, topology" },
                  { view: "tools" as WorkspaceView, label: "Tools", desc: "Execute & inspect tools" },
                  { view: "skills" as WorkspaceView, label: "Skills", desc: "SKILL.md prompts" },
                  { view: "mcp" as WorkspaceView, label: "MCP", desc: "Server configuration" },
                  { view: "providers" as WorkspaceView, label: "Providers", desc: "LLM sharing" },
                  { view: "help" as WorkspaceView, label: "Help", desc: "Docs & diagnostics" },
                ]).map((item) => (
                  <button
                    key={item.view}
                    type="button"
                    className="flex w-full items-center gap-3 rounded-lg border border-border/50 px-3 py-2.5 text-left transition-colors hover:bg-muted/30"
                    onClick={() => go(item.view)}
                  >
                    <div className="min-w-0 flex-1">
                      <p className="text-sm font-medium">{item.label}</p>
                      <p className="text-[11px] text-muted-foreground">{item.desc}</p>
                    </div>
                    <ChevronRight className="size-4 shrink-0 text-muted-foreground/40" />
                  </button>
                ))}
              </div>
            </ScrollArea>
          </TabsContent>

          {/* ══ Commands ═══════════════════════════════════════ */}
          <TabsContent value="reference" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-4 px-5 py-4 pr-3">
                <pre className="overflow-x-auto whitespace-pre-wrap break-words rounded-lg border border-border bg-muted/20 p-3 font-mono text-[10px] leading-relaxed text-primary">
                  {`peerclaw serve --web 127.0.0.1:8080 [--ollama] [--agent agent.toml]`}
                </pre>
                {Object.entries(byCat).map(([cat, cmds]) => (
                  <div key={cat}>
                    <p className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{cat}</p>
                    <div className="space-y-1">
                      {cmds.map((c) => (
                        <div key={c.cmd} className="rounded-md border border-border/40 bg-muted/10 px-2.5 py-1.5 text-xs">
                          <code className="font-mono text-primary">{c.cmd}</code>
                          {c.args && <span className="text-muted-foreground"> {c.args}</span>}
                          <p className="mt-0.5 text-[10px] text-muted-foreground">{c.desc}</p>
                        </div>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </ScrollArea>
          </TabsContent>
        </Tabs>

        <div className="shrink-0 border-t border-border px-5 py-3">
          <Button variant="secondary" className="w-full" size="sm" onClick={() => onOpenChange(false)}>
            Done
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

/* ── Shared components ──────────────────────────────────────────── */

function ProviderCard({
  checked, onChange, icon, title, desc, recommended, children,
}: {
  checked: boolean; onChange: (v: boolean) => void
  icon: React.ReactNode; title: string; desc: string
  recommended?: boolean; children?: React.ReactNode
}) {
  return (
    <div className={cn("rounded-xl border p-3 transition-colors", checked ? "border-primary/40 bg-primary/5" : "border-border")}>
      <label className="flex cursor-pointer items-start gap-3">
        <input type="checkbox" className="mt-0.5 size-4 accent-primary" checked={checked} onChange={(e) => onChange(e.target.checked)} />
        <div className="flex-1">
          <div className="flex items-center gap-2 text-sm font-medium">
            {icon} {title}
            {recommended && <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[9px] font-semibold text-primary">Recommended</span>}
          </div>
          <p className="mt-0.5 text-[10px] text-muted-foreground">{desc}</p>
        </div>
      </label>
      {checked && children}
    </div>
  )
}

function FeatureToggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <label className="flex cursor-pointer items-center gap-2.5 rounded-lg border border-border/40 px-3 py-2 text-sm transition-colors hover:bg-muted/20">
      <input type="checkbox" className="size-4 accent-primary" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  )
}
