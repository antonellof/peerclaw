import { useCallback, useEffect, useRef, useState } from "react"
import { useNavigate, useSearchParams } from "react-router-dom"
import { Menu, PanelLeftClose, Plus, Settings2, X } from "lucide-react"

import { useControlWebSocket } from "@/hooks/useControlWebSocket"
import { fetchOnboarding, fetchStatus } from "@/lib/api"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { cn } from "@/lib/utils"
import { ChatPanel } from "@/pages/chat/ChatPanel"
import { parseWorkspaceView, type WorkspaceView } from "./views"
import { WorkspaceNavProvider, type ChatControls } from "./WorkspaceNavContext"
import { ConsolePanel } from "./ConsolePanel"
import { WorkspaceSettingsDialog } from "./WorkspaceSettingsDialog"
import { WorkspaceSidebarAgentRuns } from "./WorkspaceSidebarAgentRuns"
import { WorkspaceSidebarNav, workspaceNavTitle } from "./WorkspaceSidebarNav"
import {
  loadWorkspaceChatPreferences,
  persistWorkspaceChatPreferences,
  type WorkspaceChatPreferences,
} from "@/workspace/workspacePreferences"

export function WorkspaceShell() {
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const rawViewParam = searchParams.get("view")
  const view = parseWorkspaceView(rawViewParam)

  useEffect(() => {
    if (rawViewParam === "tasks" || rawViewParam === "agent") {
      navigate({ pathname: "/", search: "" }, { replace: true })
    }
  }, [rawViewParam, navigate])

  useEffect(() => {
    if (rawViewParam === "join") {
      navigate({ pathname: "/", search: "?view=overview", hash: "join-mesh" }, { replace: true })
    }
  }, [rawViewParam, navigate])

  const [settingsOpen, setSettingsOpen] = useState(false)
  const [chatPreferences, setChatPreferencesState] = useState(loadWorkspaceChatPreferences)
  const [mobileNavOpen, setMobileNavOpen] = useState(false)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [peerLine, setPeerLine] = useState("…")
  const [balanceLine, setBalanceLine] = useState("")
  const [onboardingChips, setOnboardingChips] = useState<string[]>(["Loading…"])

  const chatControlsRef = useRef<ChatControls | null>(null)

  const setView = useCallback(
    (v: WorkspaceView, hash?: string) => {
      const bareHash = hash ? (hash.startsWith("#") ? hash.slice(1) : hash) : ""
      navigate(
        {
          pathname: "/",
          search: v === "chat" ? "" : `?view=${v}`,
          hash: bareHash,
        },
        { replace: true },
      )
      setMobileNavOpen(false)
    },
    [navigate],
  )

  const refreshSidebar = useCallback(async () => {
    try {
      const st = await fetchStatus()
      setPeerLine(`${st.connected_peers} peers`)
      setBalanceLine(`${st.balance.toFixed(4)} PCLAW`)
    } catch {
      setPeerLine("offline")
      setBalanceLine("")
    }
  }, [])

  useEffect(() => {
    void refreshSidebar()
    const t = setInterval(refreshSidebar, 10000)
    return () => clearInterval(t)
  }, [refreshSidebar])

  useEffect(() => {
    void (async () => {
      try {
        const o = await fetchOnboarding()
        setOnboardingChips(o.steps.map((s) => `${s.ok ? "✓" : "○"} ${s.id.replace(/_/g, " ")}`))
      } catch {
        setOnboardingChips(["Could not load onboarding"])
      }
    })()
  }, [])

  useControlWebSocket({
    onStatus: () => void refreshSidebar(),
  })

  useEffect(() => {
    persistWorkspaceChatPreferences(chatPreferences)
  }, [chatPreferences])

  const setChatPreferences = useCallback((u: Partial<WorkspaceChatPreferences>) => {
    setChatPreferencesState((p) => ({ ...p, ...u }))
  }, [])

  const registerChatControls = useCallback((api: ChatControls | null) => {
    chatControlsRef.current = api
  }, [])

  const newChat = useCallback(() => {
    chatControlsRef.current?.clearChat()
    setView("chat")
  }, [setView])

  const agentBuilderFullBleed = view === "workflows"

  const navValue = {
    view,
    setView,
    openHelp: () => setView("help"),
    openSettings: () => setSettingsOpen(true),
    chatPreferences,
    setChatPreferences,
    registerChatControls,
  }

  return (
    <WorkspaceNavProvider value={navValue}>
      <div className="flex h-[100dvh] max-h-[100dvh] min-h-0 overflow-hidden bg-background text-foreground">
        {/* Desktop sidebar */}
        <aside
          className={cn(
            "hidden min-h-0 shrink-0 flex-col overflow-hidden border-r border-border/80 bg-[hsl(240_8%_7%)] md:flex",
            sidebarCollapsed ? "w-[72px]" : "w-[260px]",
            agentBuilderFullBleed && "md:hidden",
          )}
        >
          <div className="flex h-12 items-center gap-2 border-b border-border/60 px-3">
            {!sidebarCollapsed && (
              <span className="truncate text-sm font-semibold tracking-tight">PeerClaw</span>
            )}
            <Button
              variant="ghost"
              size="icon"
              className={cn("size-8 shrink-0 text-muted-foreground", sidebarCollapsed && "mx-auto")}
              onClick={() => setSidebarCollapsed((c) => !c)}
              aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
            >
              <PanelLeftClose className={cn("size-4 transition-transform", sidebarCollapsed && "rotate-180")} />
            </Button>
          </div>

          <div className="p-2">
            <Button
              variant="outline"
              className={cn(
                "w-full justify-start gap-2 border-border/60 bg-background/40 text-foreground hover:bg-background/60",
                sidebarCollapsed && "justify-center px-0",
              )}
              size="sm"
              onClick={newChat}
            >
              <Plus className="size-4 shrink-0" />
              {!sidebarCollapsed && "New chat"}
            </Button>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto">
            <WorkspaceSidebarNav
              active={view}
              onSelect={(v) => setView(v)}
              sidebarCollapsed={sidebarCollapsed}
            />

            <WorkspaceSidebarAgentRuns sidebarCollapsed={sidebarCollapsed} />

            {!sidebarCollapsed && (
              <div className="mx-2 mb-2 rounded-xl border border-border/50 bg-background/30 p-3">
                <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                  Node
                </div>
                <div className="flex flex-wrap gap-1">
                  {onboardingChips.map((c) => (
                    <span
                      key={c}
                      className="rounded-md bg-muted/50 px-1.5 py-0.5 font-mono text-[9px] text-muted-foreground"
                    >
                      {c}
                    </span>
                  ))}
                </div>
              </div>
            )}
          </div>

          <div className="mt-auto shrink-0 border-t border-border/60 p-3">
            <Button
              variant="ghost"
              className={cn(
                "mb-1 w-full justify-start text-muted-foreground hover:text-foreground",
                sidebarCollapsed && "justify-center px-0",
              )}
              size="sm"
              onClick={() => setSettingsOpen(true)}
            >
              <Settings2 className="size-4 shrink-0" />
              {!sidebarCollapsed && <span className="ml-2">Settings</span>}
            </Button>
          </div>
        </aside>

        {/* Main */}
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <header
            className={cn(
              "flex h-12 shrink-0 items-center gap-2 border-b border-border/80 bg-card/30 px-3 md:px-4",
              agentBuilderFullBleed && "md:hidden",
            )}
          >
            <Button
              variant="ghost"
              size="icon"
              className="size-9 shrink-0 md:hidden"
              onClick={() => setMobileNavOpen(true)}
            >
              <Menu className="size-5" />
            </Button>
            {agentBuilderFullBleed ? (
              <span className="min-w-0 flex-1" aria-hidden />
            ) : (
              <span className="min-w-0 flex-1 truncate text-sm font-medium">{workspaceNavTitle(view)}</span>
            )}
            <div className="flex shrink-0 flex-col items-end gap-0.5 text-right sm:flex-row sm:items-center sm:gap-3">
              <div className="text-[11px] text-muted-foreground">
                <span className={peerLine === "offline" ? "text-destructive" : "text-emerald-500"}>●</span> {peerLine}
              </div>
              {peerLine === "offline" && (
                <div className="rounded bg-destructive/10 px-2 py-0.5 text-[10px] text-destructive">
                  Node not connected — run: peerclaw serve --web 127.0.0.1:8080
                </div>
              )}
              {balanceLine ? (
                <div className="font-mono text-[11px] text-muted-foreground tabular-nums">{balanceLine}</div>
              ) : null}
            </div>
          </header>

          <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
            {view === "chat" ? (
              <ChatPanel onRegisterControls={registerChatControls} />
            ) : (
              <ConsolePanel view={view} />
            )}
          </div>
        </div>
      </div>

      <Dialog open={mobileNavOpen} onOpenChange={setMobileNavOpen}>
        <DialogContent className="left-0 top-0 flex h-full max-h-[100dvh] w-[min(100%,300px)] max-w-[90vw] translate-x-0 translate-y-0 flex-col gap-0 rounded-none border-r p-0 sm:rounded-none">
          <DialogHeader className="flex flex-row items-center justify-between border-b border-border px-4 py-3">
            <DialogTitle className="text-base">PeerClaw</DialogTitle>
            <Button variant="ghost" size="icon" className="size-8" onClick={() => setMobileNavOpen(false)}>
              <X className="size-4" />
            </Button>
          </DialogHeader>
          <div className="p-2">
            <Button className="w-full gap-2" size="sm" onClick={newChat}>
              <Plus className="size-4" />
              New chat
            </Button>
          </div>
          <WorkspaceSidebarNav
            active={view}
            onSelect={(v) => setView(v)}
            onPick={() => setMobileNavOpen(false)}
          />
          <WorkspaceSidebarAgentRuns onPickTask={() => setMobileNavOpen(false)} />
          <div className="border-t border-border p-2">
            <Button
              variant="outline"
              className="w-full gap-2"
              size="sm"
              onClick={() => {
                setMobileNavOpen(false)
                setSettingsOpen(true)
              }}
            >
              <Settings2 className="size-4" />
              Settings
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      <WorkspaceSettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        chatPreferences={chatPreferences}
        setChatPreferences={setChatPreferences}
        onNavigate={(v, hash) => setView(v, hash)}
        onModelsChanged={() => chatControlsRef.current?.refreshModels()}
      />
    </WorkspaceNavProvider>
  )
}
