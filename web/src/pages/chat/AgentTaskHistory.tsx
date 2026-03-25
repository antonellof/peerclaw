import { useCallback, useEffect, useState } from "react"
import { ChevronDown, ChevronRight } from "lucide-react"

import { useControlWebSocket } from "@/hooks/useControlWebSocket"
import { fetchTaskDetail, fetchTasks, type WebTask } from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { cn } from "@/lib/utils"

function statusVariant(s: string): "default" | "secondary" | "success" | "warning" | "destructive" {
  if (s === "completed") return "success"
  if (s === "failed") return "destructive"
  if (s === "running") return "warning"
  return "secondary"
}

export type AgentTaskHistoryProps = {
  className?: string
  /** `panel`: right-rail layout — no outer “Agent runs” row; list is always visible when non-empty. */
  variant?: "default" | "panel"
}

export function AgentTaskHistory({ className, variant = "default" }: AgentTaskHistoryProps) {
  const [tasks, setTasks] = useState<WebTask[]>([])
  const [inlineOpen, setInlineOpen] = useState(false)
  const [detailOpen, setDetailOpen] = useState(false)
  const [detailTask, setDetailTask] = useState<WebTask | null>(null)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)

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

  const openDetail = async (id: string) => {
    setDetailOpen(true)
    setDetailLoading(true)
    setDetailTask(null)
    setDetailError(null)
    try {
      const res = await fetchTaskDetail(id)
      if (res.ok) {
        setDetailTask(res.task)
      } else {
        setDetailError(res.message)
      }
    } catch (e) {
      setDetailError(e instanceof Error ? e.message : "Request failed.")
    } finally {
      setDetailLoading(false)
    }
  }

  const taskButtons = [...tasks].reverse().map((t) => (
    <button
      key={t.id}
      type="button"
      className="w-full rounded-lg border border-border/50 bg-background/50 p-2 text-left text-[11px] transition-colors hover:bg-muted/50"
      onClick={() => void openDetail(t.id)}
    >
      <div className="flex flex-wrap items-center justify-between gap-1">
        <Badge variant={statusVariant(t.status)} className="text-[9px]">
          {t.status}
        </Badge>
        <span className="text-[10px] text-muted-foreground">{new Date(t.created_at).toLocaleString()}</span>
      </div>
      <p className="mt-1 line-clamp-2 text-muted-foreground">{t.description}</p>
    </button>
  ))

  const emptyCopy =
    variant === "panel" ? (
      <p className="text-[11px] leading-relaxed text-muted-foreground">
        No agent runs yet. Use <span className="font-medium text-foreground">Agent goal</span> in the composer and send
        a goal.
      </p>
    ) : (
      <div
        className={cn(
          "rounded-xl border border-border/60 bg-muted/20 px-3 py-2 text-[11px] text-muted-foreground",
          className,
        )}
      >
        No agent runs yet — switch to <span className="font-medium text-foreground">Agent goal</span> and send a goal.
      </div>
    )

  const dialog = (
    <Dialog open={detailOpen} onOpenChange={setDetailOpen}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Agent run</DialogTitle>
          <DialogDescription className="font-mono text-xs">{detailTask?.id}</DialogDescription>
        </DialogHeader>
        {detailLoading && <p className="text-sm text-muted-foreground">Loading…</p>}
        {!detailLoading && detailError && (
          <p className="whitespace-pre-wrap text-sm text-destructive">{detailError}</p>
        )}
        {!detailLoading && !detailTask && !detailError && (
          <p className="text-sm text-destructive">Could not load task.</p>
        )}
        {detailTask && (
          <div className="space-y-4 text-sm">
            <div className="flex flex-wrap gap-2">
              <Badge variant={statusVariant(detailTask.status)}>{detailTask.status}</Badge>
              <span className="text-xs text-muted-foreground">
                {detailTask.iterations} iters · {detailTask.tokens_used} tok · budget {detailTask.budget}
              </span>
            </div>
            <div>
              <div className="mb-1 text-xs font-medium text-muted-foreground">Result</div>
              <div className="max-h-48 overflow-auto rounded-md border border-border bg-muted/30 p-3 font-mono text-xs whitespace-pre-wrap">
                {detailTask.result ?? "—"}
              </div>
              <Button
                variant="outline"
                size="sm"
                className="mt-2"
                disabled={!detailTask.result}
                onClick={() => detailTask.result && void navigator.clipboard.writeText(detailTask.result)}
              >
                Copy result
              </Button>
            </div>
            <div>
              <div className="mb-1 text-xs font-medium text-muted-foreground">Execution log</div>
              <pre className="max-h-64 overflow-auto rounded-md border border-border bg-background p-3 font-mono text-[11px] whitespace-pre-wrap text-muted-foreground">
                {detailTask.logs?.join("\n") || "—"}
              </pre>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )

  if (tasks.length === 0) {
    return (
      <>
        {variant === "panel" ? <div className={className}>{emptyCopy}</div> : emptyCopy}
        {dialog}
      </>
    )
  }

  if (variant === "panel") {
    return (
      <>
        <div className={cn("flex min-h-0 flex-1 flex-col gap-2", className)}>
          <p className="shrink-0 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
            {tasks.length} run{tasks.length === 1 ? "" : "s"}
          </p>
          <div className="min-h-0 flex-1 space-y-1.5 overflow-y-auto pr-0.5">{taskButtons}</div>
        </div>
        {dialog}
      </>
    )
  }

  return (
    <>
      <div className={cn("rounded-xl border border-border/60 bg-muted/15", className)}>
        <button
          type="button"
          onClick={() => setInlineOpen((o) => !o)}
          className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs font-medium text-foreground hover:bg-muted/40"
        >
          {inlineOpen ? <ChevronDown className="size-3.5 shrink-0" /> : <ChevronRight className="size-3.5 shrink-0" />}
          Agent runs
          <span className="ml-auto font-mono text-[10px] text-muted-foreground">{tasks.length}</span>
        </button>
        {inlineOpen && (
          <div className="max-h-40 space-y-1 overflow-y-auto border-t border-border/50 px-2 py-2">{taskButtons}</div>
        )}
      </div>
      {dialog}
    </>
  )
}
