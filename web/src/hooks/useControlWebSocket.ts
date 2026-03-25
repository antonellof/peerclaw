import { useEffect, useRef } from "react"

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
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:"
    const ws = new WebSocket(`${proto}//${window.location.host}/ws`)

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
