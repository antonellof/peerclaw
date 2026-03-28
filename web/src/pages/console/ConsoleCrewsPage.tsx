import { useCallback, useEffect, useMemo, useState } from "react"
import { Loader2, Plus, Trash2 } from "lucide-react"

import {
  fetchCrewRun,
  fetchCrewRuns,
  kickoffCrew,
  stopCrewRun,
  validateCrew,
  type CrewRunRecordJson,
  type CrewSpecJson,
} from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

type AgentRow = {
  id: string
  role: string
  goal: string
  backstory: string
  llm: string
  toolsStr: string
  max_iter: string
}

type TaskRow = {
  id: string
  description: string
  expected_output: string
  agent_id: string
  contextStr: string
}

function newAgent(partial?: Partial<AgentRow>): AgentRow {
  return {
    id: partial?.id ?? `agent_${Math.random().toString(36).slice(2, 7)}`,
    role: partial?.role ?? "",
    goal: partial?.goal ?? "",
    backstory: partial?.backstory ?? "",
    llm: partial?.llm ?? "llama3.2:3b",
    toolsStr: partial?.toolsStr ?? "",
    max_iter: partial?.max_iter ?? "8",
  }
}

function newTask(partial?: Partial<TaskRow>): TaskRow {
  return {
    id: partial?.id ?? `task_${Math.random().toString(36).slice(2, 7)}`,
    description: partial?.description ?? "",
    expected_output: partial?.expected_output ?? "",
    agent_id: partial?.agent_id ?? "",
    contextStr: partial?.contextStr ?? "",
  }
}

function buildSpec(
  name: string,
  process: "sequential" | "hierarchical",
  managerId: string,
  planning: boolean,
  agents: AgentRow[],
  tasks: TaskRow[],
): CrewSpecJson {
  return {
    name,
    process,
    manager_agent_id: process === "hierarchical" && managerId.trim() ? managerId.trim() : undefined,
    planning,
    agents: agents.map((a) => ({
      id: a.id.trim(),
      role: a.role.trim(),
      goal: a.goal.trim(),
      backstory: a.backstory.trim(),
      llm: a.llm.trim() || "llama3.2:3b",
      tools: a.toolsStr
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
      max_iter: Math.max(0, parseInt(a.max_iter, 10) || 0),
    })),
    tasks: tasks.map((t) => ({
      id: t.id.trim(),
      description: t.description.trim(),
      expected_output: t.expected_output.trim(),
      agent_id: t.agent_id.trim(),
      context: t.contextStr
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
    })),
  }
}

const DEMO_INPUTS_JSON = '{\n  "topic": "Rust async runtimes"\n}'

function initialDemoAgents(): AgentRow[] {
  return [
    newAgent({
      id: "researcher",
      role: "Researcher",
      goal: "Gather accurate facts on the user topic",
      backstory: "You prefer primary sources and cite uncertainties.",
      llm: "llama3.2:3b",
      max_iter: "8",
    }),
    newAgent({
      id: "writer",
      role: "Writer",
      goal: "Produce a concise summary for a general audience",
      backstory: "You write clear, structured prose.",
      llm: "llama3.2:3b",
      max_iter: "8",
    }),
  ]
}

function initialDemoTasks(): TaskRow[] {
  return [
    newTask({
      id: "t1",
      description: "Research the topic: {{topic}}. List key facts as bullets.",
      expected_output: "Bullet list of facts",
      agent_id: "researcher",
    }),
    newTask({
      id: "t2",
      description: "Using the research above, write one short paragraph summary.",
      expected_output: "One paragraph",
      agent_id: "writer",
      contextStr: "t1",
    }),
  ]
}

export function ConsoleCrewsPage() {
  const [crewName, setCrewName] = useState("Research crew")
  const [process, setProcess] = useState<"sequential" | "hierarchical">("sequential")
  const [managerId, setManagerId] = useState("")
  const [planning, setPlanning] = useState(false)
  const [agents, setAgents] = useState<AgentRow[]>(() => initialDemoAgents())
  const [tasks, setTasks] = useState<TaskRow[]>(() => initialDemoTasks())
  const [inputsJson, setInputsJson] = useState(DEMO_INPUTS_JSON)
  const [distributed, setDistributed] = useState(false)
  const [podId, setPodId] = useState("")
  const [campaignId, setCampaignId] = useState("")

  const [validateMsg, setValidateMsg] = useState<string | null>(null)
  const [kickoffMsg, setKickoffMsg] = useState<string | null>(null)
  const [busy, setBusy] = useState<"validate" | "kickoff" | null>(null)

  const [runs, setRuns] = useState<CrewRunRecordJson[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<CrewRunRecordJson | null>(null)

  const spec = useMemo(
    () => buildSpec(crewName, process, managerId, planning, agents, tasks),
    [crewName, process, managerId, planning, agents, tasks],
  )

  const loadRuns = useCallback(async () => {
    try {
      const list = await fetchCrewRuns()
      setRuns(list)
      if (selectedId) {
        const d = await fetchCrewRun(selectedId)
        setDetail(d)
      }
    } catch {
      setRuns([])
    }
  }, [selectedId])

  useEffect(() => {
    void loadRuns()
    const t = setInterval(() => void loadRuns(), 2500)
    return () => clearInterval(t)
  }, [loadRuns])

  useEffect(() => {
    if (!selectedId) {
      setDetail(null)
      return
    }
    void (async () => {
      setDetail(await fetchCrewRun(selectedId))
    })()
  }, [selectedId])

  const parseInputs = (): unknown => {
    const raw = inputsJson.trim()
    if (!raw) return {}
    try {
      return JSON.parse(raw) as unknown
    } catch {
      throw new Error("Inputs must be valid JSON (object).")
    }
  }

  const onValidate = async () => {
    setBusy("validate")
    setValidateMsg(null)
    setKickoffMsg(null)
    try {
      const r = await validateCrew(spec)
      setValidateMsg(r.ok ? "Spec is valid." : r.error ?? "Invalid spec.")
    } catch (e) {
      setValidateMsg(e instanceof Error ? e.message : "Validation request failed.")
    } finally {
      setBusy(null)
    }
  }

  const onKickoff = async () => {
    setBusy("kickoff")
    setKickoffMsg(null)
    try {
      let inputs: unknown
      try {
        inputs = parseInputs()
      } catch (e) {
        setKickoffMsg(e instanceof Error ? e.message : "Bad JSON")
        setBusy(null)
        return
      }
      const res = await kickoffCrew({
        spec,
        inputs,
        distributed,
        pod_id: podId.trim() || null,
        campaign_id: campaignId.trim() || null,
      })
      if (res.success && res.run_id) {
        setKickoffMsg(`Started run ${res.run_id}`)
        setSelectedId(res.run_id)
        void loadRuns()
      } else {
        setKickoffMsg(res.error ?? "Kickoff failed.")
      }
    } catch (e) {
      setKickoffMsg(e instanceof Error ? e.message : "Kickoff request failed.")
    } finally {
      setBusy(null)
    }
  }

  const loadDemo = () => {
    setCrewName("Research crew")
    setProcess("sequential")
    setManagerId("")
    setPlanning(false)
    setAgents(initialDemoAgents())
    setTasks(initialDemoTasks())
    setInputsJson(DEMO_INPUTS_JSON)
    setValidateMsg(null)
    setKickoffMsg(null)
  }

  const agentOptions = agents.map((a) => a.id).filter(Boolean)

  return (
    <div className="mx-auto max-w-4xl space-y-6 pb-10">
      <div>
        <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">Orchestration</p>
        <h2 className="mt-1 text-lg font-semibold tracking-tight">Crew builder</h2>
        <p className="mt-2 text-sm text-muted-foreground">
          Define agents and tasks to match the node&apos;s <code className="text-foreground/80">CrewSpec</code>. Task text
          can use <code className="text-foreground/80">{"{{key}}"}</code> placeholders filled from the JSON inputs object.
        </p>
      </div>

      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Crew</CardTitle>
          <CardDescription>Name, process, and optional hierarchical manager.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="crew-name">Name</Label>
            <Input id="crew-name" value={crewName} onChange={(e) => setCrewName(e.target.value)} placeholder="My crew" />
          </div>
          <div className="flex flex-wrap gap-4">
            <div className="space-y-2">
              <Label>Process</Label>
              <div className="flex gap-4 text-sm">
                <label className="flex cursor-pointer items-center gap-2">
                  <input
                    type="radio"
                    name="crew-process"
                    checked={process === "sequential"}
                    onChange={() => setProcess("sequential")}
                  />
                  Sequential
                </label>
                <label className="flex cursor-pointer items-center gap-2">
                  <input
                    type="radio"
                    name="crew-process"
                    checked={process === "hierarchical"}
                    onChange={() => setProcess("hierarchical")}
                  />
                  Hierarchical
                </label>
              </div>
            </div>
            <label className="flex cursor-pointer items-center gap-2 text-sm">
              <input type="checkbox" checked={planning} onChange={(e) => setPlanning(e.target.checked)} />
              Planning pass
            </label>
          </div>
          {process === "hierarchical" ? (
            <div className="space-y-1.5">
              <Label htmlFor="mgr-id">Manager agent id</Label>
              <Input
                id="mgr-id"
                value={managerId}
                onChange={(e) => setManagerId(e.target.value)}
                placeholder="must match an agent id"
                className="font-mono text-xs"
              />
            </div>
          ) : null}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2 pb-3">
          <div>
            <CardTitle className="text-base">Agents</CardTitle>
            <CardDescription>Role, goal, model id, tools (comma-separated), max iterations.</CardDescription>
          </div>
          <Button type="button" variant="outline" size="sm" className="gap-1" onClick={() => setAgents((a) => [...a, newAgent()])}>
            <Plus className="size-3.5" />
            Add agent
          </Button>
        </CardHeader>
        <CardContent className="space-y-4">
          {agents.map((a, idx) => (
            <div
              key={`${a.id}-${idx}`}
              className="space-y-3 rounded-lg border border-border/70 bg-muted/10 p-3"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-xs font-medium text-muted-foreground">Agent {idx + 1}</span>
                {agents.length > 1 ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-7 text-destructive hover:text-destructive"
                    onClick={() => setAgents((prev) => prev.filter((_, i) => i !== idx))}
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                ) : null}
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-1.5">
                  <Label>Id</Label>
                  <Input
                    className="font-mono text-xs"
                    value={a.id}
                    onChange={(e) => setAgents((prev) => prev.map((x, i) => (i === idx ? { ...x, id: e.target.value } : x)))}
                  />
                </div>
                <div className="space-y-1.5">
                  <Label>LLM model id</Label>
                  <Input
                    className="font-mono text-xs"
                    value={a.llm}
                    onChange={(e) => setAgents((prev) => prev.map((x, i) => (i === idx ? { ...x, llm: e.target.value } : x)))}
                  />
                </div>
                <div className="space-y-1.5 sm:col-span-2">
                  <Label>Role</Label>
                  <Input
                    value={a.role}
                    onChange={(e) => setAgents((prev) => prev.map((x, i) => (i === idx ? { ...x, role: e.target.value } : x)))}
                  />
                </div>
                <div className="space-y-1.5 sm:col-span-2">
                  <Label>Goal</Label>
                  <Textarea
                    rows={2}
                    value={a.goal}
                    onChange={(e) => setAgents((prev) => prev.map((x, i) => (i === idx ? { ...x, goal: e.target.value } : x)))}
                  />
                </div>
                <div className="space-y-1.5 sm:col-span-2">
                  <Label>Backstory</Label>
                  <Textarea
                    rows={2}
                    value={a.backstory}
                    onChange={(e) =>
                      setAgents((prev) => prev.map((x, i) => (i === idx ? { ...x, backstory: e.target.value } : x)))
                    }
                  />
                </div>
                <div className="space-y-1.5">
                  <Label>Tools (comma-separated)</Label>
                  <Input
                    className="font-mono text-xs"
                    placeholder="web_fetch, file_read"
                    value={a.toolsStr}
                    onChange={(e) =>
                      setAgents((prev) => prev.map((x, i) => (i === idx ? { ...x, toolsStr: e.target.value } : x)))
                    }
                  />
                </div>
                <div className="space-y-1.5">
                  <Label>Max iterations</Label>
                  <Input
                    type="number"
                    min={0}
                    value={a.max_iter}
                    onChange={(e) =>
                      setAgents((prev) => prev.map((x, i) => (i === idx ? { ...x, max_iter: e.target.value } : x)))
                    }
                  />
                </div>
              </div>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2 pb-3">
          <div>
            <CardTitle className="text-base">Tasks</CardTitle>
            <CardDescription>Assign each task to an agent; context = prior task ids (comma-separated).</CardDescription>
          </div>
          <Button type="button" variant="outline" size="sm" className="gap-1" onClick={() => setTasks((t) => [...t, newTask()])}>
            <Plus className="size-3.5" />
            Add task
          </Button>
        </CardHeader>
        <CardContent className="space-y-4">
          {tasks.map((t, idx) => (
            <div key={`${t.id}-${idx}`} className="space-y-3 rounded-lg border border-border/70 bg-muted/10 p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-xs font-medium text-muted-foreground">Task {idx + 1}</span>
                {tasks.length > 1 ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-7 text-destructive hover:text-destructive"
                    onClick={() => setTasks((prev) => prev.filter((_, i) => i !== idx))}
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                ) : null}
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-1.5">
                  <Label>Id</Label>
                  <Input
                    className="font-mono text-xs"
                    value={t.id}
                    onChange={(e) => setTasks((prev) => prev.map((x, i) => (i === idx ? { ...x, id: e.target.value } : x)))}
                  />
                </div>
                <div className="space-y-1.5">
                  <Label>Agent</Label>
                  <select
                    className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                    value={t.agent_id}
                    onChange={(e) =>
                      setTasks((prev) => prev.map((x, i) => (i === idx ? { ...x, agent_id: e.target.value } : x)))
                    }
                  >
                    <option value="">Select…</option>
                    {agentOptions.map((id) => (
                      <option key={id} value={id}>
                        {id}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="space-y-1.5 sm:col-span-2">
                  <Label>Description</Label>
                  <Textarea
                    rows={2}
                    value={t.description}
                    onChange={(e) =>
                      setTasks((prev) => prev.map((x, i) => (i === idx ? { ...x, description: e.target.value } : x)))
                    }
                  />
                </div>
                <div className="space-y-1.5 sm:col-span-2">
                  <Label>Expected output</Label>
                  <Input
                    value={t.expected_output}
                    onChange={(e) =>
                      setTasks((prev) => prev.map((x, i) => (i === idx ? { ...x, expected_output: e.target.value } : x)))
                    }
                  />
                </div>
                <div className="space-y-1.5 sm:col-span-2">
                  <Label>Context task ids</Label>
                  <Input
                    className="font-mono text-xs"
                    placeholder="t1, t2"
                    value={t.contextStr}
                    onChange={(e) =>
                      setTasks((prev) => prev.map((x, i) => (i === idx ? { ...x, contextStr: e.target.value } : x)))
                    }
                  />
                </div>
              </div>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Run options</CardTitle>
          <CardDescription>Inputs for <code className="text-foreground/80">{"{{placeholders}}"}</code> in task text.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="inputs-json">Inputs (JSON object)</Label>
            <Textarea
              id="inputs-json"
              rows={5}
              className="font-mono text-xs"
              value={inputsJson}
              onChange={(e) => setInputsJson(e.target.value)}
            />
          </div>
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={distributed} onChange={(e) => setDistributed(e.target.checked)} />
            Distributed (P2P crew market)
          </label>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label htmlFor="pod">Pod id (optional)</Label>
              <Input id="pod" className="font-mono text-xs" value={podId} onChange={(e) => setPodId(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="campaign">Campaign id (optional)</Label>
              <Input
                id="campaign"
                className="font-mono text-xs"
                value={campaignId}
                onChange={(e) => setCampaignId(e.target.value)}
              />
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button type="button" variant="secondary" size="sm" onClick={loadDemo}>
              Load example crew
            </Button>
            <Button type="button" variant="outline" size="sm" disabled={busy !== null} onClick={() => void onValidate()}>
              {busy === "validate" ? <Loader2 className="size-3.5 animate-spin" /> : null}
              Validate spec
            </Button>
            <Button type="button" size="sm" disabled={busy !== null} onClick={() => void onKickoff()}>
              {busy === "kickoff" ? <Loader2 className="size-3.5 animate-spin" /> : null}
              Kick off run
            </Button>
          </div>
          {validateMsg ? (
            <p className={cn("text-sm", validateMsg.includes("valid") ? "text-emerald-600 dark:text-emerald-400" : "text-destructive")}>
              {validateMsg}
            </p>
          ) : null}
          {kickoffMsg ? <p className="text-sm text-muted-foreground">{kickoffMsg}</p> : null}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Recent runs</CardTitle>
          <CardDescription>Stored on this node; select a row for detail.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {runs.length === 0 ? (
            <p className="text-sm text-muted-foreground">No crew runs yet.</p>
          ) : (
            <ul className="space-y-1">
              {runs
                .slice()
                .reverse()
                .map((r) => (
                  <li key={r.id}>
                    <button
                      type="button"
                      onClick={() => setSelectedId(r.id)}
                      className={cn(
                        "flex w-full flex-wrap items-center justify-between gap-2 rounded-md border px-3 py-2 text-left text-sm transition-colors",
                        selectedId === r.id
                          ? "border-primary/50 bg-primary/5"
                          : "border-border/60 bg-muted/10 hover:bg-muted/25",
                      )}
                    >
                      <span className="font-mono text-xs">{r.id.slice(0, 8)}…</span>
                      <span className="text-muted-foreground">{r.crew_name || "—"}</span>
                      <Badge variant="outline" className="font-normal">
                        {r.status}
                      </Badge>
                    </button>
                  </li>
                ))}
            </ul>
          )}

          {detail ? (
            <div className="space-y-3 rounded-lg border border-border/80 bg-muted/10 p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">Run detail</span>
                {!["completed", "failed"].includes(detail.status) ? (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="text-destructive"
                    onClick={() => void stopCrewRun(detail.id).then(() => void loadRuns())}
                  >
                    Request stop
                  </Button>
                ) : null}
              </div>
              <pre className="max-h-48 overflow-auto rounded-md bg-background/80 p-2 font-mono text-[11px] leading-relaxed">
                {JSON.stringify(detail, null, 2)}
              </pre>
            </div>
          ) : null}
        </CardContent>
      </Card>
    </div>
  )
}
