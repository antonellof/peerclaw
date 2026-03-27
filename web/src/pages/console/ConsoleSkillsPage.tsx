import { useCallback, useEffect, useState } from "react"
import { Download, Power, PowerOff } from "lucide-react"

import {
  fetchMcpStatus,
  fetchSkillsLocal,
  fetchSkillsMeta,
  fetchSkillsNetwork,
  fetchSkillTemplates,
  scanSkills,
  toggleSkill,
  type McpStatusResponse,
  type SkillInfo,
  type SkillsMetaResponse,
  type SkillTemplate,
} from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Separator } from "@/components/ui/separator"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { SkillStudioPanel } from "@/pages/console/SkillStudioPanel"

function trustColor(trust: string): string {
  switch (trust) {
    case "local":
      return "bg-emerald-500/15 text-emerald-400 border-emerald-500/30"
    case "installed":
      return "bg-blue-500/15 text-blue-400 border-blue-500/30"
    case "network":
      return "bg-amber-500/15 text-amber-400 border-amber-500/30"
    default:
      return "bg-muted text-muted-foreground"
  }
}

function SkillCard({
  skill,
  onToggle,
}: {
  skill: SkillInfo
  onToggle: (name: string) => void
}) {
  return (
    <div
      className={`rounded-lg border p-4 text-sm transition-colors ${
        skill.enabled ? "border-border bg-card" : "border-border/50 bg-muted/30 opacity-60"
      }`}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="font-semibold">{skill.name}</span>
            <Badge variant="outline" className={`text-[10px] px-1.5 py-0 ${trustColor(skill.trust)}`}>
              {skill.trust}
            </Badge>
            <span className="text-[10px] text-muted-foreground">v{skill.version}</span>
          </div>
          <p className="mt-1 text-xs text-muted-foreground leading-relaxed">{skill.description}</p>
          {(skill.keywords.length > 0 || skill.tags.length > 0) && (
            <div className="mt-2 flex flex-wrap gap-1">
              {skill.keywords.map((kw) => (
                <Badge key={kw} variant="secondary" className="text-[10px] px-1.5 py-0 font-normal">
                  {kw}
                </Badge>
              ))}
              {skill.tags.map((tag) => (
                <Badge key={tag} variant="outline" className="text-[10px] px-1.5 py-0 font-normal text-muted-foreground">
                  #{tag}
                </Badge>
              ))}
            </div>
          )}
        </div>
        <Button
          variant="ghost"
          size="sm"
          className={`h-8 w-8 p-0 shrink-0 ${skill.enabled ? "text-emerald-400 hover:text-emerald-300" : "text-muted-foreground hover:text-foreground"}`}
          title={skill.enabled ? "Disable skill" : "Enable skill"}
          onClick={() => onToggle(skill.name)}
        >
          {skill.enabled ? <Power className="h-4 w-4" /> : <PowerOff className="h-4 w-4" />}
        </Button>
      </div>
    </div>
  )
}

function TemplateCard({
  template,
  installed,
  onInstall,
}: {
  template: SkillTemplate
  installed: boolean
  onInstall: (tpl: SkillTemplate) => void
}) {
  return (
    <div className="rounded-lg border border-border p-4 text-sm">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="font-semibold">{template.name}</span>
            <span className="text-[10px] text-muted-foreground">v{template.version}</span>
            {template.author && (
              <span className="text-[10px] text-muted-foreground">by {template.author}</span>
            )}
          </div>
          <p className="mt-1 text-xs text-muted-foreground leading-relaxed">{template.description}</p>
          {(template.keywords.length > 0 || template.tags.length > 0) && (
            <div className="mt-2 flex flex-wrap gap-1">
              {template.keywords.map((kw) => (
                <Badge key={kw} variant="secondary" className="text-[10px] px-1.5 py-0 font-normal">
                  {kw}
                </Badge>
              ))}
              {template.tags.map((tag) => (
                <Badge key={tag} variant="outline" className="text-[10px] px-1.5 py-0 font-normal text-muted-foreground">
                  #{tag}
                </Badge>
              ))}
            </div>
          )}
        </div>
        <Button
          variant={installed ? "ghost" : "secondary"}
          size="sm"
          className="shrink-0"
          disabled={installed}
          onClick={() => onInstall(template)}
        >
          {installed ? (
            "Installed"
          ) : (
            <>
              <Download className="mr-1 h-3 w-3" />
              Install
            </>
          )}
        </Button>
      </div>
    </div>
  )
}

export function ConsoleSkillsPage() {
  const [meta, setMeta] = useState<SkillsMetaResponse | null>(null)
  const [local, setLocal] = useState<SkillInfo[]>([])
  const [network, setNetwork] = useState<SkillInfo[]>([])
  const [templates, setTemplates] = useState<SkillTemplate[]>([])
  const [mcp, setMcp] = useState<McpStatusResponse | null>(null)
  const [scanMsg, setScanMsg] = useState<{ ok: boolean; text: string } | null>(null)

  const load = useCallback(async () => {
    try {
      const [mr, lr, nr, mc, tpl] = await Promise.all([
        fetchSkillsMeta(),
        fetchSkillsLocal(),
        fetchSkillsNetwork(),
        fetchMcpStatus(),
        fetchSkillTemplates(),
      ])
      setMeta(mr)
      setLocal(lr)
      setNetwork(nr)
      setMcp(mc)
      setTemplates(tpl)
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

  const onToggle = async (name: string) => {
    const r = await toggleSkill(name)
    if (r.ok) {
      // Update local state optimistically
      setLocal((prev) =>
        prev.map((s) => (s.name === name ? { ...s, enabled: r.enabled ?? !s.enabled } : s)),
      )
      setNetwork((prev) =>
        prev.map((s) => (s.name === name ? { ...s, enabled: r.enabled ?? !s.enabled } : s)),
      )
    }
  }

  const onInstallTemplate = async (tpl: SkillTemplate) => {
    try {
      const { saveSkillStudio } = await import("@/lib/api")
      const result = await saveSkillStudio(tpl.name, tpl.content)
      if (result.ok) {
        setScanMsg({ ok: true, text: `Installed "${tpl.name}" skill.` })
        await scanSkills()
        await load()
      } else {
        setScanMsg({ ok: false, text: "Install failed." })
      }
    } catch (e) {
      setScanMsg({ ok: false, text: `Install error: ${e instanceof Error ? e.message : String(e)}` })
    }
  }

  const installedNames = new Set([...local.map((s) => s.name), ...network.map((s) => s.name)])

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
          <TabsTrigger value="templates">
            Templates
            {templates.length > 0 && (
              <span className="ml-1.5 rounded-full bg-primary/15 px-1.5 text-[10px] font-medium text-primary">
                {templates.length}
              </span>
            )}
          </TabsTrigger>
          <TabsTrigger value="studio">Skill studio</TabsTrigger>
        </TabsList>

        {/* ---- Library tab ---- */}
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
                    <code className="mt-1 block rounded-md bg-muted p-2 font-mono text-xs break-all">
                      {meta.skills_dir}
                    </code>
                  </div>
                  <div>
                    <div className="text-xs text-muted-foreground">Config</div>
                    <code className="mt-1 block rounded-md bg-muted p-2 font-mono text-xs break-all">
                      {meta.config_path}
                    </code>
                  </div>
                  {!meta.registry_attached && (
                    <p className="text-xs text-amber-400">
                      Registry not attached -- use full `peerclaw serve --web`.
                    </p>
                  )}
                  <pre className="max-h-40 overflow-auto rounded-md border border-border bg-muted/30 p-3 text-xs text-muted-foreground whitespace-pre-wrap">
                    {meta.directory_toml_snippet}
                  </pre>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={!meta.registry_attached}
                    onClick={() => void onScan()}
                  >
                    Rescan disk
                  </Button>
                  {scanMsg && (
                    <p className={scanMsg.ok ? "text-xs text-emerald-400" : "text-xs text-destructive"}>
                      {scanMsg.text}
                    </p>
                  )}
                </>
              )}
            </CardContent>
          </Card>

          <div className="grid gap-6 lg:grid-cols-2">
            <Card>
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle>Local &amp; installed</CardTitle>
                  {local.length > 0 && (
                    <span className="text-xs text-muted-foreground">
                      {local.filter((s) => s.enabled).length}/{local.length} active
                    </span>
                  )}
                </div>
              </CardHeader>
              <CardContent className="space-y-3">
                {local.length === 0 ? (
                  <p className="text-sm text-muted-foreground">
                    No local skills. Check the <strong>Templates</strong> tab for bundled skills you can install.
                  </p>
                ) : (
                  local.map((s) => <SkillCard key={s.name} skill={s} onToggle={onToggle} />)
                )}
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle>From the network</CardTitle>
                  {network.length > 0 && (
                    <span className="text-xs text-muted-foreground">
                      {network.filter((s) => s.enabled).length}/{network.length} active
                    </span>
                  )}
                </div>
              </CardHeader>
              <CardContent className="space-y-3">
                {network.length === 0 ? (
                  <p className="text-sm text-muted-foreground">No P2P skills yet.</p>
                ) : (
                  network.map((s) => (
                    <SkillCard key={s.name + s.provider} skill={s} onToggle={onToggle} />
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
                  <a
                    href={mcp.spec_url}
                    className="text-xs text-primary hover:underline"
                    target="_blank"
                    rel="noreferrer"
                  >
                    MCP specification
                  </a>
                </>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        {/* ---- Templates tab ---- */}
        <TabsContent value="templates" className="mt-6 space-y-6 focus-visible:outline-none">
          <Card>
            <CardHeader>
              <CardTitle>Bundled skill templates</CardTitle>
              <CardDescription>
                Pre-built SKILL.md files that ship with PeerClaw. Install them into your skills directory to activate.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              {templates.length === 0 ? (
                <p className="text-sm text-muted-foreground">
                  No bundled templates found. Make sure the examples/skills/ directory is present.
                </p>
              ) : (
                templates.map((tpl) => (
                  <TemplateCard
                    key={tpl.name}
                    template={tpl}
                    installed={installedNames.has(tpl.name)}
                    onInstall={onInstallTemplate}
                  />
                ))
              )}
              {scanMsg && (
                <p className={scanMsg.ok ? "text-xs text-emerald-400" : "text-xs text-destructive"}>
                  {scanMsg.text}
                </p>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        {/* ---- Studio tab ---- */}
        <TabsContent value="studio" className="mt-6 focus-visible:outline-none">
          <SkillStudioPanel
            onSaved={() => {
              void load()
              setScanMsg({ ok: true, text: "Skill saved -- library refreshed." })
            }}
          />
        </TabsContent>
      </Tabs>
    </div>
  )
}
