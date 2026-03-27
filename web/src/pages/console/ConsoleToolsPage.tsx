import { useEffect, useState, useCallback } from "react"

import { fetchTools, executeTool, type ToolInfo, type ToolExecResult } from "@/lib/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"

type ExecHistoryEntry = {
  tool: string
  args: string
  result: ToolExecResult
  timestamp: number
}

export function ConsoleToolsPage() {
  const [tools, setTools] = useState<ToolInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  // Execution panel
  const [selectedTool, setSelectedTool] = useState("")
  const [argsJson, setArgsJson] = useState("{}")
  const [executing, setExecuting] = useState(false)
  const [lastResult, setLastResult] = useState<ToolExecResult | null>(null)
  const [history, setHistory] = useState<ExecHistoryEntry[]>([])

  const load = useCallback(async () => {
    try {
      const resp = await fetchTools()
      setTools(resp.tools)
      setError(null)
      if (resp.tools.length > 0 && !selectedTool) setSelectedTool(resp.tools[0].name)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load tools")
      setTools([])
    } finally {
      setLoading(false)
    }
  }, [selectedTool])

  useEffect(() => {
    void load()
  }, [load])

  const handleExecute = async (toolName?: string, preArgs?: string) => {
    const name = toolName ?? selectedTool
    const rawArgs = preArgs ?? argsJson
    if (!name) return

    setExecuting(true)
    setLastResult(null)
    try {
      const args = JSON.parse(rawArgs) as Record<string, unknown>
      const result = await executeTool({ name, args })
      setLastResult(result)
      setHistory((prev) => [
        { tool: name, args: rawArgs, result, timestamp: Date.now() },
        ...prev.slice(0, 19),
      ])
    } catch (e) {
      const errResult: ToolExecResult = {
        ok: false,
        success: false,
        data: null,
        output: null,
        error: e instanceof Error ? e.message : "Execution failed",
        message: e instanceof Error ? e.message : "Execution failed",
        duration_ms: 0,
      }
      setLastResult(errResult)
    } finally {
      setExecuting(false)
    }
  }

  const quickActions = [
    { label: "Fetch URL", tool: "web_fetch", defaultArgs: '{"url": "https://example.com"}' },
    { label: "Shell Command", tool: "shell", defaultArgs: '{"command": "echo hello"}' },
  ]

  return (
    <div className="space-y-6">
      {/* Quick actions */}
      <div className="flex flex-wrap gap-2">
        {quickActions.map((qa) => (
          <Button
            key={qa.label}
            variant="outline"
            size="sm"
            onClick={() => {
              setSelectedTool(qa.tool)
              setArgsJson(qa.defaultArgs)
            }}
          >
            {qa.label}
          </Button>
        ))}
      </div>

      {/* Tool grid */}
      <div>
        <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Available tools
        </h2>
        {loading ? (
          <p className="text-sm text-muted-foreground">Loading...</p>
        ) : error && tools.length === 0 ? (
          <p className="text-sm text-muted-foreground">{error}</p>
        ) : tools.length === 0 ? (
          <p className="text-sm text-muted-foreground">No tools available.</p>
        ) : (
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {tools.map((t) => (
              <button
                key={t.name}
                type="button"
                onClick={() => {
                  setSelectedTool(t.name)
                  setArgsJson("{}")
                }}
                className={`rounded-xl border p-4 text-left transition-colors ${
                  selectedTool === t.name
                    ? "border-primary/60 bg-muted/30"
                    : "border-border bg-card hover:border-primary/40 hover:bg-muted/20"
                }`}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-medium text-foreground">{t.name}</span>
                  {t.category && (
                    <Badge variant="outline" className="text-[10px]">
                      {t.category}
                    </Badge>
                  )}
                </div>
                <p className="mt-1 text-xs text-muted-foreground line-clamp-2">
                  {t.description || "No description"}
                </p>
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        {/* Execute panel */}
        <Card>
          <CardHeader>
            <CardTitle>Execute tool</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="te-tool">Tool</Label>
              <select
                id="te-tool"
                className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                value={selectedTool}
                onChange={(e) => setSelectedTool(e.target.value)}
              >
                {tools.map((t) => (
                  <option key={t.name} value={t.name}>
                    {t.name}
                  </option>
                ))}
              </select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="te-args">Arguments (JSON)</Label>
              <Textarea
                id="te-args"
                className="font-mono text-xs"
                rows={4}
                value={argsJson}
                onChange={(e) => setArgsJson(e.target.value)}
              />
            </div>

            <Button
              onClick={() => handleExecute()}
              disabled={executing || !selectedTool}
            >
              {executing ? "Running..." : "Execute"}
            </Button>

            {lastResult && (
              <div
                className={`rounded-lg border p-3 text-sm ${
                  lastResult.ok
                    ? "border-emerald-600/30 bg-emerald-950/20"
                    : "border-red-600/30 bg-red-950/20"
                }`}
              >
                <div className="flex items-center justify-between">
                  <span className={lastResult.ok ? "text-emerald-400" : "text-red-400"}>
                    {lastResult.ok ? "Success" : "Error"}
                  </span>
                  {lastResult.duration_ms != null && (
                    <span className="text-xs text-muted-foreground">
                      {lastResult.duration_ms}ms
                    </span>
                  )}
                </div>
                <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap text-xs text-muted-foreground">
                  {String(lastResult.error || lastResult.output || lastResult.message || "(empty)")}
                </pre>
              </div>
            )}
          </CardContent>
        </Card>

        {/* History */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle>Execution history</CardTitle>
            <span className="text-xs text-muted-foreground">{history.length} runs</span>
          </CardHeader>
          <CardContent className="max-h-[500px] space-y-3 overflow-y-auto">
            {history.length === 0 ? (
              <p className="text-sm text-muted-foreground">No executions yet.</p>
            ) : (
              history.map((h, i) => (
                <div key={i} className="rounded-lg border border-border p-3 text-sm">
                  <div className="flex items-center justify-between gap-2">
                    <code className="text-xs text-primary">{h.tool}</code>
                    <Badge
                      variant="outline"
                      className={h.result.ok ? "text-emerald-400" : "text-red-400"}
                    >
                      {h.result.ok ? "ok" : "fail"}
                    </Badge>
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    {new Date(h.timestamp).toLocaleTimeString()}
                  </div>
                  <pre className="mt-1 max-h-20 overflow-auto whitespace-pre-wrap text-xs text-muted-foreground/80">
                    {String(h.result.error || h.result.output || h.result.message || "").slice(0, 200)}
                  </pre>
                </div>
              ))
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
