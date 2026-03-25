import { useCallback, useEffect, useState } from "react"
import { RefreshCw } from "lucide-react"

import {
  fetchMcpStatus,
  putMcpConfig,
  type McpConfigJson,
  type McpStatusResponse,
} from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

const EXAMPLE_JSON: McpConfigJson = {
  enabled: true,
  timeout_secs: 30,
  auto_reconnect: true,
  servers: [
    {
      name: "fs",
      url: "stdio://local",
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
      env: {},
    },
  ],
}

function statusToJson(s: McpStatusResponse | null): string {
  if (!s) return JSON.stringify(EXAMPLE_JSON, null, 2)
  const cfg: McpConfigJson = {
    enabled: s.config.enabled,
    timeout_secs: s.config.timeout_secs,
    auto_reconnect: s.config.auto_reconnect,
    servers: s.config.servers.map((x) => ({
      name: x.name,
      url: x.url,
      env: x.env ?? {},
      command: x.command ?? undefined,
      args: x.args ?? [],
    })),
  }
  return JSON.stringify(cfg, null, 2)
}

export function ConsoleMcpPage() {
  const [status, setStatus] = useState<McpStatusResponse | null>(null)
  const [jsonText, setJsonText] = useState("")
  const [parseErr, setParseErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [saveMsg, setSaveMsg] = useState<string | null>(null)

  const load = useCallback(async () => {
    setBusy(true)
    setSaveMsg(null)
    try {
      const s = await fetchMcpStatus()
      setStatus(s)
      setJsonText(statusToJson(s))
      setParseErr(null)
    } catch (e) {
      setSaveMsg(e instanceof Error ? e.message : "Failed to load MCP status")
    } finally {
      setBusy(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const apply = async () => {
    setSaveMsg(null)
    setParseErr(null)
    let cfg: McpConfigJson
    try {
      cfg = JSON.parse(jsonText) as McpConfigJson
    } catch (e) {
      setParseErr(e instanceof Error ? e.message : "Invalid JSON")
      return
    }
    if (typeof cfg.enabled !== "boolean" || !Array.isArray(cfg.servers)) {
      setParseErr("JSON must include `enabled` (boolean) and `servers` (array).")
      return
    }
    setBusy(true)
    try {
      await putMcpConfig(cfg)
      setSaveMsg("Applied. Servers reconnected in the background.")
      await load()
    } catch (e) {
      setSaveMsg(e instanceof Error ? e.message : "Save failed")
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div>
        <h2 className="text-lg font-semibold tracking-tight">MCP servers</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          Edit JSON matching the node&apos;s <code className="text-foreground">McpConfig</code> shape, then Apply.
          Stdio servers need <code className="text-foreground">command</code> and <code className="text-foreground">args</code>
          ; HTTP MCP transport is not enabled in the client yet. Turn on <strong className="text-foreground">MCP</strong> in
          chat or agent to run the tool loop. Apply updates this process only — merge the same{" "}
          <code className="text-foreground">[mcp]</code> block into <code className="text-foreground">config.toml</code> if
          you want it after restart.
        </p>
        <p className="mt-2 text-xs text-muted-foreground">
          Config file (for persistence across restarts):{" "}
          <code className="break-all text-foreground">{status?.config_path ?? "…"}</code>
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => void load()}>
          <RefreshCw className={cn("mr-1 size-3.5", busy && "animate-spin")} />
          Refresh status
        </Button>
        <Button type="button" size="sm" disabled={busy} onClick={() => void apply()}>
          Apply &amp; reconnect
        </Button>
        {saveMsg && <span className="text-xs text-muted-foreground">{saveMsg}</span>}
      </div>

      {status && (
        <div className="rounded-xl border border-border/80 bg-card/40 p-4 text-sm">
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
            <span>
              Connected servers:{" "}
              <span className="font-medium text-foreground">
                {status.connected_servers?.length ? status.connected_servers.join(", ") : "—"}
              </span>
            </span>
            <span>
              Tools: <span className="font-medium text-foreground">{status.tool_count ?? 0}</span>
            </span>
          </div>
          {status.tools && status.tools.length > 0 && (
            <ul className="mt-3 max-h-40 space-y-1 overflow-y-auto text-xs">
              {status.tools.map((t) => (
                <li key={t.id} className="font-mono text-primary">
                  {t.id}
                  {t.description ? (
                    <span className="ml-2 font-sans text-muted-foreground">{t.description}</span>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      <div className="space-y-2">
        <Label className="text-xs">Configuration JSON</Label>
        <Textarea
          value={jsonText}
          onChange={(e) => setJsonText(e.target.value)}
          spellCheck={false}
          className="min-h-[280px] font-mono text-xs leading-relaxed"
          placeholder="Paste or edit MCP config JSON…"
        />
        {parseErr && <p className="text-xs text-destructive">{parseErr}</p>}
      </div>

      <div className="rounded-xl border border-border/60 bg-muted/20 p-4">
        <p className="mb-2 text-xs font-medium text-muted-foreground">Example TOML (reference — use JSON above for the UI)</p>
        <pre className="overflow-x-auto whitespace-pre-wrap break-words font-mono text-[10px] leading-relaxed text-muted-foreground">
          {status?.mcp_toml_snippet ?? ""}
        </pre>
      </div>
    </div>
  )
}
