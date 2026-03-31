import { useCallback, useEffect, useState } from "react"
import {
  Bot,
  Check,
  ChevronRight,
  Cloud,
  Download,
  Loader2,
  Monitor,
  Sparkles,
} from "lucide-react"

import {
  fetchInferenceSettings,
  fetchOnboarding,
  fetchOpenAiModels,
  putInferenceSettings,
  type InferenceSettingsResponse,
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

type Step = "welcome" | "inference" | "models" | "done"

export function SetupPage({ onFinish }: { onFinish: () => void }) {
  const [step, setStep] = useState<Step>("welcome")
  const [settings, setSettings] = useState<InferenceSettingsResponse | null>(null)
  const [onboarding, setOnboarding] = useState<OnboardingStep[]>([])
  const [models, setModels] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Form state
  const [useOllama, setUseOllama] = useState(true)
  const [ollamaUrl, setOllamaUrl] = useState("http://localhost:11434")
  const [useRemoteApi, setUseRemoteApi] = useState(false)
  const [remoteApiUrl, setRemoteApiUrl] = useState("")
  const [remoteApiModel, setRemoteApiModel] = useState("")
  const [remoteApiKey, setRemoteApiKey] = useState("")

  const loadSettings = useCallback(async () => {
    try {
      const s = await fetchInferenceSettings()
      setSettings(s)
      setUseOllama(s.use_ollama)
      setOllamaUrl(s.ollama_url || "http://localhost:11434")
      setUseRemoteApi(s.remote_api_enabled)
      setRemoteApiUrl(s.remote_api_base_url)
      setRemoteApiModel(s.remote_api_model)
    } catch {
      /* keep defaults */
    }
  }, [])

  useEffect(() => {
    void loadSettings()
    void fetchOnboarding()
      .then((o) => setOnboarding(o.steps))
      .catch(() => {})
    void fetchOpenAiModels()
      .then((m) => setModels(m.map((x) => x.id)))
      .catch(() => setModels([]))
  }, [loadSettings])

  const saveSettings = async () => {
    setSaving(true)
    setError(null)
    try {
      await putInferenceSettings({
        use_local_gguf: settings?.use_local_gguf ?? false,
        use_ollama: useOllama,
        ollama_url: ollamaUrl,
        remote_api_enabled: useRemoteApi,
        remote_api_base_url: remoteApiUrl,
        remote_api_model: remoteApiModel,
        ...(remoteApiKey ? { remote_api_key: remoteApiKey } : {}),
      })
      // Refresh models
      const m = await fetchOpenAiModels()
      setModels(m.map((x) => x.id))
      setStep("models")
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to save settings")
    } finally {
      setSaving(false)
    }
  }

  const finish = () => {
    markSetupDone()
    onFinish()
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <div className="w-full max-w-lg space-y-6">
        {/* Progress */}
        <div className="flex items-center justify-center gap-2">
          {(["welcome", "inference", "models", "done"] as Step[]).map((s, i) => (
            <div key={s} className="flex items-center gap-2">
              <div
                className={cn(
                  "flex size-8 items-center justify-center rounded-full text-xs font-semibold transition-colors",
                  step === s
                    ? "bg-primary text-primary-foreground"
                    : (["welcome", "inference", "models", "done"].indexOf(step) > i
                        ? "bg-primary/20 text-primary"
                        : "bg-muted text-muted-foreground"),
                )}
              >
                {["welcome", "inference", "models", "done"].indexOf(step) > i ? (
                  <Check className="size-4" />
                ) : (
                  i + 1
                )}
              </div>
              {i < 3 && <div className="h-px w-8 bg-border" />}
            </div>
          ))}
        </div>

        {/* ── Welcome ──────────────────────────────────────── */}
        {step === "welcome" && (
          <div className="space-y-6 text-center">
            <div className="mx-auto flex size-16 items-center justify-center rounded-2xl bg-primary/10">
              <Sparkles className="size-8 text-primary" />
            </div>
            <div>
              <h1 className="text-2xl font-bold">Welcome to PeerClaw</h1>
              <p className="mt-2 text-muted-foreground">
                Decentralized P2P AI agent network. Let's configure your node.
              </p>
            </div>
            <div className="space-y-2 text-left text-sm text-muted-foreground">
              {onboarding.map((s) => (
                <div key={s.id} className="flex items-center gap-2 rounded-lg bg-muted/20 px-3 py-2">
                  <span className={s.ok ? "text-emerald-500" : "text-muted-foreground"}>
                    {s.ok ? <Check className="size-4" /> : <span className="inline-block size-4 rounded-full border-2 border-current" />}
                  </span>
                  <span className="capitalize">{s.id.replace(/_/g, " ")}</span>
                </div>
              ))}
            </div>
            <Button size="lg" className="w-full gap-2" onClick={() => setStep("inference")}>
              Get started
              <ChevronRight className="size-4" />
            </Button>
          </div>
        )}

        {/* ── Inference settings ───────────────────────────── */}
        {step === "inference" && (
          <div className="space-y-5">
            <div>
              <h2 className="text-xl font-bold">Configure AI Models</h2>
              <p className="mt-1 text-sm text-muted-foreground">
                Choose how PeerClaw runs inference. You can use Ollama (local), a remote API, or both.
              </p>
            </div>

            {/* Ollama */}
            <div
              className={cn(
                "rounded-xl border p-4 transition-colors",
                useOllama ? "border-primary/40 bg-primary/5" : "border-border",
              )}
            >
              <label className="flex cursor-pointer items-start gap-3">
                <input
                  type="checkbox"
                  className="mt-1 size-4 accent-primary"
                  checked={useOllama}
                  onChange={(e) => setUseOllama(e.target.checked)}
                />
                <div className="flex-1">
                  <div className="flex items-center gap-2 font-medium">
                    <Monitor className="size-4" />
                    Ollama (local)
                  </div>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    Run models locally via Ollama. Install from ollama.com.
                  </p>
                </div>
              </label>
              {useOllama && (
                <div className="mt-3 pl-7">
                  <Label className="text-xs">Ollama URL</Label>
                  <Input
                    className="mt-1 h-9 text-sm"
                    value={ollamaUrl}
                    onChange={(e) => setOllamaUrl(e.target.value)}
                    placeholder="http://localhost:11434"
                  />
                  <p className="mt-1 text-[10px] text-muted-foreground">
                    Docker: use <code>http://host.docker.internal:11434</code>
                  </p>
                </div>
              )}
            </div>

            {/* Remote API */}
            <div
              className={cn(
                "rounded-xl border p-4 transition-colors",
                useRemoteApi ? "border-primary/40 bg-primary/5" : "border-border",
              )}
            >
              <label className="flex cursor-pointer items-start gap-3">
                <input
                  type="checkbox"
                  className="mt-1 size-4 accent-primary"
                  checked={useRemoteApi}
                  onChange={(e) => setUseRemoteApi(e.target.checked)}
                />
                <div className="flex-1">
                  <div className="flex items-center gap-2 font-medium">
                    <Cloud className="size-4" />
                    Remote API (OpenAI-compatible)
                  </div>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    OpenAI, Anthropic, Groq, Together, or any compatible endpoint.
                  </p>
                </div>
              </label>
              {useRemoteApi && (
                <div className="mt-3 space-y-2 pl-7">
                  <div>
                    <Label className="text-xs">API Base URL</Label>
                    <Input
                      className="mt-1 h-9 text-sm"
                      value={remoteApiUrl}
                      onChange={(e) => setRemoteApiUrl(e.target.value)}
                      placeholder="https://api.openai.com/v1"
                    />
                  </div>
                  <div>
                    <Label className="text-xs">Model</Label>
                    <Input
                      className="mt-1 h-9 text-sm"
                      value={remoteApiModel}
                      onChange={(e) => setRemoteApiModel(e.target.value)}
                      placeholder="gpt-4o-mini"
                    />
                  </div>
                  <div>
                    <Label className="text-xs">API Key</Label>
                    <Input
                      type="password"
                      className="mt-1 h-9 text-sm"
                      value={remoteApiKey}
                      onChange={(e) => setRemoteApiKey(e.target.value)}
                      placeholder="sk-..."
                    />
                  </div>
                </div>
              )}
            </div>

            {error && <p className="text-sm text-destructive">{error}</p>}

            <div className="flex gap-2">
              <Button variant="outline" onClick={() => setStep("welcome")}>
                Back
              </Button>
              <Button className="flex-1 gap-2" disabled={saving} onClick={() => void saveSettings()}>
                {saving ? <Loader2 className="size-4 animate-spin" /> : null}
                Save & continue
              </Button>
            </div>
          </div>
        )}

        {/* ── Models ───────────────────────────────────────── */}
        {step === "models" && (
          <div className="space-y-5">
            <div>
              <h2 className="text-xl font-bold">Available Models</h2>
              <p className="mt-1 text-sm text-muted-foreground">
                {models.length > 0
                  ? `${models.length} model(s) detected. You're ready to go.`
                  : "No models found. Pull a model in Ollama or configure a remote API."}
              </p>
            </div>

            {models.length > 0 ? (
              <div className="max-h-48 space-y-1 overflow-y-auto rounded-xl border border-border p-2">
                {models.map((m) => (
                  <div
                    key={m}
                    className="flex items-center gap-2 rounded-lg bg-muted/20 px-3 py-2 text-sm"
                  >
                    <Bot className="size-4 shrink-0 text-primary" />
                    <span className="font-mono text-xs">{m}</span>
                  </div>
                ))}
              </div>
            ) : (
              <div className="rounded-xl border border-dashed border-border p-6 text-center text-sm text-muted-foreground">
                <Download className="mx-auto mb-2 size-8 opacity-40" />
                <p>Run <code className="text-foreground">ollama pull llama3.2</code> to download a model.</p>
                <p className="mt-1 text-xs">Or configure a remote API in the previous step.</p>
              </div>
            )}

            <div className="flex gap-2">
              <Button variant="outline" onClick={() => setStep("inference")}>
                Back
              </Button>
              <Button className="flex-1 gap-2" onClick={() => setStep("done")}>
                {models.length > 0 ? "Continue" : "Skip for now"}
                <ChevronRight className="size-4" />
              </Button>
            </div>
          </div>
        )}

        {/* ── Done ─────────────────────────────────────────── */}
        {step === "done" && (
          <div className="space-y-6 text-center">
            <div className="mx-auto flex size-16 items-center justify-center rounded-2xl bg-emerald-500/10">
              <Check className="size-8 text-emerald-500" />
            </div>
            <div>
              <h2 className="text-xl font-bold">You're all set!</h2>
              <p className="mt-2 text-sm text-muted-foreground">
                PeerClaw is configured and ready. You can change settings anytime from the sidebar.
              </p>
            </div>
            <div className="space-y-2 text-left text-sm">
              <div className="rounded-lg bg-muted/20 px-4 py-3">
                <p className="font-medium">What you can do:</p>
                <ul className="mt-1 space-y-1 text-muted-foreground">
                  <li>Chat with AI models in the <strong className="text-foreground">Chat</strong> tab</li>
                  <li>Build multi-step agents in the <strong className="text-foreground">Agents</strong> builder</li>
                  <li>Execute tools in the <strong className="text-foreground">Tools</strong> panel</li>
                  <li>Connect to the P2P mesh in <strong className="text-foreground">P2P Network</strong></li>
                </ul>
              </div>
            </div>
            <Button size="lg" className="w-full gap-2" onClick={finish}>
              Open PeerClaw
              <ChevronRight className="size-4" />
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}
