import { useEffect, useState } from "react"
import { useNavigate } from "react-router-dom"
import { BookOpen, Settings2 } from "lucide-react"

import { fetchOnboarding } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { SCENARIO_PRESETS } from "@/pages/chat/scenarios"
import { useWorkspaceNav } from "@/workspace/WorkspaceNavContext"

/**
 * Full-page help: getting started, chat vs agents, node readiness, starters, and CLI — single source (no Home duplicate).
 */
export function ConsoleHelpPage() {
  const nav = useNavigate()
  const { setView, openSettings } = useWorkspaceNav()
  const [steps, setSteps] = useState<{ id: string; ok: boolean; detail: string }[]>([])

  useEffect(() => {
    void (async () => {
      try {
        const o = await fetchOnboarding()
        setSteps(o.steps)
      } catch {
        setSteps([])
      }
    })()
  }, [])

  const startScenario = (key: string) => {
    const p = SCENARIO_PRESETS[key]
    if (!p) return
    nav("/", { state: { agentPreset: { taskType: p.type, text: p.text } } })
  }

  return (
    <div className="mx-auto max-w-3xl space-y-8 pb-8">
      <div className="rounded-2xl border border-border bg-gradient-to-br from-card to-background p-6 md:p-8">
        <div className="flex flex-wrap items-start gap-3">
          <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/15">
            <BookOpen className="size-5 text-primary" />
          </div>
          <div className="min-w-0 flex-1">
            <h1 className="text-xl font-semibold tracking-tight md:text-2xl">Help &amp; getting started</h1>
            <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
              The app opens on <strong className="text-foreground">Chat</strong>. Describe a goal and the node runs tools
              within a <strong className="text-foreground">budget</strong>. Use <strong className="text-foreground">Saved agent</strong>{" "}
              for flows from Agent builder, <strong className="text-foreground">Agent goal</strong> for ad-hoc multi-step
              tasks, or stay in streaming chat. Crews and flows API:{" "}
              <code className="text-foreground/90">/api/crews</code>, <code className="text-foreground/90">/api/flows</code>.
            </p>
            <div className="mt-4 flex flex-wrap gap-2">
              <Button size="sm" onClick={() => setView("chat")}>
                Back to Chat
              </Button>
              <Button size="sm" variant="outline" onClick={() => openSettings()}>
                <Settings2 className="mr-1.5 size-3.5" />
                Settings
              </Button>
            </div>
          </div>
        </div>
      </div>

      <section className="space-y-3 text-sm text-muted-foreground">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-foreground">Chat vs agents</h2>
        <ul className="list-inside list-disc space-y-2 text-xs leading-relaxed">
          <li>
            <strong className="text-foreground">Chat</strong> — streaming assistant; session memory when a session id is set.
          </li>
          <li>
            <strong className="text-foreground">Saved agent</strong> — run a stored flow (kickoff) or a task preset from the
            node library (<code className="text-foreground/80">agent_library.json</code>).
          </li>
          <li>
            <strong className="text-foreground">Agent goal</strong> — one-off task; logs and results stay in the thread. Past
            runs appear under <strong className="text-foreground">Agent runs</strong> in the sidebar.
          </li>
        </ul>
        <p className="text-xs leading-relaxed">
          Type <kbd className="rounded border border-border px-1 font-mono">/</kbd> for slash commands (full list under{" "}
          <strong className="text-foreground">Settings</strong> → Commands). Sidebar: <strong className="text-foreground">P2P Network</strong>,{" "}
          <strong className="text-foreground">Agent builder</strong>, and other console panels.
        </p>
      </section>

      <section className="space-y-2">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-foreground">Run the node with an agent spec</h2>
        <p className="text-xs text-muted-foreground">
          So <strong className="text-foreground">chat tools</strong> and workflows work end-to-end:
        </p>
        <pre className="max-w-full overflow-x-auto whitespace-pre-wrap break-all rounded-lg border border-border bg-muted/50 p-3 font-mono text-[10px] leading-relaxed text-primary">
          peerclaw serve --web 127.0.0.1:8080 --agent templates/agents/assistant.toml
        </pre>
      </section>

      <Card>
        <CardHeader>
          <CardTitle>Node readiness</CardTitle>
          <CardDescription>
            From <code className="text-foreground">/api/onboarding</code> — same chips as the sidebar.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {steps.length === 0 ? (
            <p className="text-sm text-muted-foreground">Loading…</p>
          ) : (
            steps.map((s) => (
              <div key={s.id} className="flex gap-3 rounded-lg border border-border bg-muted/20 px-3 py-2 text-sm">
                <span className={s.ok ? "text-emerald-400" : "text-amber-400"}>{s.ok ? "✓" : "○"}</span>
                <div>
                  <div className="font-medium capitalize">{s.id.replace(/_/g, " ")}</div>
                  <div className="text-xs text-muted-foreground">{s.detail}</div>
                </div>
              </div>
            ))
          )}
        </CardContent>
      </Card>

      <div>
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Shortcuts</h2>
        <p className="mt-1 text-sm text-muted-foreground">Jump to common console destinations.</p>
        <div className="mt-4 flex flex-wrap gap-2">
          <Button variant="outline" size="sm" onClick={() => nav("/", { state: { openAgent: true } })}>
            Start with Agent
          </Button>
          <Button variant="outline" size="sm" onClick={() => setView("workflows")}>
            Workflows
          </Button>
          <Button variant="secondary" size="sm" onClick={() => setView("overview", "join-mesh")}>
            Join the mesh
          </Button>
          <Button variant="secondary" size="sm" onClick={() => setView("overview", "health")}>
            Node overview
          </Button>
        </div>
      </div>

      <div>
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Starters</h2>
        <p className="mt-1 text-sm text-muted-foreground">Opens Chat in Agent goal mode — edit the prompt and send.</p>
        <div className="mt-4 grid gap-3 sm:grid-cols-2">
          {(
            [
              ["trip", "Trip planning", "Weekend itinerary & dining"],
              ["email", "Email or doc", "Draft messages"],
              ["research", "Work research", "Summaries & tradeoffs"],
              ["data", "Data & analysis", "Interpret pasted data"],
              ["bugfix", "Code & bugs", "Review errors"],
              ["meeting", "Meeting prep", "Agenda & talking points"],
            ] as const
          ).map(([key, title, desc]) => (
            <button
              key={key}
              type="button"
              onClick={() => startScenario(key)}
              className="rounded-xl border border-border bg-card p-4 text-left transition-colors hover:border-primary/40 hover:bg-muted/30"
            >
              <h3 className="font-medium">{title}</h3>
              <p className="mt-1 text-xs text-muted-foreground">{desc}</p>
            </button>
          ))}
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Distributed mode</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3 text-sm text-muted-foreground">
          <p>
            Other peers can share inference (<code className="text-foreground/90">--share-inference</code>) or claim crew
            steps (<code className="text-foreground/90">--crew-worker</code>). Use <strong className="text-foreground">P2P Network</strong>{" "}
            → <strong className="text-foreground">Join the mesh</strong> for commands and stats.
          </p>
        </CardContent>
      </Card>
    </div>
  )
}
