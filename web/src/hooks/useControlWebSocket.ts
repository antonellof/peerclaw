import { useEffect, useRef } from "react"

import { peerclawWsUrl } from "@/lib/api"

type WsStatusPayload = {
  cpu_usage?: number
  ram_used_mb?: number
  ram_total_mb?: number
  connected_peers?: number
  active_jobs?: number
}

export type FlowLogEvent = {
  run_id: string
  line: string
  status: string
}

export function useControlWebSocket(handlers: {
  onStatus?: (data: WsStatusPayload) => void
  onTasksChanged?: () => void
  /** Live LLM text chunks for dashboard agent tasks (`task_stream_delta` on `/ws`). */
  onTaskStreamDelta?: (taskId: string, text: string) => void
  /** Node persisted workflow library updated (`POST/DELETE /api/agents/library`). */
  onAgentsLibraryChanged?: () => void
  /** Real-time flow/agent run log lines streamed from the node. */
  onFlowLog?: (event: FlowLogEvent) => void
  /** Model download progress events. */
  onDownloadProgress?: (event: { preset: string; downloaded: number; total: number | null; percent: number | null }) => void
}) {
  const handlersRef = useRef(handlers)

  useEffect(() => {
    handlersRef.current = handlers
  })

  useEffect(() => {
    const ws = new WebSocket(peerclawWsUrl())

    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(ev.data as string) as {
          type?: string
          data?: WsStatusPayload
          task_id?: string
          text?: string
          run_id?: string
          line?: string
          status?: string
        }
        if (msg.type === "status" && msg.data) {
          handlersRef.current.onStatus?.(msg.data)
        }
        if (msg.type === "tasks_changed") {
          handlersRef.current.onTasksChanged?.()
        }
        if (msg.type === "task_stream_delta" && msg.task_id && msg.text != null) {
          handlersRef.current.onTaskStreamDelta?.(msg.task_id, msg.text)
        }
        if (msg.type === "agents_library_changed") {
          handlersRef.current.onAgentsLibraryChanged?.()
        }
        if (msg.type === "flow_log" && msg.run_id && msg.line != null) {
          handlersRef.current.onFlowLog?.({
            run_id: msg.run_id,
            line: msg.line,
            status: msg.status ?? "running",
          })
        }
        if (msg.type === "download_progress") {
          handlersRef.current.onDownloadProgress?.({
            preset: (msg as Record<string, unknown>).preset as string ?? "",
            downloaded: (msg as Record<string, unknown>).downloaded as number ?? 0,
            total: (msg as Record<string, unknown>).total as number | null ?? null,
            percent: (msg as Record<string, unknown>).percent as number | null ?? null,
          })
        }
      } catch {
        /* ignore */
      }
    }

    ws.onclose = () => {
      /* optional: reconnect — dashboard used setTimeout */
    }

    return () => {
      ws.close()
    }
  }, [])
}
