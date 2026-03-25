import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react"
import { useLocation, useNavigate } from "react-router-dom"
import { ChevronDown, Send } from "lucide-react"

import { useControlWebSocket } from "@/hooks/useControlWebSocket"
import { createTask, fetchOpenAiModels, fetchTaskDetail, postChatStream, stopWebTask } from "@/lib/api"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import { ChatMessageMarkdown } from "@/pages/chat/ChatMessageMarkdown"
import { AGENT_TASK_TEMPLATES, SCENARIO_PRESETS } from "@/pages/chat/agentTemplates"
import { SLASH_COMMANDS, runSlashCommand, type ChatSettings } from "./slashCommands"
import { useWorkspaceNav } from "@/workspace/WorkspaceNavContext"

const CHAT_SESSION_KEY = "peerclaw_chat_session_v1"
const TRANSCRIPT_KEY_PREFIX = "peerclaw_chat_transcript_v1_"
const AGENT_POLL_MS = 900
const AGENT_POLL_MAX_FAILS = 8
/** Stop polling only after this long (agent tasks may run for hours). */
const AGENT_POLL_MAX_MS = 48 * 60 * 60 * 1000

type MsgRole = "user" | "assistant" | "system" | "error"

type ChatMessage = {
  id: string
  role: MsgRole
  content: string
  meta?: string
  agentTaskId?: string
  agentLogs?: string[]
  agentStatusLine?: string
}

function newId() {
  return crypto.randomUUID?.() ?? String(Date.now()) + Math.random().toString(36).slice(2)
}

function transcriptKey(sessionId: string) {
  return `${TRANSCRIPT_KEY_PREFIX}${sessionId}`
}

function getOrCreateSession(): string | null {
  try {
    let id = localStorage.getItem(CHAT_SESSION_KEY)
    if (!id || id.length < 8) {
      id = crypto.randomUUID?.() ?? String(Date.now()) + Math.random().toString(36).slice(2)
      localStorage.setItem(CHAT_SESSION_KEY, id)
    }
    return id
  } catch {
    return null
  }
}

type Props = {
  onRegisterControls?: (api: { clearChat: () => void } | null) => void
}

/** Agent task log stream: auto-scroll to latest line as the server appends steps. */
function AgentTaskLiveLogs({
  logs,
  running,
  taskId,
}: {
  logs: string[]
  running: boolean
  taskId?: string
}) {
  const preRef = useRef<HTMLPreElement>(null)
  useLayoutEffect(() => {
    const el = preRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [logs])

  const requestStop = () => {
    if (taskId) void stopWebTask(taskId)
  }

  return (
    <div className="mt-2 rounded-lg border border-border/60 bg-background/50">
      <div className="flex items-center justify-between gap-2 border-b border-border/40 px-2 py-1.5">
        <span className="text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
          {running ? "Live steps" : "Steps"}
        </span>
        <div className="flex shrink-0 items-center gap-2">
          {running && taskId ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-6 border-destructive/40 px-2 text-[10px] text-destructive hover:bg-destructive/10"
              title="Request stop (finishes after current model/tool step)"
              onClick={requestStop}
            >
              Stop
            </Button>
          ) : null}
          {running ? (
            <span
              className="inline-flex items-center gap-1 text-[10px] text-muted-foreground"
              title="Agent is still running"
            >
              <span className="inline-flex size-1.5 animate-pulse rounded-full bg-primary" />
              Active
            </span>
          ) : null}
        </div>
      </div>
      <pre
        ref={preRef}
        className="max-h-[min(40vh,280px)] overflow-x-auto overflow-y-auto whitespace-pre-wrap break-words p-2 text-[11px] leading-relaxed text-muted-foreground"
      >
        {logs.length > 0 ? logs.join("\n") : running ? "Waiting for first step…" : "(no steps)"}
      </pre>
    </div>
  )
}

export function ChatPanel({ onRegisterControls }: Props) {
  const { setView, openHelp, chatPreferences, setChatPreferences } = useWorkspaceNav()
  const navigate = useNavigate()
  const location = useLocation()
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [input, setInput] = useState("")
  const [showWelcome, setShowWelcome] = useState(true)
  const [composerMode, setComposerMode] = useState<"chat" | "agent">("chat")
  const [models, setModels] = useState<string[]>([])
  const model = chatPreferences.model
  const setModel = useCallback(
    (m: string) => {
      setChatPreferences({ model: m })
    },
    [setChatPreferences],
  )
  const settings: ChatSettings = useMemo(
    () => ({
      temperature: chatPreferences.temperature,
      maxTokens: chatPreferences.maxTokens,
      distributed: chatPreferences.distributed,
    }),
    [chatPreferences.temperature, chatPreferences.maxTokens, chatPreferences.distributed],
  )
  const setSettings = useCallback(
    (p: Partial<ChatSettings>) => {
      setChatPreferences(p)
    },
    [setChatPreferences],
  )
  const [sessionId, setSessionId] = useState<string | null>(() => getOrCreateSession())
  const [sessionStats, setSessionStats] = useState({ tokens: 0, requests: 0, startTime: Date.now() })
  const [typing, setTyping] = useState(false)
  const [streamLocked, setStreamLocked] = useState(false)
  const [streamingMessageId, setStreamingMessageId] = useState<string | null>(null)
  const [autocompleteOpen, setAutocompleteOpen] = useState(false)
  const [autocompleteFilter, setAutocompleteFilter] = useState("")
  const [autocompleteIdx, setAutocompleteIdx] = useState(0)
  const [agentTaskType, setAgentTaskType] = useState("general")
  const [agentBudget, setAgentBudget] = useState(5)
  const [transcriptReady, setTranscriptReady] = useState(false)

  const scrollRef = useRef<HTMLDivElement>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  /** User is following the latest messages (scroll pinned to bottom). */
  const stickToBottomRef = useRef(true)
  const [showScrollDown, setShowScrollDown] = useState(false)
  const pollingRef = useRef<Set<string>>(new Set())
  const pollIntervalsRef = useRef<Map<string, ReturnType<typeof setInterval>>>(new Map())
  const finishedRef = useRef<Set<string>>(new Set())
  const failuresRef = useRef<Map<string, number>>(new Map())
  const startedAtRef = useRef<Map<string, number>>(new Map())

  const toggleDistributed = useCallback(() => {
    setChatPreferences({ distributed: !chatPreferences.distributed })
  }, [chatPreferences.distributed, setChatPreferences])

  const clearChat = useCallback(() => {
    try {
      const sid = sessionId
      if (sid) localStorage.removeItem(transcriptKey(sid))
    } catch {
      /* ignore */
    }
    setMessages([])
    setShowWelcome(true)
    setSessionStats({ tokens: 0, requests: 0, startTime: Date.now() })
    try {
      const id = crypto.randomUUID?.() ?? String(Date.now())
      localStorage.setItem(CHAT_SESSION_KEY, id)
      setSessionId(id)
    } catch {
      setSessionId(null)
    }
  }, [sessionId])

  useEffect(() => {
    onRegisterControls?.({ clearChat })
    return () => onRegisterControls?.(null)
  }, [clearChat, onRegisterControls])

  useEffect(() => {
    const st = location.state as { focusAgentTaskId?: string } | Record<string, unknown> | null
    const id = st && typeof st === "object" && "focusAgentTaskId" in st ? st.focusAgentTaskId : undefined
    if (typeof id !== "string" || !id) return
    requestAnimationFrame(() => {
      document.querySelector(`[data-agent-task-id="${CSS.escape(id)}"]`)?.scrollIntoView({
        behavior: "smooth",
        block: "center",
      })
    })
    const { focusAgentTaskId: _, ...rest } = st as Record<string, unknown>
    navigate(
      { pathname: location.pathname, search: location.search, hash: location.hash },
      { replace: true, state: Object.keys(rest).length ? rest : {} },
    )
  }, [location.state, location.pathname, location.search, location.hash, navigate])

  useEffect(() => {
    const st = location.state as {
      agentPreset?: { taskType: string; text: string }
      openAgent?: boolean
    } | null
    if (st?.agentPreset) {
      setComposerMode("agent")
      setAgentTaskType(st.agentPreset.taskType)
      setInput(st.agentPreset.text)
      setShowWelcome(false)
      navigate(`${location.pathname}${location.search}`, { replace: true, state: {} })
      return
    }
    if (st?.openAgent) {
      setComposerMode("agent")
      setShowWelcome(false)
      navigate(`${location.pathname}${location.search}`, { replace: true, state: {} })
    }
  }, [location.state, location.pathname, location.search, navigate])

  useEffect(() => {
    if (!sessionId) {
      setTranscriptReady(true)
      return
    }
    setTranscriptReady(false)
    try {
      const raw = localStorage.getItem(transcriptKey(sessionId))
      if (raw) {
        const parsed = JSON.parse(raw) as unknown
        if (Array.isArray(parsed) && parsed.length > 0) {
          const ok = parsed.every(
            (m) =>
              m &&
              typeof m === "object" &&
              typeof (m as ChatMessage).id === "string" &&
              typeof (m as ChatMessage).role === "string" &&
              typeof (m as ChatMessage).content === "string",
          )
          if (ok) {
            setMessages(parsed as ChatMessage[])
            setShowWelcome(false)
          }
        }
      }
    } catch {
      /* ignore corrupt */
    } finally {
      setTranscriptReady(true)
    }
  }, [sessionId])

  useEffect(() => {
    if (!sessionId || !transcriptReady) return
    const t = window.setTimeout(() => {
      try {
        localStorage.setItem(transcriptKey(sessionId), JSON.stringify(messages))
      } catch {
        /* quota */
      }
    }, 400)
    return () => window.clearTimeout(t)
  }, [messages, sessionId, transcriptReady])

  const stopPoll = useCallback((taskId: string) => {
    const intv = pollIntervalsRef.current.get(taskId)
    if (intv) clearInterval(intv)
    pollIntervalsRef.current.delete(taskId)
    pollingRef.current.delete(taskId)
    failuresRef.current.delete(taskId)
    startedAtRef.current.delete(taskId)
  }, [])

  const pollTaskOnce = useCallback(
    async (taskId: string) => {
      if (finishedRef.current.has(taskId)) return

      const started = startedAtRef.current.get(taskId) ?? Date.now()
      if (Date.now() - started > AGENT_POLL_MAX_MS) {
        finishedRef.current.add(taskId)
        stopPoll(taskId)
        setMessages((prev) =>
          prev.map((m) =>
            m.agentTaskId === taskId
              ? {
                  ...m,
                  agentStatusLine: "timed out",
                  content: m.content + "\n[timeout after 48h client poll limit]",
                }
              : m,
          ),
        )
        return
      }

      let detail: Awaited<ReturnType<typeof fetchTaskDetail>>
      try {
        detail = await fetchTaskDetail(taskId)
      } catch {
        const n = (failuresRef.current.get(taskId) ?? 0) + 1
        failuresRef.current.set(taskId, n)
        if (n >= AGENT_POLL_MAX_FAILS) {
          finishedRef.current.add(taskId)
          stopPoll(taskId)
        }
        return
      }

      if (!detail.ok) {
        finishedRef.current.add(taskId)
        stopPoll(taskId)
        setMessages((prev) =>
          prev.map((m) =>
            m.agentTaskId === taskId
              ? {
                  ...m,
                  agentStatusLine: "error",
                  agentLogs: [...(m.agentLogs ?? []), detail.message],
                }
              : m,
          ),
        )
        return
      }

      const t = detail.task
      failuresRef.current.delete(taskId)

      setMessages((prev) =>
        prev.map((m) =>
          m.agentTaskId === taskId
            ? {
                ...m,
                agentStatusLine: `${t.status} · pass ${t.iterations ?? 0} · ${t.tokens_used ?? 0} tok`,
                agentLogs: Array.isArray(t.logs) ? [...t.logs] : m.agentLogs,
              }
            : m,
        ),
      )

      if (t.status === "completed" || t.status === "failed" || t.status === "cancelled") {
        if (finishedRef.current.has(taskId)) return
        finishedRef.current.add(taskId)
        stopPoll(taskId)
        const raw =
          typeof t.result === "string" ? t.result : t.result != null ? String(t.result) : ""
        const trimmed = raw.trim()
        const summary =
          trimmed.length > 0
            ? trimmed
            : t.status === "failed"
              ? "Agent run failed."
              : t.status === "cancelled"
                ? "Stopped."
                : "No summary text was returned. Open **Steps** above for tool output (the model may have stopped after tools, or only emitted tool markup). Try again or use Chat with Tools for a follow-up."
        setMessages((prev) => [
          ...prev,
          {
            id: newId(),
            role: t.status === "failed" ? "error" : "assistant",
            content: summary,
            meta: `${t.status} · ${t.tokens_used ?? 0} tokens`,
          },
        ])
      }
    },
    [stopPoll],
  )

  const startAgentPoll = useCallback(
    (taskId: string) => {
      finishedRef.current.delete(taskId)
      failuresRef.current.delete(taskId)
      startedAtRef.current.set(taskId, Date.now())
      pollingRef.current.add(taskId)
      void pollTaskOnce(taskId)
      const intv = setInterval(() => void pollTaskOnce(taskId), AGENT_POLL_MS)
      pollIntervalsRef.current.set(taskId, intv)
    },
    [pollTaskOnce],
  )

  useControlWebSocket({
    onTasksChanged: () => {
      pollingRef.current.forEach((tid) => void pollTaskOnce(tid))
    },
  })

  useEffect(() => {
    void (async () => {
      try {
        const list = await fetchOpenAiModels()
        const ids = list.map((m) => m.id).filter(Boolean)
        if (ids.length) {
          setModels(ids)
          if (!ids.includes(chatPreferences.model)) {
            setChatPreferences({ model: ids[0]! })
          }
        }
      } catch {
        setModels(["llama-3.2-3b", "llama-3.2-1b", "phi-3-mini"])
      }
    })()
  }, [chatPreferences.model, setChatPreferences])

  const slashCtx = {
    settings,
    setSettings,
    toggleDistributed,
    model,
    setModel,
    sessionStats,
    onClearSession: clearChat,
    setWorkspaceView: setView,
    openHelp,
  }

  const filteredAc =
    autocompleteOpen && autocompleteFilter.length >= 0
      ? SLASH_COMMANDS.filter((c) => c.cmd.slice(1).startsWith(autocompleteFilter.toLowerCase()))
      : []

  const composerMaxHeightPx = useCallback(() => Math.min(Math.round(window.innerHeight * 0.5), 360), [])

  const syncComposerHeight = useCallback(() => {
    const ta = textareaRef.current
    if (!ta) return
    ta.style.height = "0px"
    const max = composerMaxHeightPx()
    const sh = ta.scrollHeight
    const h = Math.min(Math.max(sh, 52), max)
    ta.style.height = `${h}px`
    ta.style.overflowY = sh > max ? "auto" : "hidden"
  }, [composerMaxHeightPx])

  useLayoutEffect(() => {
    syncComposerHeight()
  }, [input, composerMode, autocompleteOpen, syncComposerHeight])

  useEffect(() => {
    window.addEventListener("resize", syncComposerHeight)
    return () => window.removeEventListener("resize", syncComposerHeight)
  }, [syncComposerHeight])

  const updateScrollPinState = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const dist = el.scrollHeight - el.scrollTop - el.clientHeight
    const nearBottom = dist < 100
    stickToBottomRef.current = nearBottom
    setShowScrollDown(!nearBottom && el.scrollHeight > el.clientHeight + 40)
  }, [])

  const scrollMessagesToBottom = useCallback((smooth: boolean) => {
    const el = scrollRef.current
    if (!el) return
    el.scrollTo({ top: el.scrollHeight, behavior: smooth ? "smooth" : "auto" })
    stickToBottomRef.current = true
    setShowScrollDown(false)
  }, [])

  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    el.addEventListener("scroll", updateScrollPinState, { passive: true })
    updateScrollPinState()
    return () => el.removeEventListener("scroll", updateScrollPinState)
  }, [updateScrollPinState, showWelcome, messages.length])

  const onInputChange = (v: string) => {
    setInput(v)
    if (v.startsWith("/")) {
      const space = v.indexOf(" ")
      const frag = space === -1 ? v.slice(1) : v.slice(1, space)
      setAutocompleteOpen(true)
      setAutocompleteFilter(frag)
      setAutocompleteIdx(0)
    } else {
      setAutocompleteOpen(false)
    }
  }

  const send = async () => {
    const content = input.trim()
    if (!content || typing || streamLocked) return
    setAutocompleteOpen(false)

    stickToBottomRef.current = true

    if (content.startsWith("/")) {
      setInput("")
      setShowWelcome(false)
      setMessages((m) => [...m, { id: newId(), role: "system", content: `Running: ${content}` }])
      const out = await runSlashCommand(content, slashCtx)
      setMessages((m) => [...m, { id: newId(), role: "system", content: out }])
      return
    }

    if (composerMode === "agent") {
      setInput("")
      setShowWelcome(false)
      setMessages((m) => [...m, { id: newId(), role: "user", content }])
      setTyping(true)
      try {
        let budget = agentBudget
        if (!Number.isFinite(budget) || budget < 0.5) budget = 5
        const res = await createTask({
          task_type: agentTaskType,
          description: content,
          budget,
          model,
          use_mcp: chatPreferences.useMcp,
        })
        if (!res.success || !res.task_id) {
          setMessages((m) => [
            ...m,
            { id: newId(), role: "error", content: res.error ?? "Could not create task." },
          ])
        } else {
          const tid = res.task_id
          setMessages((m) => [
            ...m,
            {
              id: newId(),
              role: "system",
              content: `Agent task ${tid.slice(0, 8)}…`,
              agentTaskId: tid,
              agentLogs: [],
              agentStatusLine: "starting…",
            },
          ])
          startAgentPoll(tid)
        }
      } catch (e) {
        setMessages((m) => [
          ...m,
          { id: newId(), role: "error", content: e instanceof Error ? e.message : "Task error" },
        ])
      }
      setTyping(false)
      return
    }

    setShowWelcome(false)
    const assistId = newId()
    setMessages((m) => [...m, { id: newId(), role: "user", content }])
    setMessages((m) => [...m, { id: assistId, role: "assistant", content: "" }])
    setInput("")
    setStreamLocked(true)
    setStreamingMessageId(assistId)
    try {
      const data = await postChatStream(
        {
          message: content,
          model,
          max_tokens: settings.maxTokens,
          temperature: settings.temperature,
          session_id: sessionId,
          agentic: chatPreferences.useAgentic,
          use_mcp: chatPreferences.useMcp,
        },
        (text) => {
          setMessages((m) =>
            m.map((x) => (x.id === assistId ? { ...x, content: x.content + text } : x)),
          )
        },
      )
      const meta = [
        data.tokens ? `${data.tokens} tokens` : "",
        data.tokens_per_second ? `${data.tokens_per_second.toFixed(1)} tok/s` : "",
        data.location,
        data.provider_peer_id ? `by ${data.provider_peer_id.slice(0, 8)}…` : "",
      ]
        .filter(Boolean)
        .join(" · ")
      setMessages((m) =>
        m.map((x) =>
          x.id === assistId ? { ...x, content: data.response, meta } : x,
        ),
      )
      setSessionStats((s) => ({
        tokens: s.tokens + (data.tokens || 0),
        requests: s.requests + 1,
        startTime: s.startTime,
      }))
    } catch (e) {
      setMessages((m) =>
        m.map((x) =>
          x.id === assistId
            ? {
                ...x,
                role: "error" as const,
                content: e instanceof Error ? e.message : "Chat error",
              }
            : x,
        ),
      )
    } finally {
      setStreamLocked(false)
      setStreamingMessageId(null)
    }
  }

  useEffect(() => {
    if (!stickToBottomRef.current) return
    const el = scrollRef.current
    if (!el) return
    const smooth = !streamLocked
    el.scrollTo({ top: el.scrollHeight, behavior: smooth ? "smooth" : "auto" })
    requestAnimationFrame(updateScrollPinState)
  }, [messages, streamLocked, typing, updateScrollPinState])

  const modelList = models.length ? models : ["llama-3.2-3b"]

  const applyAgentTemplate = (key: keyof typeof AGENT_TASK_TEMPLATES) => {
    const t = AGENT_TASK_TEMPLATES[key]
    if (!t) return
    setAgentTaskType(t.taskType)
    setInput(t.text)
    setComposerMode("agent")
    setShowWelcome(false)
  }

  const applyScenarioPreset = (key: string) => {
    const p = SCENARIO_PRESETS[key]
    if (!p) return
    setAgentTaskType(p.type)
    setInput(p.text)
    setComposerMode("agent")
    setShowWelcome(false)
  }

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
      <header className="flex h-12 shrink-0 items-center border-b border-border/70 bg-card/20 px-3 md:px-4">
        <div className="min-w-0">
          <p className="truncate text-sm font-medium text-foreground">Assistant</p>
          <p className="truncate text-[11px] text-muted-foreground">Streaming chat &amp; quick agent runs</p>
        </div>
      </header>

      <div className="relative flex min-h-0 flex-1 flex-col">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="relative min-h-0 flex-1">
            <div
              ref={scrollRef}
              className="h-full min-h-0 overflow-y-auto overflow-x-hidden overscroll-contain"
            >
            <div className="mx-auto flex min-h-full max-w-3xl flex-col justify-end px-3 py-4 md:px-4">
              {showWelcome && messages.length === 0 ? (
            <div className="flex min-h-0 flex-1 flex-col justify-center py-6">
              <div className="text-center">
                <h1 className="text-2xl font-semibold tracking-tight md:text-3xl">How can I help today?</h1>
                <p className="mx-auto mt-3 max-w-md text-sm leading-relaxed text-muted-foreground">
                  Ask anything, use{" "}
                  <kbd className="rounded border border-border bg-muted px-1.5 py-0.5 font-mono text-xs">/</kbd> for
                  commands, or switch to <strong className="text-foreground">Agent goal</strong> for multi-step agent
                  runs (history is saved in this browser for the current session). Past agent tasks appear under{" "}
                  <strong className="text-foreground">Agent runs</strong> in the left sidebar.
                </p>
                <div className="mx-auto mt-8 grid max-w-lg gap-2 sm:grid-cols-2">
                  {[
                    { t: "New conversation", p: "Hello — what can you do on this node?", cmd: null },
                    { t: "Commands", p: null, cmd: "/help" },
                    { t: "Skills", p: null, cmd: "/skills" },
                    { t: "Node overview", p: null, cmd: "/open overview" },
                  ].map((x) => (
                    <button
                      key={x.t}
                      type="button"
                      className="rounded-xl border border-border/80 bg-card/50 p-4 text-left text-sm shadow-sm transition-colors hover:border-primary/30 hover:bg-muted/30"
                      onClick={() => {
                        if (x.cmd) setInput(x.cmd)
                        else setInput(x.p ?? "")
                      }}
                    >
                      <div className="font-medium">{x.t}</div>
                      <div className="mt-1 text-xs text-muted-foreground">{x.p ?? x.cmd}</div>
                    </button>
                  ))}
                </div>
              </div>
              </div>
              ) : (
                <div className="space-y-6 pb-2">
            {messages.map((m) => (
              <div
                key={m.id}
                data-agent-task-id={m.agentTaskId}
                className={cn(
                  "flex gap-3",
                  m.role === "user" && "flex-row-reverse",
                )}
              >
                <div
                  className={cn(
                    "flex size-8 shrink-0 items-center justify-center rounded-full text-[10px] font-semibold uppercase tracking-wide",
                    m.role === "user" && "bg-primary text-primary-foreground",
                    m.role === "assistant" && "bg-secondary text-secondary-foreground",
                    m.role === "system" && "bg-muted text-muted-foreground",
                    m.role === "error" && "bg-destructive/20 text-destructive",
                  )}
                >
                  {m.role === "user" ? "You" : m.role === "assistant" ? "AI" : "!"}
                </div>
                <div
                  className={cn(
                    "min-w-0 max-w-[min(100%,28rem)] rounded-2xl px-4 py-3 text-sm leading-relaxed",
                    m.role === "user" && "bg-primary text-primary-foreground",
                    m.role === "assistant" && "border border-border/80 bg-card text-card-foreground shadow-sm",
                    (m.role === "system" || m.role === "error") &&
                      "border border-border bg-muted/40 font-mono text-xs text-muted-foreground",
                  )}
                >
                  {m.agentTaskId && (
                    <div className="mb-2 text-[11px] text-muted-foreground">{m.agentStatusLine}</div>
                  )}
                  {m.role === "assistant" ? (
                    <ChatMessageMarkdown
                      content={m.content}
                      isAnimating={streamLocked && m.id === streamingMessageId}
                    />
                  ) : (
                    <div className="whitespace-pre-wrap break-words">{m.content}</div>
                  )}
                  {m.agentTaskId ? (
                    <AgentTaskLiveLogs
                      logs={m.agentLogs ?? []}
                      taskId={m.agentTaskId}
                      running={(() => {
                        const s = m.agentStatusLine ?? ""
                        if (!s || s === "error") return false
                        if (/^(completed|failed|cancelled|timed out)/i.test(s)) return false
                        return true
                      })()}
                    />
                  ) : null}
                  {m.meta && <div className="mt-2 text-[11px] text-muted-foreground">{m.meta}</div>}
                </div>
              </div>
            ))}
            {typing && composerMode === "agent" && (
              <div className="pl-11 text-sm text-muted-foreground">Working…</div>
            )}
                </div>
              )}
            </div>
            </div>
            {showScrollDown && (
              <div className="pointer-events-none absolute inset-x-0 bottom-0 flex justify-center pb-2 pt-8">
                <Button
                  type="button"
                  size="icon"
                  variant="secondary"
                  className="pointer-events-auto size-9 rounded-full border border-border/80 bg-card/95 shadow-md backdrop-blur-sm hover:bg-muted"
                  aria-label="Scroll to latest messages"
                  onClick={() => scrollMessagesToBottom(true)}
                >
                  <ChevronDown className="size-4" />
                </Button>
              </div>
            )}
          </div>

          <div className="shrink-0 border-t border-border/80 bg-gradient-to-t from-card/90 to-card/40 px-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] pt-3 md:px-4">
            <div className="mx-auto max-w-3xl space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <div className="inline-flex rounded-lg border border-border/60 bg-muted/40 p-0.5">
              <button
                type="button"
                className={cn(
                  "rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
                  composerMode === "chat" ? "bg-background text-foreground shadow-sm" : "text-muted-foreground",
                )}
                onClick={() => setComposerMode("chat")}
              >
                Chat
              </button>
              <button
                type="button"
                className={cn(
                  "rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
                  composerMode === "agent" ? "bg-background text-foreground shadow-sm" : "text-muted-foreground",
                )}
                onClick={() => setComposerMode("agent")}
              >
                Agent goal
              </button>
            </div>
            <button
              type="button"
              title="Node tools + P2P job_submit / job_status ReAct loop"
              className={cn(
                "rounded-md border px-2.5 py-1 text-xs font-medium transition-colors",
                chatPreferences.useAgentic
                  ? "border-primary/60 bg-primary/15 text-foreground"
                  : "border-border/60 bg-muted/30 text-muted-foreground hover:bg-muted/50",
              )}
              onClick={() => setChatPreferences({ useAgentic: !chatPreferences.useAgentic })}
            >
              Tools
            </button>
            <button
              type="button"
              title="Use MCP tools from configured servers (MCP page)"
              className={cn(
                "rounded-md border px-2.5 py-1 text-xs font-medium transition-colors",
                chatPreferences.useMcp
                  ? "border-primary/60 bg-primary/15 text-foreground"
                  : "border-border/60 bg-muted/30 text-muted-foreground hover:bg-muted/50",
              )}
              onClick={() => setChatPreferences({ useMcp: !chatPreferences.useMcp })}
            >
              MCP
            </button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-8 max-w-[11rem] gap-1 border-border/80 px-2 font-normal"
                  title="Model for this chat"
                >
                  <span className="truncate text-xs">{model}</span>
                  <ChevronDown className="size-3.5 shrink-0 opacity-50" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-56">
                <DropdownMenuLabel className="text-xs">Model</DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuRadioGroup value={model} onValueChange={setModel}>
                  {modelList.map((m) => (
                    <DropdownMenuRadioItem key={m} value={m} className="text-xs">
                      {m}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
            {composerMode === "agent" && (
              <>
                <select
                  className="h-8 rounded-md border border-input bg-background px-2 text-xs"
                  value={agentTaskType}
                  onChange={(e) => setAgentTaskType(e.target.value)}
                >
                  {["general", "research", "code", "monitor", "analyze"].map((x) => (
                    <option key={x} value={x}>
                      {x}
                    </option>
                  ))}
                </select>
                <label className="flex items-center gap-1 text-xs text-muted-foreground">
                  Budget
                  <input
                    type="number"
                    className="h-8 w-16 rounded-md border border-input bg-background px-1.5 text-xs"
                    value={agentBudget}
                    min={0.5}
                    step={0.5}
                    onChange={(e) => setAgentBudget(parseFloat(e.target.value) || 5)}
                  />
                </label>
              </>
            )}
          </div>

          {composerMode === "agent" && (
            <div className="space-y-2">
              <p className="text-[10px] font-medium uppercase tracking-wide text-muted-foreground">Quick templates</p>
              <div className="flex flex-wrap gap-1.5">
                {(
                  [
                    ["research", "Research"],
                    ["summarize", "Summarize URL"],
                    ["code", "Code review"],
                    ["monitor", "Check URL"],
                    ["analyze", "Analyze"],
                    ["automate", "Automate"],
                  ] as const
                ).map(([key, label]) => (
                  <Button
                    key={key}
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-7 border-border/70 px-2 text-[10px]"
                    onClick={() => applyAgentTemplate(key)}
                  >
                    {label}
                  </Button>
                ))}
                {Object.keys(SCENARIO_PRESETS).map((k) => (
                  <Button
                    key={k}
                    type="button"
                    variant="secondary"
                    size="sm"
                    className="h-7 px-2 text-[10px]"
                    onClick={() => applyScenarioPreset(k)}
                  >
                    {k}
                  </Button>
                ))}
              </div>
            </div>
          )}

          <div className="relative rounded-2xl border border-border/80 bg-background shadow-sm">
            {autocompleteOpen && filteredAc.length > 0 && (
              <div className="absolute bottom-full left-0 right-0 z-20 mb-2 max-h-48 overflow-auto rounded-xl border border-border bg-popover p-1 shadow-lg">
                {filteredAc.map((c, i) => (
                  <button
                    key={c.cmd}
                    type="button"
                    className={cn(
                      "block w-full rounded-lg px-3 py-2 text-left text-xs hover:bg-muted",
                      i === autocompleteIdx && "bg-muted",
                    )}
                    onClick={() => {
                      setInput(c.cmd + " ")
                      setAutocompleteOpen(false)
                    }}
                  >
                    <span className="font-mono text-primary">{c.cmd}</span>{" "}
                    <span className="text-muted-foreground">{c.desc}</span>
                  </button>
                ))}
              </div>
            )}
            <Textarea
              ref={textareaRef}
              rows={1}
              placeholder={
                composerMode === "agent"
                  ? "Describe the goal (node needs --agent)…"
                  : "Message, or type / for commands…"
              }
              value={input}
              onChange={(e) => onInputChange(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault()
                  void send()
                }
                if (autocompleteOpen && filteredAc.length) {
                  if (e.key === "ArrowDown") {
                    e.preventDefault()
                    setAutocompleteIdx((i) => (i + 1) % filteredAc.length)
                  }
                  if (e.key === "ArrowUp") {
                    e.preventDefault()
                    setAutocompleteIdx((i) => (i - 1 + filteredAc.length) % filteredAc.length)
                  }
                  if (e.key === "Tab" && filteredAc[autocompleteIdx]) {
                    e.preventDefault()
                    setInput(filteredAc[autocompleteIdx]!.cmd + " ")
                  }
                }
              }}
              className="min-h-[52px] max-h-[min(50dvh,22.5rem)] resize-none border-0 bg-transparent py-3 pr-12 focus-visible:ring-0"
              disabled={typing || streamLocked}
            />
            <Button
              size="icon"
              className="absolute bottom-2 right-2 size-9 shrink-0 rounded-full"
              disabled={typing || streamLocked}
              onClick={() => void send()}
            >
              <Send className="size-4" />
            </Button>
          </div>
              <p className="text-center text-[10px] text-muted-foreground">
                <kbd className="rounded border border-border/80 px-1">/</kbd> commands ·{" "}
                <kbd className="rounded border border-border/80 px-1">↵</kbd> send · Shift+Enter newline
              </p>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
