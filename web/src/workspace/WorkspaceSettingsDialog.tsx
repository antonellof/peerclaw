import { useCallback, useEffect, useState } from "react"
import { Briefcase, BookOpen, Cpu, HardDrive, Home, LayoutGrid, Plug, Terminal } from "lucide-react"

import {
  downloadGgufModel,
  fetchInferenceSettings,
  fetchOpenAiModels,
  putInferenceSettings,
  type InferenceSettingsResponse,
  type InferenceSettingsPut,
} from "@/lib/api"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
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
  onNavigate: (view: WorkspaceView) => void
}

export function WorkspaceSettingsDialog({
  open,
  onOpenChange,
  chatPreferences,
  setChatPreferences,
  onNavigate,
}: Props) {
  const [models, setModels] = useState<string[]>([])
  const [inf, setInf] = useState<InferenceSettingsResponse | null>(null)
  const [infErr, setInfErr] = useState<string | null>(null)
  const [infSaving, setInfSaving] = useState(false)
  const [remoteKeyDraft, setRemoteKeyDraft] = useState("")
  const [dlPreset, setDlPreset] = useState("llama-3.2-3b")
  const [dlQuant, setDlQuant] = useState("q4_k_m")
  const [dlUrl, setDlUrl] = useState("")
  const [dlFilename, setDlFilename] = useState("")
  const [dlBusy, setDlBusy] = useState(false)
  const [dlMsg, setDlMsg] = useState<string | null>(null)

  const loadInf = useCallback(async () => {
    setInfErr(null)
    try {
      const s = await fetchInferenceSettings()
      setInf(s)
      setRemoteKeyDraft("")
    } catch (e) {
      setInf(null)
      setInfErr(e instanceof Error ? e.message : "Inference API unavailable (use full peerclaw serve --web).")
    }
  }, [])

  const loadModels = useCallback(async () => {
    try {
      const list = await fetchOpenAiModels()
      const ids = list.map((m) => m.id).filter(Boolean)
      setModels(ids.length ? ids : ["llama-3.2-3b", "llama-3.2-1b", "phi-3-mini"])
    } catch {
      setModels(["llama-3.2-3b", "llama-3.2-1b", "phi-3-mini"])
    }
  }, [])

  useEffect(() => {
    if (open) {
      void loadModels()
      void loadInf()
    }
  }, [open, loadModels, loadInf])

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
    } catch (e) {
      setInfErr(e instanceof Error ? e.message : "Save failed")
    } finally {
      setInfSaving(false)
    }
  }

  const runPresetDownload = async () => {
    setDlBusy(true)
    setDlMsg(null)
    try {
      const r = await downloadGgufModel({ preset: dlPreset, quant: dlQuant })
      if (r.success) setDlMsg(`Saved ${r.path ?? ""} (${((r.bytes ?? 0) / 1048576).toFixed(0)} MB).`)
      else setDlMsg(r.error ?? "Download failed")
    } catch (e) {
      setDlMsg(e instanceof Error ? e.message : "Download failed")
    } finally {
      setDlBusy(false)
      void loadInf()
      void loadModels()
    }
  }

  const runUrlDownload = async () => {
    const u = dlUrl.trim()
    if (!u) {
      setDlMsg("Paste a Hugging Face resolve URL to a .gguf file.")
      return
    }
    setDlBusy(true)
    setDlMsg(null)
    try {
      const r = await downloadGgufModel({
        url: u,
        filename: dlFilename.trim() || undefined,
      })
      if (r.success) setDlMsg(`Saved ${r.path ?? ""}.`)
      else setDlMsg(r.error ?? "Download failed")
    } catch (e) {
      setDlMsg(e instanceof Error ? e.message : "Download failed")
    } finally {
      setDlBusy(false)
      void loadInf()
      void loadModels()
    }
  }

  const go = (v: WorkspaceView) => {
    onNavigate(v)
    onOpenChange(false)
  }

  const byCat = SLASH_COMMANDS.reduce<Record<string, typeof SLASH_COMMANDS>>((acc, c) => {
    ;(acc[c.category] ??= []).push(c)
    return acc
  }, {})

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className={cn(
          "flex max-h-[min(90dvh,720px)] w-[min(100%,520px)] max-w-[calc(100vw-1.5rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-lg",
        )}
      >
        <DialogHeader className="shrink-0 border-b border-border px-5 py-4 pr-12 text-left">
          <DialogTitle className="text-base">Settings</DialogTitle>
          <DialogDescription className="text-xs">
            Workspace panels, chat defaults, and command reference (also available via{" "}
            <kbd className="rounded border border-border px-1 font-mono">/</kbd> in chat).
          </DialogDescription>
        </DialogHeader>

        <Tabs defaultValue="workspace" className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="shrink-0 border-b border-border px-4 pt-1">
            <TabsList className="h-auto w-full flex-wrap justify-start gap-1 bg-transparent p-0 pb-2">
              <TabsTrigger value="workspace" className="text-xs">
                Workspace
              </TabsTrigger>
              <TabsTrigger value="chat" className="text-xs">
                Chat
              </TabsTrigger>
              <TabsTrigger value="inference" className="text-xs">
                <HardDrive className="mr-1 inline size-3" />
                Inference
              </TabsTrigger>
              <TabsTrigger value="reference" className="text-xs">
                <Terminal className="mr-1 inline size-3" />
                Commands
              </TabsTrigger>
            </TabsList>
          </div>

          <TabsContent value="workspace" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-4 px-5 py-4 pr-3">
                <p className="text-xs text-muted-foreground">
                  Open console panels. Same destinations as <code className="text-primary">/open</code> slash routes.
                </p>
                <div className="grid gap-2 sm:grid-cols-2">
                  <Button variant="outline" className="h-auto justify-start gap-2 py-3 text-left" onClick={() => go("home")}>
                    <Home className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">Home</span>
                  </Button>
                  <Button variant="outline" className="h-auto justify-start gap-2 py-3 text-left" onClick={() => go("jobs")}>
                    <Briefcase className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">Jobs</span>
                  </Button>
                  <Button
                    variant="outline"
                    className="h-auto justify-start gap-2 py-3 text-left"
                    onClick={() => go("providers")}
                  >
                    <Cpu className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">Providers</span>
                  </Button>
                  <Button variant="outline" className="h-auto justify-start gap-2 py-3 text-left" onClick={() => go("skills")}>
                    <BookOpen className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">Skills</span>
                  </Button>
                  <Button variant="outline" className="h-auto justify-start gap-2 py-3 text-left" onClick={() => go("mcp")}>
                    <Plug className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">MCP servers</span>
                  </Button>
                  <Button
                    variant="outline"
                    className="h-auto justify-start gap-2 py-3 text-left"
                    onClick={() => go("overview")}
                  >
                    <LayoutGrid className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">P2P Network</span>
                  </Button>
                </div>
              </div>
            </ScrollArea>
          </TabsContent>

          <TabsContent value="chat" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-4 px-5 py-4 pr-3">
                <p className="text-xs text-muted-foreground">
                  Defaults for the chat composer. Slash commands like <code className="text-primary">/model</code> still
                  override for the session.
                </p>
                <div className="space-y-1.5">
                  <Label className="text-xs">Default model</Label>
                  <select
                    className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                    value={chatPreferences.model}
                    onChange={(e) => setChatPreferences({ model: e.target.value })}
                  >
                    {(models.length ? models : [chatPreferences.model]).map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                  </select>
                  <p className="text-[10px] text-muted-foreground">
                    Models come from your enabled backends below (Inference tab). Add more via GGUF download or Ollama.
                  </p>
                </div>
                <div className="grid gap-4 sm:grid-cols-2">
                  <div className="space-y-1.5">
                    <Label className="text-xs">Temperature</Label>
                    <input
                      type="number"
                      step={0.05}
                      min={0}
                      max={2}
                      className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                      value={chatPreferences.temperature}
                      onChange={(e) => setChatPreferences({ temperature: parseFloat(e.target.value) || 0.7 })}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label className="text-xs">Max tokens</Label>
                    <input
                      type="number"
                      min={16}
                      max={32000}
                      step={1}
                      className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                      value={chatPreferences.maxTokens}
                      onChange={(e) => setChatPreferences({ maxTokens: parseInt(e.target.value, 10) || 500 })}
                    />
                  </div>
                </div>
                <div className="space-y-2 rounded-lg border border-border bg-muted/20 p-3">
                  <div className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                    Features
                  </div>
                  <label className="flex cursor-pointer items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      className="size-4 rounded border-input"
                      checked={chatPreferences.useAgentic}
                      onChange={(e) => setChatPreferences({ useAgentic: e.target.checked })}
                    />
                    <span>
                      Agentic chat (tools: <code className="text-foreground">web_fetch</code>, <code className="text-foreground">job_submit</code>, shell, …)
                    </span>
                  </label>
                  <label className="flex cursor-pointer items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      className="size-4 rounded border-input"
                      checked={chatPreferences.useMcp}
                      onChange={(e) => setChatPreferences({ useMcp: e.target.checked })}
                    />
                    <span>
                      Use MCP tools (configure under Workspace → MCP servers)
                    </span>
                  </label>
                  <label className="flex cursor-pointer items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      className="size-4 rounded border-input"
                      checked={chatPreferences.distributed}
                      onChange={(e) => setChatPreferences({ distributed: e.target.checked })}
                    />
                    <span>Prefer distributed (P2P) inference</span>
                  </label>
                </div>
              </div>
            </ScrollArea>
          </TabsContent>

          <TabsContent
            value="inference"
            className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none"
          >
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-4 px-5 py-4 pr-3">
                <p className="text-xs text-muted-foreground">
                  Inference backends and model management. Priority:{" "}
                  <span className="font-medium text-foreground">Remote API</span> →{" "}
                  <span className="font-medium text-foreground">Local GGUF</span> →{" "}
                  <span className="font-medium text-foreground">Ollama</span>.
                  Changes are saved to <code className="text-primary">config.toml</code>.
                </p>
                {infErr ? (
                  <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-xs text-destructive">
                    {infErr}
                  </p>
                ) : null}
                {inf ? (
                  <>
                    {/* ── Ollama ── */}
                    <div className="space-y-2 rounded-lg border border-border bg-muted/20 p-3">
                      <div className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                        Ollama
                      </div>
                      <label className="flex cursor-pointer items-center gap-2 text-sm">
                        <input
                          type="checkbox"
                          className="size-4 rounded border-input"
                          checked={inf.use_ollama}
                          onChange={(e) => setInf({ ...inf, use_ollama: e.target.checked })}
                        />
                        <span>Enable Ollama</span>
                      </label>
                      {inf.use_ollama && (
                        <div className="space-y-1.5">
                          <Label className="text-xs">Base URL</Label>
                          <input
                            type="url"
                            className="flex h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-xs"
                            value={inf.ollama_url}
                            onChange={(e) => setInf({ ...inf, ollama_url: e.target.value })}
                          />
                        </div>
                      )}
                    </div>

                    {/* ── Local GGUF ── */}
                    <div className="space-y-2 rounded-lg border border-border bg-muted/20 p-3">
                      <div className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                        Local GGUF models
                      </div>
                      <label className="flex cursor-pointer items-center gap-2 text-sm">
                        <input
                          type="checkbox"
                          className="size-4 rounded border-input"
                          checked={inf.use_local_gguf}
                          onChange={(e) => setInf({ ...inf, use_local_gguf: e.target.checked })}
                        />
                        <span>Use local GGUF files (Llama, Phi, Qwen, Gemma)</span>
                      </label>
                      <p className="text-[10px] text-muted-foreground">
                        Directory: <code className="break-all text-primary">{inf.models_directory}</code>
                      </p>
                    </div>

                    {/* ── Remote API ── */}
                    <div className="space-y-2 rounded-lg border border-border bg-muted/20 p-3">
                      <div className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                        Remote API (OpenAI-compatible)
                      </div>
                      <label className="flex cursor-pointer items-center gap-2 text-sm">
                        <input
                          type="checkbox"
                          className="size-4 rounded border-input"
                          checked={inf.remote_api_enabled}
                          onChange={(e) => setInf({ ...inf, remote_api_enabled: e.target.checked })}
                        />
                        <span>Enable remote API</span>
                      </label>
                      {inf.remote_api_enabled && (
                        <>
                          <div className="space-y-1.5">
                            <Label className="text-xs">Base URL</Label>
                            <input
                              type="url"
                              placeholder="https://api.openai.com/v1"
                              className="flex h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-xs"
                              value={inf.remote_api_base_url}
                              onChange={(e) => setInf({ ...inf, remote_api_base_url: e.target.value })}
                            />
                          </div>
                          <div className="space-y-1.5">
                            <Label className="text-xs">Model override (optional)</Label>
                            <input
                              type="text"
                              placeholder="Empty = use the model selected in chat"
                              className="flex h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-xs"
                              value={inf.remote_api_model}
                              onChange={(e) => setInf({ ...inf, remote_api_model: e.target.value })}
                            />
                          </div>
                          <div className="space-y-1.5">
                            <Label className="text-xs">
                              API key{inf.api_key_configured ? " (configured — enter to replace)" : ""}
                            </Label>
                            <input
                              type="password"
                              autoComplete="off"
                              placeholder={inf.api_key_configured ? "••••••••" : "sk-…"}
                              className="flex h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-xs"
                              value={remoteKeyDraft}
                              onChange={(e) => setRemoteKeyDraft(e.target.value)}
                            />
                          </div>
                        </>
                      )}
                    </div>

                    <Button type="button" size="sm" disabled={infSaving} onClick={() => void saveInference()}>
                      {infSaving ? "Saving…" : "Save settings"}
                    </Button>

                    {/* ── Download GGUF ── */}
                    <div className="space-y-3 border-t border-border pt-4">
                      <div className="text-xs font-semibold text-foreground">Download GGUF models</div>
                      <p className="text-[11px] text-muted-foreground">
                        Pick a preset or paste any Hugging Face <code className="text-primary">.gguf</code> URL.
                        Files are saved to the models directory above.
                      </p>
                      <div className="space-y-2 rounded-lg border border-border bg-muted/20 p-3">
                        <div className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                          Preset
                        </div>
                        <div className="flex flex-wrap items-end gap-2">
                          <div className="min-w-[10rem] flex-1 space-y-1">
                            <select
                              className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-xs"
                              value={dlPreset}
                              onChange={(e) => setDlPreset(e.target.value)}
                            >
                              {inf.gguf_presets.map((p) => (
                                <option key={p.id} value={p.id}>
                                  {p.id}
                                </option>
                              ))}
                            </select>
                          </div>
                          <div className="w-24 space-y-1">
                            <Label className="text-[10px]">Quant</Label>
                            <input
                              className="flex h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-xs"
                              value={dlQuant}
                              onChange={(e) => setDlQuant(e.target.value)}
                            />
                          </div>
                          <Button type="button" size="sm" disabled={dlBusy} onClick={() => void runPresetDownload()}>
                            {dlBusy ? "Downloading…" : "Download"}
                          </Button>
                        </div>
                      </div>
                      <div className="space-y-2 rounded-lg border border-border bg-muted/20 p-3">
                        <div className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                          Custom URL
                        </div>
                        <div className="space-y-1.5">
                          <input
                            type="url"
                            placeholder="https://huggingface.co/…/resolve/main/….gguf"
                            className="flex h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-[11px]"
                            value={dlUrl}
                            onChange={(e) => setDlUrl(e.target.value)}
                          />
                        </div>
                        <div className="flex items-end gap-2">
                          <div className="flex-1 space-y-1">
                            <Label className="text-[10px]">Save as (optional)</Label>
                            <input
                              type="text"
                              placeholder="my-model.gguf"
                              className="flex h-9 w-full rounded-md border border-input bg-background px-2 font-mono text-xs"
                              value={dlFilename}
                              onChange={(e) => setDlFilename(e.target.value)}
                            />
                          </div>
                          <Button type="button" size="sm" variant="outline" disabled={dlBusy} onClick={() => void runUrlDownload()}>
                            {dlBusy ? "Downloading…" : "Download"}
                          </Button>
                        </div>
                      </div>
                      {dlMsg ? (
                        <p className={cn(
                          "rounded-md border px-2 py-1.5 text-xs",
                          dlMsg.startsWith("Saved")
                            ? "border-green-500/30 bg-green-500/10 text-green-700 dark:text-green-400"
                            : "border-destructive/30 bg-destructive/10 text-destructive",
                        )}>
                          {dlMsg}
                        </p>
                      ) : null}
                    </div>
                  </>
                ) : !infErr ? (
                  <p className="text-xs text-muted-foreground">Loading…</p>
                ) : null}
              </div>
            </ScrollArea>
          </TabsContent>

          <TabsContent value="reference" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-4 px-5 py-4 pr-3">
                <section className="space-y-2">
                  <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">CLI (serve)</h3>
                  <pre className="overflow-x-auto whitespace-pre-wrap break-words rounded-lg border border-border bg-muted/40 p-3 font-mono text-[10px] leading-relaxed text-primary">
                    {`peerclaw serve --web 127.0.0.1:8080 \\
  [--agent path/to/agent.toml] [--ollama] [--gpu] \\
  [--share-inference] [--provider-max-requests N]`}
                  </pre>
                  <p className="text-[11px] text-muted-foreground">
                    Run <code className="text-foreground">peerclaw --help</code> and{" "}
                    <code className="text-foreground">peerclaw serve --help</code> for full flags.
                  </p>
                </section>
                {Object.entries(byCat).map(([cat, cmds]) => (
                  <div key={cat}>
                    <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{cat}</div>
                    <ul className="space-y-1.5 text-xs">
                      {cmds.map((c) => (
                        <li
                          key={c.cmd}
                          className="break-words rounded-md border border-border/50 bg-muted/15 px-2 py-1.5"
                        >
                          <code className="font-mono text-primary">{c.cmd}</code>
                          {c.args ? (
                            <span className="text-muted-foreground"> {c.args}</span>
                          ) : null}
                          <div className="mt-0.5 text-[11px] text-muted-foreground">{c.desc}</div>
                        </li>
                      ))}
                    </ul>
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
