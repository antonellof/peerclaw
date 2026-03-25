import { useCallback, useEffect, useState, type MouseEvent } from "react"
import { useNavigate } from "react-router-dom"
import { ListTodo, Square } from "lucide-react"

import { useControlWebSocket } from "@/hooks/useControlWebSocket"
import { fetchTasks, stopWebTask, type WebTask } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

function statusDotClass(s: string): string {
  if (s === "completed") return "bg-emerald-500"
  if (s === "failed" || s === "cancelled") return "bg-destructive"
  if (s === "running" || s === "pending") return "bg-amber-500 animate-pulse"
  return "bg-muted-foreground"
}

type Props = {
  sidebarCollapsed?: boolean
  onPickTask?: () => void
}

export function WorkspaceSidebarAgentRuns({ sidebarCollapsed, onPickTask }: Props) {
  const navigate = useNavigate()
  const [tasks, setTasks] = useState<WebTask[]>([])
  const [stopping, setStopping] = useState<string | null>(null)

  const loadTasks = useCallback(async () => {
    try {
      setTasks(await fetchTasks())
    } catch {
      setTasks([])
    }
  }, [])

  useEffect(() => {
    void loadTasks()
  }, [loadTasks])

  useControlWebSocket({
    onTasksChanged: () => void loadTasks(),
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
      <div className="flex justify-center px-2 pb-2" title="Agent runs — expand sidebar">
        <ListTodo className="size-5 text-muted-foreground opacity-70" aria-hidden />
      </div>
    )
  }

  const recent = [...tasks].reverse().slice(0, 40)

  return (
    <div className="border-t border-border/50 px-2 pb-3 pt-2">
      <div className="mb-2 flex items-center gap-2 px-2">
        <ListTodo className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
        <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Agent runs
        </span>
      </div>
      {recent.length === 0 ? (
        <p className="px-2 text-[11px] leading-snug text-muted-foreground">
          No runs yet — use <span className="font-medium text-foreground">Agent goal</span> in chat.
        </p>
      ) : (
        <ul className="max-h-[min(40vh,320px)] space-y-0.5 overflow-y-auto pr-0.5">
          {recent.map((t) => {
            const canStop = t.status === "running" || t.status === "pending"
            return (
              <li key={t.id}>
                <div className="group flex items-start gap-1 rounded-lg hover:bg-muted/60">
                  <button
                    type="button"
                    className="min-w-0 flex-1 rounded-lg px-2 py-1.5 text-left text-[11px] leading-snug text-muted-foreground transition-colors hover:text-foreground"
                    onClick={() => focusTaskInChat(t.id)}
                    title={t.description}
                  >
                    <span className="flex items-start gap-2">
                      <span
                        className={cn("mt-1.5 size-1.5 shrink-0 rounded-full", statusDotClass(t.status))}
                        title={t.status}
                      />
                      <span className="line-clamp-2">{t.description || t.id.slice(0, 8)}</span>
                    </span>
                  </button>
                  {canStop ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="size-7 shrink-0 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100"
                      title="Stop task"
                      disabled={stopping === t.id}
                      onClick={(e) => void onStop(e, t.id)}
                    >
                      <Square className="size-3 fill-current" />
                    </Button>
                  ) : null}
                </div>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}
