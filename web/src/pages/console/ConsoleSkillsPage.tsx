import { useCallback, useEffect, useState } from "react"

import {
  fetchMcpStatus,
  fetchSkillsLocal,
  fetchSkillsMeta,
  fetchSkillsNetwork,
  scanSkills,
  type McpStatusResponse,
  type SkillInfo,
  type SkillsMetaResponse,
} from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Separator } from "@/components/ui/separator"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { SkillStudioPanel } from "@/pages/console/SkillStudioPanel"

export function ConsoleSkillsPage() {
  const [meta, setMeta] = useState<SkillsMetaResponse | null>(null)
  const [local, setLocal] = useState<SkillInfo[]>([])
  const [network, setNetwork] = useState<SkillInfo[]>([])
  const [mcp, setMcp] = useState<McpStatusResponse | null>(null)
  const [scanMsg, setScanMsg] = useState<{ ok: boolean; text: string } | null>(null)

  const load = useCallback(async () => {
    try {
      const [mr, lr, nr, mc] = await Promise.all([
        fetchSkillsMeta(),
        fetchSkillsLocal(),
        fetchSkillsNetwork(),
        fetchMcpStatus(),
      ])
      setMeta(mr)
      setLocal(lr)
      setNetwork(nr)
      setMcp(mc)
    } catch {
      /* ignore */
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const onScan = async () => {
    setScanMsg(null)
    const r = await scanSkills()
    if (r.ok) {
      setScanMsg({ ok: true, text: `Loaded ${r.loaded ?? 0} skill(s).` })
      await load()
    } else {
      setScanMsg({ ok: false, text: r.error ?? "Scan failed" })
    }
  }

  return (
    <div className="space-y-6">
      <p className="text-sm text-muted-foreground">
        Add <code className="text-foreground">SKILL.md</code> under the skills directory or set{" "}
        <code className="text-foreground">[skills].directory</code> in config. Use the{" "}
        <strong className="font-medium text-foreground">Skill studio</strong> tab to author files with AI assist. MCP
        servers live under <code className="text-foreground">[mcp]</code>.
      </p>

      <Tabs defaultValue="library" className="w-full">
        <TabsList>
          <TabsTrigger value="library">Library</TabsTrigger>
          <TabsTrigger value="studio">Skill studio</TabsTrigger>
        </TabsList>
        <TabsContent value="library" className="mt-6 space-y-6 focus-visible:outline-none">
      <Card>
        <CardHeader>
          <CardTitle>Paths &amp; rescan</CardTitle>
          <CardDescription>CLI: peerclaw skill scan</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          {meta && (
            <>
              <div>
                <div className="text-xs text-muted-foreground">Skills directory</div>
                <code className="mt-1 block rounded-md bg-muted p-2 font-mono text-xs break-all">{meta.skills_dir}</code>
              </div>
              <div>
                <div className="text-xs text-muted-foreground">Config</div>
                <code className="mt-1 block rounded-md bg-muted p-2 font-mono text-xs break-all">{meta.config_path}</code>
              </div>
              {!meta.registry_attached && (
                <p className="text-xs text-amber-400">Registry not attached — use full `peerclaw serve --web`.</p>
              )}
              <pre className="max-h-40 overflow-auto rounded-md border border-border bg-muted/30 p-3 text-xs text-muted-foreground whitespace-pre-wrap">
                {meta.directory_toml_snippet}
              </pre>
              <Button variant="secondary" size="sm" disabled={!meta.registry_attached} onClick={() => void onScan()}>
                Rescan disk
              </Button>
              {scanMsg && (
                <p className={scanMsg.ok ? "text-xs text-emerald-400" : "text-xs text-destructive"}>{scanMsg.text}</p>
              )}
            </>
          )}
        </CardContent>
      </Card>

      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Local &amp; installed</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {local.length === 0 ? (
              <p className="text-sm text-muted-foreground">No local skills.</p>
            ) : (
              local.map((s) => (
                <div key={s.name} className="rounded-lg border border-border p-3 text-sm">
                  <div className="flex justify-between gap-2">
                    <span className="font-medium">{s.name}</span>
                    <span className="text-xs text-muted-foreground">{s.trust}</span>
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">{s.description}</p>
                </div>
              ))
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>From the network</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {network.length === 0 ? (
              <p className="text-sm text-muted-foreground">No P2P skills yet.</p>
            ) : (
              network.map((s) => (
                <div key={s.name + s.provider} className="rounded-lg border border-border p-3 text-sm">
                  <div className="font-medium">{s.name}</div>
                  <p className="text-xs text-muted-foreground">{s.description}</p>
                </div>
              ))
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>MCP bridge</CardTitle>
          <CardDescription>{mcp?.config_path}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          {mcp && (
            <>
              <div className="flex flex-wrap gap-3 text-xs text-muted-foreground">
                <span>mode: {mcp.mode}</span>
                <span>servers: {mcp.config.servers.length}</span>
                <span>tools: {mcp.tool_count ?? 0}</span>
                <span>timeout: {mcp.config.timeout_secs}s</span>
              </div>
              <p className="text-xs text-muted-foreground">{mcp.hint}</p>
              <Separator />
              <pre className="max-h-48 overflow-auto rounded-md border border-border bg-muted/30 p-3 text-xs whitespace-pre-wrap">
                {mcp.mcp_toml_snippet}
              </pre>
              <a href={mcp.spec_url} className="text-xs text-primary hover:underline" target="_blank" rel="noreferrer">
                MCP specification →
              </a>
            </>
          )}
        </CardContent>
      </Card>
        </TabsContent>
        <TabsContent value="studio" className="mt-6 focus-visible:outline-none">
          <SkillStudioPanel
            onSaved={() => {
              void load()
              setScanMsg({ ok: true, text: "Skill saved — library refreshed." })
            }}
          />
        </TabsContent>
      </Tabs>
    </div>
  )
}
