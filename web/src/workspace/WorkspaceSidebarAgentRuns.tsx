import { useCallback, useEffect, useState, type MouseEvent } from "react"
import { useNavigate } from "react-router-dom"
import { ListTodo, Square, Workflow } from "lucide-react"

import { useControlWebSocket } from "@/hooks/useControlWebSocket"
import { fetchFlowRuns, fetchTasks, stopWebTask, type FlowRunRecordJson, type WebTask } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

function statusDotClass(s: string): string {
  if (s === "completed") return "bg-emerald-500"
  if (s === "failed" || s === "cancelled") return "bg-destructive"
  if (s === "running" || s === "pending") return "bg-amber-500 animate-pulse"
  return "bg-muted-foreground"
}

/** Format a date string as a relative time label (e.g. "2m ago", "1h ago"). */
function relativeTime(dateStr: string): string {
  const now = Date.now()
  const then = new Date(dateStr).getTime()
  if (isNaN(then)) return ""
  const diffMs = now - then
  if (diffMs < 0) return "just now"
  const sec = Math.floor(diffMs / 1000)
  if (sec < 60) return `${sec}s ago`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}m ago`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr}h ago`
  const day = Math.floor(hr / 24)
  return `${day}d ago`
}

/** Unified run entry (task or workflow run). */
type RunEntry = {
  id: string
  label: string
  status: string
  createdAt: string
  kind: "task" | "workflow"
  canStop: boolean
}

function toRunEntries(tasks: WebTask[], flowRuns: FlowRunRecordJson[]): RunEntry[] {
  const entries: RunEntry[] = []
  for (const t of tasks) {
    entries.push({
      id: t.id,
      label: t.description || t.id.slice(0, 8),
      status: t.status,
      createdAt: t.created_at,
      kind: "task",
      canStop: t.status === "running" || t.status === "pending",
    })
  }
  for (const f of flowRuns) {
    entries.push({
      id: f.id,
      label: f.flow_name || f.id.slice(0, 8),
      status: f.status,
      createdAt: f.created_at,
      kind: "workflow",
      canStop: false,
    })
  }
  // Sort by creation time descending (most recent first)
  entries.sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime())
  return entries.slice(0, 40)
}

type Props = {
  sidebarCollapsed?: boolean
  onPickTask?: () => void
}

export function WorkspaceSidebarAgentRuns({ sidebarCollapsed, onPickTask }: Props) {
  const navigate = useNavigate()
  const [tasks, setTasks] = useState<WebTask[]>([])
  const [flowRuns, setFlowRuns] = useState<FlowRunRecordJson[]>([])
  const [stopping, setStopping] = useState<string | null>(null)

  const loadTasks = useCallback(async () => {
    try {
      setTasks(await fetchTasks())
    } catch {
      setTasks([])
    }
  }, [])

  const loadFlowRuns = useCallback(async () => {
    try {
      setFlowRuns(await fetchFlowRuns())
    } catch {
      setFlowRuns([])
    }
  }, [])

  useEffect(() => {
    void loadTasks()
    void loadFlowRuns()
  }, [loadTasks, loadFlowRuns])

  useControlWebSocket({
    onTasksChanged: () => {
      void loadTasks()
      void loadFlowRuns()
    },
  })

  const focusTaskInChat = (id: string) => {
    navigate("/", { state: { focusAgentTaskId: id } })
    onPickTask?.()
  }

  const onStop = async (e: MouseEvent, id: string) => {
    e.stopPropagation()
    setStopping(id)
    try {
      await stopWebTask(id)
      void loadTasks()
    } finally {
      setStopping(null)
    }
  }

  if (sidebarCollapsed) {
    return (
      <div className="flex justify-center px-2 pb-2" title="Runs — expand sidebar">
        <ListTodo className="size-5 text-muted-foreground opacity-70" aria-hidden />
      </div>
    )
  }

  const recent = toRunEntries(tasks, flowRuns)

  return (
    <div className="border-t border-border/50 px-2 pb-3 pt-2">
      <div className="mb-2 flex items-center gap-2 px-2">
        <ListTodo className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
        <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Runs
        </span>
      </div>
      {recent.length === 0 ? (
        <p className="px-2 text-[11px] leading-snug text-muted-foreground">
          No runs yet — send a message in chat or run a workflow.
        </p>
      ) : (
        <ul className="max-h-[min(40vh,320px)] space-y-0.5 overflow-y-auto pr-0.5">
          {recent.map((entry) => (
            <li key={`${entry.kind}-${entry.id}`}>
              <div className="group flex items-start gap-1 rounded-lg hover:bg-muted/60">
                <button
                  type="button"
                  className="min-w-0 flex-1 rounded-lg px-2 py-1.5 text-left text-[11px] leading-snug text-muted-foreground transition-colors hover:text-foreground"
                  onClick={() => focusTaskInChat(entry.id)}
                  title={entry.label}
                >
                  <span className="flex items-start gap-2">
                    <span
                      className={cn("mt-1.5 size-1.5 shrink-0 rounded-full", statusDotClass(entry.status))}
                      title={entry.status}
                    />
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-1">
                        {entry.kind === "workflow" && (
                          <Workflow className="inline size-3 shrink-0 text-muted-foreground/70" />
                        )}
                        <span className="line-clamp-2">{entry.label}</span>
                      </span>
                      <span className="block text-[10px] text-muted-foreground/60">
                        {relativeTime(entry.createdAt)}
                      </span>
                    </span>
                  </span>
                </button>
                {entry.canStop ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-7 shrink-0 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100"
                    title="Stop task"
                    disabled={stopping === entry.id}
                    onClick={(e) => void onStop(e, entry.id)}
                  >
                    <Square className="size-3 fill-current" />
                  </Button>
                ) : null}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
