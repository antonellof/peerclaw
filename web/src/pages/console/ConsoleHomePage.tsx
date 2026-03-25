import { useEffect, useState } from "react"
import { Link, useNavigate } from "react-router-dom"

import { fetchOnboarding } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { SCENARIO_PRESETS } from "@/pages/chat/scenarios"

export function ConsoleHomePage() {
  const nav = useNavigate()
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
    <div className="space-y-8">
      <div className="rounded-2xl border border-border bg-gradient-to-br from-card to-background p-8">
        <h1 className="text-2xl font-semibold tracking-tight md:text-3xl">Turn a goal into an outcome</h1>
        <p className="mt-3 max-w-2xl text-sm leading-relaxed text-muted-foreground">
          Describe what you need. Your <span className="text-violet-400">agent</span> plans steps, runs tools within a{" "}
          <strong>budget</strong>, and returns a structured answer. In <strong>Chat</strong>, switch to{" "}
          <strong>Agent goal</strong> for multi-step work, or stay in <strong>Chat</strong> for quick messages.
        </p>
        <div className="mt-6 flex flex-wrap gap-2">
          <Button onClick={() => nav("/", { state: { openAgent: true } })}>Start with Agent</Button>
          <Button variant="outline" asChild>
            <Link to="/">Open Assistant</Link>
          </Button>
          <Button variant="secondary" onClick={() => nav({ pathname: "/", search: "?view=overview", hash: "health" })}>
            Node overview
          </Button>
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Node readiness</CardTitle>
          <CardDescription>From <code className="text-foreground">/api/onboarding</code></CardDescription>
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
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Starters</h2>
        <p className="mt-1 text-sm text-muted-foreground">Opens Chat in Agent goal mode — edit the prompt and send.</p>
        <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
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
          <CardTitle>Requirements</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2 text-sm text-muted-foreground">
          <p>
            <strong className="text-foreground">Agent</strong> needs a node with an agent spec, for example:
          </p>
          <code className="block rounded-lg bg-muted px-3 py-2 font-mono text-xs text-primary">
            peerclaw serve --web 127.0.0.1:8080 --agent examples/agents/assistant.toml
          </code>
        </CardContent>
      </Card>
    </div>
  )
}
