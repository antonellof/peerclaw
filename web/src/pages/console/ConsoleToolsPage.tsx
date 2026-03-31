import { useCallback, useEffect, useMemo, useState } from "react"
import {
  Bot,
  ChevronRight,
  Clock,
  FileText,
  Globe,
  Loader2,
  Play,
  Search,
  Terminal,
  Wrench,
} from "lucide-react"

import {
  executeTool,
  fetchToolDetail,
  fetchTools,
  type ToolDetailInfo,
  type ToolExecResult,
  type ToolInfo,
} from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

/* ── Helpers ────────────────────────────────────────────────────────── */

const TOOL_ICONS: Record<string, typeof Wrench> = {
  echo: Terminal,
  time: Clock,
  json: FileText,
  http: Globe,
  web_fetch: Globe,
  web_search: Search,
  shell: Terminal,
  file_read: FileText,
  file_write: FileText,
  file_list: FileText,
  apply_patch: FileText,
  pdf_read: FileText,
  browser: Globe,
  memory_search: Search,
  memory_write: FileText,
  job_submit: Bot,
  job_status: Bot,
  peer_discovery: Bot,
  wallet_balance: Bot,
  llm_task: Bot,
  agent_spawn: Bot,
}

function toolIcon(name: string) {
  return TOOL_ICONS[name] ?? Wrench
}

/** Build a default args JSON from required_params. */
function defaultArgs(params?: string[]): string {
  if (!params || params.length === 0) return "{}"
  const obj: Record<string, string> = {}
  for (const p of params) obj[p] = ""
  return JSON.stringify(obj, null, 2)
}

/* ── Component ──────────────────────────────────────────────────────── */

export function ConsoleToolsPage() {
  const [tools, setTools] = useState<ToolInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [filter, setFilter] = useState("")
  const [selectedName, setSelectedName] = useState<string | null>(null)
  const [detail, setDetail] = useState<ToolDetailInfo | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)

  // Execution
  const [argsJson, setArgsJson] = useState("{}")
  const [executing, setExecuting] = useState(false)
  const [lastResult, setLastResult] = useState<ToolExecResult | null>(null)

  const load = useCallback(async () => {
    try {
      const resp = await fetchTools()
      setTools(resp.tools)
      if (resp.tools.length > 0 && !selectedName) {
        setSelectedName(resp.tools[0]!.name)
      }
    } catch {
      setTools([])
    } finally {
      setLoading(false)
    }
  }, [selectedName])

  useEffect(() => {
    void load()
  }, [load])

  // Fetch detail when selection changes
  useEffect(() => {
    if (!selectedName) {
      setDetail(null)
      return
    }
    setDetailLoading(true)
    setLastResult(null)
    void fetchToolDetail(selectedName).then((d) => {
      setDetail(d)
      setDetailLoading(false)
      // Build default args from schema
      const schema = d?.parameters_schema as Record<string, unknown> | undefined
      const required = (schema?.required as string[]) ?? d?.required_params
      setArgsJson(defaultArgs(required))
    })
  }, [selectedName])

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase()
    if (!q) return tools
    return tools.filter(
      (t) => t.name.toLowerCase().includes(q) || (t.description ?? "").toLowerCase().includes(q),
    )
  }, [tools, filter])

  const handleExecute = async () => {
    if (!selectedName) return
    setExecuting(true)
    setLastResult(null)
    try {
      const args = JSON.parse(argsJson) as Record<string, unknown>
      const result = await executeTool({ name: selectedName, args })
      setLastResult(result)
    } catch (e) {
      setLastResult({
        ok: false,
        success: false,
        data: null,
        output: null,
        error: e instanceof Error ? e.message : "Execution failed",
        message: e instanceof Error ? e.message : "Execution failed",
        duration_ms: 0,
      })
    } finally {
      setExecuting(false)
    }
  }

  const schema = detail?.parameters_schema as Record<string, unknown> | undefined
  const properties = schema?.properties as Record<string, { type?: string; description?: string; enum?: string[] }> | undefined
  const required = (schema?.required as string[]) ?? []

  return (
    <div className="flex h-full min-h-0 gap-0">
      {/* ── Left column: tool list ────────────────────────────── */}
      <div className="flex w-64 shrink-0 flex-col border-r border-border/50 lg:w-72">
        <div className="border-b border-border/50 px-3 py-2.5">
          <div className="relative">
            <Search className="absolute left-2.5 top-2.5 size-3.5 text-muted-foreground" />
            <Input
              className="h-8 pl-8 text-xs"
              placeholder="Filter tools…"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>
        </div>
        <ScrollArea className="flex-1">
          {loading ? (
            <p className="p-4 text-sm text-muted-foreground">Loading…</p>
          ) : filtered.length === 0 ? (
            <p className="p-4 text-sm text-muted-foreground">No tools found.</p>
          ) : (
            <div className="flex flex-col gap-0.5 p-1.5">
              {filtered.map((t) => {
                const Icon = toolIcon(t.name)
                const active = selectedName === t.name
                return (
                  <button
                    key={t.name}
                    type="button"
                    className={cn(
                      "flex w-full items-center gap-2.5 overflow-hidden rounded-lg px-2.5 py-2 text-left transition-colors",
                      active
                        ? "bg-primary/10 text-foreground"
                        : "text-muted-foreground hover:bg-muted/40 hover:text-foreground",
                    )}
                    onClick={() => setSelectedName(t.name)}
                  >
                    <Icon className="size-4 shrink-0" />
                    <div className="min-w-0 flex-1">
                      <p className="block truncate text-[13px] font-medium">{t.name}</p>
                      <p className="line-clamp-2 break-words text-[11px] leading-tight opacity-60">{t.description}</p>
                    </div>
                    {active && <ChevronRight className="size-3.5 shrink-0 opacity-40" />}
                  </button>
                )
              })}
            </div>
          )}
        </ScrollArea>
        <div className="border-t border-border/50 px-3 py-2 text-[10px] text-muted-foreground">
          {tools.length} tools available
        </div>
      </div>

      {/* ── Right column: detail + execute ─────────────────────── */}
      <ScrollArea className="min-w-0 flex-1">
        {!selectedName ? (
          <div className="flex h-full items-center justify-center p-8 text-sm text-muted-foreground">
            Select a tool to view details and execute it.
          </div>
        ) : detailLoading ? (
          <div className="flex items-center justify-center p-8">
            <Loader2 className="size-5 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <div className="space-y-6 p-5">
            {/* Header */}
            <div>
              <div className="flex items-center gap-3">
                <h2 className="text-lg font-semibold">{selectedName}</h2>
                {detail?.location && (
                  <Badge variant="outline" className="text-[10px]">
                    {detail.location}
                  </Badge>
                )}
              </div>
              <p className="mt-1 text-sm text-muted-foreground">
                {detail?.description || "No description."}
              </p>

              {/* Stats */}
              {detail?.stats && detail.stats.total_calls > 0 && (
                <div className="mt-3 flex gap-4 text-xs text-muted-foreground">
                  <span>{detail.stats.total_calls} calls</span>
                  <span className="text-emerald-500">{detail.stats.successful_calls} ok</span>
                  {detail.stats.failed_calls > 0 && (
                    <span className="text-destructive">{detail.stats.failed_calls} failed</span>
                  )}
                  <span>avg {Math.round(detail.stats.total_time_ms / detail.stats.total_calls)}ms</span>
                </div>
              )}
            </div>

            {/* Parameters */}
            {properties && Object.keys(properties).length > 0 && (
              <div>
                <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                  Parameters
                </h3>
                <div className="space-y-2">
                  {Object.entries(properties).map(([name, prop]) => (
                    <div
                      key={name}
                      className="flex items-start gap-3 rounded-lg border border-border/50 bg-muted/10 px-3 py-2"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <code className="text-xs font-semibold text-foreground">{name}</code>
                          {prop.type && (
                            <span className="text-[10px] text-muted-foreground">{prop.type}</span>
                          )}
                          {required.includes(name) && (
                            <Badge variant="secondary" className="h-4 px-1 text-[9px]">
                              required
                            </Badge>
                          )}
                        </div>
                        {prop.description && (
                          <p className="mt-0.5 text-[11px] text-muted-foreground">
                            {prop.description}
                          </p>
                        )}
                        {prop.enum && (
                          <div className="mt-1 flex flex-wrap gap-1">
                            {prop.enum.map((v) => (
                              <Badge key={v} variant="outline" className="text-[9px]">
                                {v}
                              </Badge>
                            ))}
                          </div>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Execute */}
            <div className="space-y-3 rounded-xl border border-border/60 bg-card/50 p-4">
              <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                Execute
              </h3>
              <div>
                <Label className="text-xs">Arguments (JSON)</Label>
                <Textarea
                  className="mt-1 font-mono text-xs"
                  rows={Math.max(3, argsJson.split("\n").length)}
                  value={argsJson}
                  onChange={(e) => setArgsJson(e.target.value)}
                />
              </div>
              <Button
                size="sm"
                className="gap-1.5"
                disabled={executing}
                onClick={() => void handleExecute()}
              >
                {executing ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <Play className="size-3.5" />
                )}
                {executing ? "Running…" : "Run"}
              </Button>

              {lastResult && (
                <div
                  className={cn(
                    "rounded-lg border p-3",
                    lastResult.ok
                      ? "border-emerald-500/30 bg-emerald-500/5"
                      : "border-destructive/30 bg-destructive/5",
                  )}
                >
                  <div className="flex items-center justify-between text-xs">
                    <span
                      className={cn(
                        "font-medium",
                        lastResult.ok ? "text-emerald-500" : "text-destructive",
                      )}
                    >
                      {lastResult.ok ? "Success" : "Error"}
                    </span>
                    {lastResult.duration_ms != null && (
                      <span className="text-muted-foreground">{lastResult.duration_ms}ms</span>
                    )}
                  </div>
                  <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] text-muted-foreground">
                    {typeof lastResult.output === "object" && lastResult.output != null
                      ? JSON.stringify(lastResult.output, null, 2)
                      : String(
                          lastResult.error ||
                            lastResult.output ||
                            lastResult.message ||
                            "(empty)",
                        )}
                  </pre>
                </div>
              )}
            </div>
          </div>
        )}
      </ScrollArea>
    </div>
  )
}
