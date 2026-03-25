import { useEffect, useRef } from "react"

import { peerclawWsUrl } from "@/lib/api"

type WsStatusPayload = {
  cpu_usage?: number
  ram_used_mb?: number
  ram_total_mb?: number
  connected_peers?: number
  active_jobs?: number
}

export function useControlWebSocket(handlers: {
  onStatus?: (data: WsStatusPayload) => void
  onTasksChanged?: () => void
}) {
  const handlersRef = useRef(handlers)

  useEffect(() => {
    handlersRef.current = handlers
  })

  useEffect(() => {
    const ws = new WebSocket(peerclawWsUrl())

    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(ev.data as string) as { type?: string; data?: WsStatusPayload }
        if (msg.type === "status" && msg.data) {
          handlersRef.current.onStatus?.(msg.data)
        }
        if (msg.type === "tasks_changed") {
          handlersRef.current.onTasksChanged?.()
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
