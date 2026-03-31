import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  addEdge,
  Background,
  BackgroundVariant,
  Controls,
  type Connection,
  type Edge,
  type Node,
  Panel,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  useReactFlow,
  useStore,
} from "@xyflow/react"
import {
  Bot,
  BookmarkPlus,
  ChevronDown,
  ChevronLeft,
  ChevronUp,
  Code2,
  FolderOpen,
  Diamond,
  FileSearch,
  Flag,
  GitBranch,
  GripHorizontal,
  LayoutList,
  Loader2,
  type LucideIcon,
  Pencil,
  Play,
  Plus,
  Redo2,
  Repeat,
  Settings,
  Shield,
  Sparkles,
  StickyNote,
  Terminal,
  Trash2,
  Undo2,
  UserCheck,
  Wrench,
} from "lucide-react"

import {
  deleteAgentLibraryEntry,
  fetchAgentLibrary,
  fetchFlowRun,
  fetchFlowRuns,
  fetchMcpStatus,
  fetchOpenAiModels,
  kickoffFlow,
  upsertAgentLibraryEntry,
  validateFlow,
  type AgentLibraryEntryJson,
  type FlowRunRecordJson,
  type FlowSpecJson,
  type McpToolListItem,
} from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

import { useWorkspaceNav } from "@/workspace/WorkspaceNavContext"
import { builderNodeTypes } from "./builderNodes"
import { compileFlowSpec, demoGraph, flowSpecToReactFlow, type FlowNodeData } from "./flowCompile"
import { FlowNodeInspector } from "./nodeInspector"

const DEMO_INPUTS = `{
  "topic": "Rust async",
  "input_as_text": "Summarize async in Rust",
  "human_approval": "approve"
}`

const LS_RUN_PANEL_H = "peerclaw-agent-builder-run-panel-h"
const LS_RUN_PANEL_COLLAPSED = "peerclaw-agent-builder-run-panel-collapsed"

function readRunPanelHeight(): number {
  if (typeof window === "undefined") return 232
  try {
    const raw = localStorage.getItem(LS_RUN_PANEL_H)
    if (raw == null) return 232
    const n = parseInt(raw, 10)
    if (!Number.isFinite(n)) return 232
    return Math.min(560, Math.max(120, n))
  } catch {
    return 232
  }
}

function readRunPanelCollapsed(): boolean {
  if (typeof window === "undefined") return true
  try {
    const v = localStorage.getItem(LS_RUN_PANEL_COLLAPSED)
    if (v === null) return true
    return v === "1"
  } catch {
    return true
  }
}

function newId() {
  return `n_${crypto.randomUUID().slice(0, 8)}`
}

type PaletteItem = { type: string; label: string; section: string; hint: string; icon: LucideIcon }

const PALETTE: PaletteItem[] = [
  { section: "Core", type: "agent", label: "Agent", hint: "ReAct agent with tools", icon: Bot },
  { section: "Core", type: "classify", label: "Classify", hint: "Routes to LLM with category list", icon: LayoutList },
  { section: "Core", type: "end", label: "End", hint: "Stops the run", icon: Flag },
  { section: "Core", type: "note", label: "Note", hint: "Canvas-only (not exported)", icon: StickyNote },
  { section: "Tools", type: "fileSearch", label: "File search", hint: "vectX semantic search", icon: FileSearch },
  { section: "Tools", type: "guardrails", label: "Guardrails", hint: "SafetyLayer pass / fail branches", icon: Shield },
  { section: "Tools", type: "mcp", label: "MCP", hint: "Tools from node MCP config", icon: Wrench },
  { section: "Logic", type: "if", label: "If / else", hint: "CEL → true / false edges", icon: GitBranch },
  { section: "Logic", type: "while", label: "While", hint: "CEL → loop / exit edges", icon: Repeat },
  { section: "Logic", type: "userApproval", label: "User approval", hint: "inputs.human_approval: approve|reject", icon: UserCheck },
  { section: "Data", type: "transform", label: "Transform", hint: "Copy prior output into state", icon: Diamond },
  { section: "Data", type: "setState", label: "Set state", hint: "JSON value into state map", icon: Diamond },
]

function CanvasSync({ onGraph }: { onGraph: (nodes: Node[], edges: Edge[]) => void }) {
  const nodes = useStore((s) => s.nodes)
  const edges = useStore((s) => s.edges)
  useEffect(() => {
    onGraph(nodes, edges)
  }, [nodes, edges, onGraph])
  return null
}

function PaletteSidebar({ preview, onAdd }: { preview: boolean; onAdd: (type: string) => void }) {
  return (
    <ScrollArea className="flex-1 px-3 pb-4 pt-1">
      {["Core", "Tools", "Logic", "Data"].map((sec) => (
        <div key={sec} className="mb-4">
          <p className="mb-2 px-0.5 text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/70">
            {sec}
          </p>
          <div className="flex flex-col gap-0.5">
            {PALETTE.filter((p) => p.section === sec).map((p) => {
              const Icon = p.icon
              return (
                <button
                  key={p.type}
                  type="button"
                  title={p.hint}
                  className={cn(
                    "flex items-center gap-2.5 rounded-lg px-2 py-2 text-left text-[13px] transition-colors",
                    "text-foreground/90 hover:bg-muted/50",
                    preview && "pointer-events-none opacity-40",
                  )}
                  disabled={preview}
                  onClick={() => onAdd(p.type)}
                >
                  <Icon className="size-4 shrink-0 text-muted-foreground" />
                  <span>{p.label}</span>
                </button>
              )
            })}
          </div>
        </div>
      ))}
    </ScrollArea>
  )
}

function AgentBuilderInner() {
  const { setView } = useWorkspaceNav()
  const starter = useMemo(() => demoGraph(), [])
  const [preview, setPreview] = useState(false)
  const [flowName, setFlowName] = useState("New agent")
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [runPanelTab, setRunPanelTab] = useState<"logs" | "steps" | "json" | "inputs">("logs")
  const [nodes, setNodes, onNodesChange] = useNodesState(starter.nodes)
  const [edges, setEdges, onEdgesChange] = useEdgesState(starter.edges)
  const { screenToFlowPosition } = useReactFlow()

  const [nodesSnap, setNodesSnap] = useState<Node[]>(starter.nodes)
  const [edgesSnap, setEdgesSnap] = useState<Edge[]>(starter.edges)
  const onGraph = useCallback((n: Node[], e: Edge[]) => {
    setNodesSnap(n)
    setEdgesSnap(e)
  }, [])

  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null)
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null)

  // Agent library — load/save/edit existing agents
  const [libraryOpen, setLibraryOpen] = useState(false)
  const [library, setLibrary] = useState<AgentLibraryEntryJson[]>([])
  const [loadedAgentId, setLoadedAgentId] = useState<string | null>(null)
  const [inputsJson, setInputsJson] = useState(DEMO_INPUTS)
  const [codeOpen, setCodeOpen] = useState(false)
  const [codeText, setCodeText] = useState("")
  const [validateMsg, setValidateMsg] = useState<string | null>(null)
  const [runMsg, setRunMsg] = useState<string | null>(null)
  const [busy, setBusy] = useState<"v" | "r" | "l" | null>(null)
  const [runs, setRuns] = useState<FlowRunRecordJson[]>([])
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null)
  const [runDetail, setRunDetail] = useState<FlowRunRecordJson | null>(null)
  const [modelIds, setModelIds] = useState<string[]>([])
  const [mcpTools, setMcpTools] = useState<McpToolListItem[]>([])
  const [mcpEnabled, setMcpEnabled] = useState(false)
  const [mcpHint, setMcpHint] = useState<string | null>(null)
  const [runPanelCollapsed, setRunPanelCollapsed] = useState(readRunPanelCollapsed)
  const [runPanelHeight, setRunPanelHeight] = useState(readRunPanelHeight)
  const runPanelHeightRef = useRef(runPanelHeight)

  useEffect(() => {
    runPanelHeightRef.current = runPanelHeight
  }, [runPanelHeight])

  useEffect(() => {
    try {
      localStorage.setItem(LS_RUN_PANEL_COLLAPSED, runPanelCollapsed ? "1" : "0")
    } catch {
      /* ignore */
    }
  }, [runPanelCollapsed])

  useEffect(() => {
    try {
      localStorage.setItem(LS_RUN_PANEL_H, String(runPanelHeight))
    } catch {
      /* ignore */
    }
  }, [runPanelHeight])

  const onRunPanelResizePointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault()
    const target = e.currentTarget
    target.setPointerCapture(e.pointerId)
    const startY = e.clientY
    const startH = runPanelHeightRef.current
    const maxH = () => Math.min(Math.floor(window.innerHeight * 0.5), 560)
    const minH = 120
    const onMove = (ev: PointerEvent) => {
      const dy = startY - ev.clientY
      setRunPanelHeight(Math.min(maxH(), Math.max(minH, startH + dy)))
    }
    const onUp = (ev: PointerEvent) => {
      try {
        target.releasePointerCapture(ev.pointerId)
      } catch {
        /* released */
      }
      window.removeEventListener("pointermove", onMove)
      window.removeEventListener("pointerup", onUp)
      window.removeEventListener("pointercancel", onUp)
    }
    window.addEventListener("pointermove", onMove)
    window.addEventListener("pointerup", onUp)
    window.addEventListener("pointercancel", onUp)
  }, [])

  useEffect(() => {
    void fetchOpenAiModels()
      .then((m) => setModelIds(m.map((x) => x.id)))
      .catch(() => setModelIds([]))
  }, [])

  const refreshMcpCatalog = useCallback(() => {
    void fetchMcpStatus()
      .then((s) => {
        setMcpTools(s.tools)
        setMcpEnabled(s.config.enabled)
        const up = s.connected_servers.length
        const total = s.config.servers.length
        setMcpHint(
          s.config.enabled
            ? `${s.tool_count} tools in catalog · ${up}/${total} servers connected`
            : "MCP disabled — enable in Settings → MCP",
        )
      })
      .catch(() => {
        setMcpTools([])
        setMcpEnabled(false)
        setMcpHint("Could not load MCP status (is the node running?)")
      })
  }, [])

  useEffect(() => {
    refreshMcpCatalog()
    const t = setInterval(refreshMcpCatalog, 45_000)
    return () => clearInterval(t)
  }, [refreshMcpCatalog])

  const refreshLibrary = useCallback(() => {
    void fetchAgentLibrary()
      .then(setLibrary)
      .catch(() => setLibrary([]))
  }, [])

  useEffect(() => {
    refreshLibrary()
  }, [refreshLibrary])

  const loadAgent = useCallback(
    (entry: AgentLibraryEntryJson) => {
      if (!entry.flow_spec) return
      const { nodes: nn, edges: ee } = flowSpecToReactFlow(entry.flow_spec)
      setNodes(nn)
      setEdges(ee)
      setFlowName(entry.name)
      setLoadedAgentId(entry.id)
      setLibraryOpen(false)
      setSelectedNodeId(null)
      setSelectedEdgeId(null)
      setValidateMsg(null)
    },
    [setNodes, setEdges],
  )

  const deleteAgent = useCallback(
    async (id: string) => {
      const r = await deleteAgentLibraryEntry(id)
      if (r.ok) {
        refreshLibrary()
        if (loadedAgentId === id) setLoadedAgentId(null)
      }
    },
    [refreshLibrary, loadedAgentId],
  )

  const newAgent = useCallback(() => {
    const g = demoGraph()
    setNodes(g.nodes)
    setEdges(g.edges)
    setFlowName("New agent")
    setLoadedAgentId(null)
    setLibraryOpen(false)
    setSelectedNodeId(null)
    setSelectedEdgeId(null)
    setValidateMsg(null)
  }, [setNodes, setEdges])

  const spec = useMemo(
    () => compileFlowSpec(nodesSnap, edgesSnap, flowName.trim() || "agent"),
    [nodesSnap, edgesSnap, flowName],
  )

  const activeRunStatus = useMemo(() => {
    return runDetail?.status ?? runs.find((r) => r.id === selectedRunId)?.status ?? null
  }, [runDetail?.status, runs, selectedRunId])

  const selectedNode = useMemo(
    () => nodes.find((x) => x.id === selectedNodeId) ?? null,
    [nodes, selectedNodeId],
  )
  const selectedEdge = useMemo(
    () => edges.find((x) => x.id === selectedEdgeId) ?? null,
    [edges, selectedEdgeId],
  )

  const onConnect = useCallback(
    (c: Connection) => {
      setEdges((eds) => addEdge({ ...c, type: "smoothstep" }, eds))
    },
    [setEdges],
  )

  const onSelectionChange = useCallback(
    ({ nodes: ns, edges: es }: { nodes: Node[]; edges: Edge[] }) => {
      setSelectedNodeId(ns[0]?.id ?? null)
      setSelectedEdgeId(es[0]?.id ?? null)
    },
    [],
  )

  const addPaletteNode = useCallback(
    (type: string) => {
      const id = newId()
      const pos = screenToFlowPosition({ x: window.innerWidth * 0.45, y: 220 })
      setNodes((nds) => [
        ...nds,
        {
          id,
          type,
          position: { x: pos.x + nds.length * 14, y: pos.y + nds.length * 10 },
          data: { title: type },
        },
      ])
    },
    [screenToFlowPosition, setNodes],
  )

  const updateNodeData = useCallback(
    (id: string, data: Partial<FlowNodeData>) => {
      setNodes((nds) =>
        nds.map((n) =>
          n.id === id ? { ...n, data: { ...(n.data as FlowNodeData), ...data } } : n,
        ),
      )
    },
    [setNodes],
  )

  const updateEdgeLabel = useCallback(
    (edgeId: string, label: string) => {
      const t = label.trim().toLowerCase()
      setEdges((eds) =>
        eds.map((e) =>
          e.id === edgeId
            ? {
                ...e,
                sourceHandle: t || undefined,
                data: { ...e.data, label },
              }
            : e,
        ),
      )
    },
    [setEdges],
  )

  const loadRuns = useCallback(async () => {
    try {
      const list = await fetchFlowRuns()
      setRuns(list)
      if (selectedRunId) {
        setRunDetail(await fetchFlowRun(selectedRunId))
      }
    } catch {
      setRuns([])
    }
  }, [selectedRunId])

  useEffect(() => {
    void loadRuns()
    const t = setInterval(() => void loadRuns(), 5000)
    return () => clearInterval(t)
  }, [loadRuns])

  useEffect(() => {
    if (!selectedRunId) {
      setRunDetail(null)
      return
    }
    void (async () => {
      setRunDetail(await fetchFlowRun(selectedRunId))
    })()
  }, [selectedRunId])

  useEffect(() => {
    if (!selectedRunId) return
    let cancelled = false
    const tick = async () => {
      const r = await fetchFlowRun(selectedRunId)
      if (!cancelled && r) setRunDetail(r)
    }
    void tick()
    if (activeRunStatus !== "running" && activeRunStatus !== "pending") {
      return () => {
        cancelled = true
      }
    }
    const id = setInterval(() => void tick(), 700)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [selectedRunId, activeRunStatus])

  const openCode = () => {
    setCodeText(JSON.stringify(spec, null, 2))
    setCodeOpen(true)
  }

  const runEvaluate = () => {
    setBusy("v")
    setValidateMsg(null)
    setRunPanelTab("inputs")
    setRunPanelCollapsed(false)
    void validateFlow(spec).then((r) => {
      setValidateMsg(r.ok ? "Flow is valid and ready to run." : r.error ?? "Invalid")
      setBusy(null)
    })
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex shrink-0 items-center gap-3 border-b border-border/60 bg-card/60 px-3 py-2 backdrop-blur-sm md:px-4">
        {/* Left: back + name + badge */}
        <div className="flex min-w-0 items-center gap-1.5">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-8 shrink-0"
            aria-label="Back"
            onClick={() => setView("chat")}
          >
            <ChevronLeft className="size-4" />
          </Button>
          <Input
            className="h-8 max-w-[200px] border-0 bg-transparent px-1 text-sm font-semibold tracking-tight focus-visible:ring-0"
            value={flowName}
            onChange={(e) => setFlowName(e.target.value)}
            disabled={preview}
          />
          <Badge variant="secondary" className="shrink-0 text-[10px] font-normal">
            {loadedAgentId ? "Saved" : "Draft"}
          </Badge>
        </div>

        {/* Center: canvas tools */}
        <div className="flex items-center gap-1">
          <Button type="button" variant="ghost" size="icon" className="size-8" title="New agent" onClick={newAgent}>
            <Plus className="size-3.5" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-8"
            title="Open saved agent"
            onClick={() => { refreshLibrary(); setLibraryOpen(true) }}
          >
            <FolderOpen className="size-3.5" />
          </Button>
          <div className="mx-0.5 h-4 w-px bg-border/40" />
          <Button type="button" variant="ghost" size="icon" className="size-8" title="Edit name" onClick={() => setSettingsOpen(true)}>
            <Pencil className="size-3.5" />
          </Button>
          <Button type="button" variant="ghost" size="icon" className="size-8" title="Undo" disabled>
            <Undo2 className="size-3.5" />
          </Button>
          <Button type="button" variant="ghost" size="icon" className="size-8" title="Redo" disabled>
            <Redo2 className="size-3.5" />
          </Button>
        </div>

        <div className="flex-1" />

        {/* Right: actions */}
        <div className="flex items-center gap-1.5">
          <Button type="button" size="sm" variant="ghost" className="size-8 p-0" title="Settings" onClick={() => setSettingsOpen(true)}>
            <Settings className="size-4" />
          </Button>

          <div className="mx-1 h-5 w-px bg-border/60" />

          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-8 gap-1.5 text-xs font-medium"
            disabled={busy !== null}
            onClick={runEvaluate}
          >
            {busy === "v" ? <Loader2 className="size-3.5 animate-spin" /> : <Sparkles className="size-3.5" />}
            Evaluate
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-8 gap-1.5 text-xs font-medium"
            disabled={busy !== null || preview}
            title="Save to agent library"
            onClick={() => {
              setBusy("l")
              setValidateMsg(null)
              setRunPanelTab("inputs")
              setRunPanelCollapsed(false)
              void validateFlow(spec).then((v) => {
                if (!v.ok) {
                  setValidateMsg(v.error ?? "Fix the graph before saving to the library.")
                  setBusy(null)
                  return
                }
                const id = loadedAgentId ?? `user-flow-${crypto.randomUUID().slice(0, 12)}`
                void upsertAgentLibraryEntry({
                  id,
                  name: flowName.trim() || "Agent",
                  description: "Saved from agent builder",
                  kind: "flow",
                  flow_spec: spec,
                }).then((r) => {
                  setBusy(null)
                  if (r.ok) {
                    setLoadedAgentId(id)
                    refreshLibrary()
                  }
                  setValidateMsg(
                    r.ok
                      ? `Saved «${flowName.trim() || "Agent"}». Select it in Chat → Agents.`
                      : (r.error ?? "Could not save to library."),
                  )
                })
              })
            }}
          >
            {busy === "l" ? <Loader2 className="size-3.5 animate-spin" /> : <BookmarkPlus className="size-3.5" />}
            Save
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-8 gap-1.5 text-xs font-medium"
            onClick={openCode}
          >
            <Code2 className="size-3.5" />
            Code
          </Button>
          <Button
            type="button"
            size="sm"
            variant="default"
            className="h-8 gap-1.5 rounded-lg px-4 text-xs font-semibold"
            disabled={busy !== null}
            onClick={() => {
              setBusy("r")
              setRunMsg(null)
              setRunPanelTab("logs")
              setRunPanelCollapsed(false)
              let inputs: unknown = {}
              try {
                inputs = JSON.parse(inputsJson || "{}") as unknown
              } catch {
                setRunMsg("Inputs must be JSON object")
                setBusy(null)
                return
              }
              void kickoffFlow({ spec, inputs }).then((r) => {
                if (r.success && r.run_id) {
                  setRunMsg(`Run started — tracking ${r.run_id.slice(0, 8)}…`)
                  setSelectedRunId(r.run_id)
                  setPreview(true)
                  setRunPanelCollapsed(false)
                } else {
                  setRunMsg(r.error ?? "Kickoff failed")
                }
                setBusy(null)
                void loadRuns()
              })
            }}
          >
            {busy === "r" ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
            Publish
          </Button>
        </div>
      </header>

      <Dialog open={settingsOpen} onOpenChange={setSettingsOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Agent settings</DialogTitle>
            <DialogDescription>
              Agents execute on this node using your local models, the same <strong className="text-foreground">MCP</strong>{" "}
              servers as the MCP console / Settings, and the shared <strong className="text-foreground">vectX</strong> vector
              store. MCP nodes list tools from <code className="text-foreground/90">/api/mcp/status</code>. File search uses
              collection names from the API or console.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2 text-xs text-muted-foreground">
            <Label htmlFor="settings-flow-name">Display name</Label>
            <Input
              id="settings-flow-name"
              value={flowName}
              onChange={(e) => setFlowName(e.target.value)}
              className="h-9"
            />
            <p className="pt-2 text-[11px] leading-relaxed">
              <strong className="text-foreground">User approval</strong> nodes read{" "}
              <code className="text-foreground/90">inputs.human_approval</code> as{" "}
              <code className="text-foreground/90">approve</code> or <code className="text-foreground/90">reject</code>{" "}
              until pause/resume is implemented.
            </p>
          </div>
        </DialogContent>
      </Dialog>

      {/* Agent library browser */}
      <Dialog open={libraryOpen} onOpenChange={setLibraryOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>Saved agents</DialogTitle>
            <DialogDescription>
              Load an agent into the builder to edit it, or create a new one.
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-[50vh] space-y-1 overflow-y-auto">
            {library.length === 0 ? (
              <p className="py-6 text-center text-sm text-muted-foreground">
                No agents saved yet. Build one and click Save.
              </p>
            ) : (
              library.map((entry) => (
                <div
                  key={entry.id}
                  className={cn(
                    "flex items-center gap-3 rounded-lg border px-3 py-2.5 transition-colors",
                    loadedAgentId === entry.id
                      ? "border-primary/40 bg-primary/5"
                      : "border-border/50 hover:bg-muted/40",
                  )}
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium">{entry.name}</p>
                    {entry.description ? (
                      <p className="truncate text-xs text-muted-foreground">{entry.description}</p>
                    ) : null}
                    <p className="mt-0.5 font-mono text-[10px] text-muted-foreground/60">
                      {entry.id.slice(0, 16)}
                    </p>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="h-7 gap-1 text-xs"
                      disabled={!entry.flow_spec}
                      onClick={() => loadAgent(entry)}
                    >
                      <FolderOpen className="size-3" />
                      Edit
                    </Button>
                    <Button
                      type="button"
                      size="icon"
                      variant="ghost"
                      className="size-7 text-muted-foreground hover:text-destructive"
                      title="Delete agent"
                      onClick={() => void deleteAgent(entry.id)}
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </div>
                </div>
              ))
            )}
          </div>
          <div className="flex justify-between">
            <Button type="button" variant="outline" size="sm" className="gap-1.5 text-xs" onClick={newAgent}>
              <Plus className="size-3.5" />
              New agent
            </Button>
            <Button type="button" variant="ghost" size="sm" className="text-xs" onClick={() => setLibraryOpen(false)}>
              Close
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      <div className="flex min-h-0 flex-1">
        <aside className="hidden w-48 shrink-0 flex-col border-r border-border/50 bg-card/30 md:flex">
          <PaletteSidebar preview={preview} onAdd={addPaletteNode} />
        </aside>

        <div className="relative min-h-0 min-w-0 flex-1">
          <CanvasSync onGraph={onGraph} />
          <ReactFlow
            nodes={nodes}
            edges={edges}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={preview ? undefined : onConnect}
            onSelectionChange={onSelectionChange}
            nodeTypes={builderNodeTypes}
            nodesDraggable={!preview}
            nodesConnectable={!preview}
            elementsSelectable
            fitView
            fitViewOptions={{ padding: 0.25 }}
            deleteKeyCode={preview ? null : ["Backspace", "Delete"]}
            defaultEdgeOptions={{ type: "smoothstep", animated: false }}
            className="bg-muted/10"
          >
            <Background variant={BackgroundVariant.Dots} gap={20} size={0.8} className="opacity-40" />
            <Controls
              className="!rounded-xl !border-border/50 !bg-card/90 !shadow-lg !backdrop-blur-sm"
              position="bottom-center"
              showInteractive={false}
            />
            <Panel position="top-left" className="m-2 md:hidden">
              <div className="max-h-[50vh] w-40 overflow-hidden rounded-xl border border-border/50 bg-card/95 p-1 shadow-lg backdrop-blur-sm">
                <PaletteSidebar preview={preview} onAdd={addPaletteNode} />
              </div>
            </Panel>
          </ReactFlow>
        </div>

        <aside className="hidden w-[21rem] shrink-0 flex-col border-l border-border/50 bg-card/30 lg:flex">
          <ScrollArea className="flex-1 p-4">
            {selectedEdge ? (
              <EdgeInspector
                edge={selectedEdge}
                disabled={preview}
                onLabelChange={(lab) => updateEdgeLabel(selectedEdge.id, lab)}
              />
            ) : selectedNode ? (
              <FlowNodeInspector
                node={selectedNode}
                disabled={preview}
                modelIds={modelIds}
                mcpTools={mcpTools}
                mcpEnabled={mcpEnabled}
                mcpHint={mcpHint}
                onOpenMcpConsole={() => setView("mcp")}
                onRefreshMcpCatalog={refreshMcpCatalog}
                onChangeData={(patch) => updateNodeData(selectedNode.id, patch)}
              />
            ) : (
              <div className="space-y-3 py-4 text-center text-xs text-muted-foreground">
                <p className="text-sm font-medium text-foreground/60">No selection</p>
                <p className="text-[11px] leading-relaxed">
                  Click a node or edge to view and edit its properties.
                </p>
              </div>
            )}
          </ScrollArea>
        </aside>
      </div>

      {runPanelCollapsed ? (
        <div className="flex h-11 shrink-0 items-center gap-2 border-t border-border/80 bg-card/50 px-3 shadow-[0_-1px_0_0_hsl(var(--border)/0.5)] backdrop-blur-sm md:px-4">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 gap-1.5 border-dashed text-xs"
            onClick={() => setRunPanelCollapsed(false)}
          >
            <ChevronUp className="size-3.5" aria-hidden />
            Run console
          </Button>
          {selectedRunId ? (
            <Badge variant="secondary" className="max-w-[min(280px,50vw)] truncate font-mono text-[10px] font-normal">
              {selectedRunId.slice(0, 10)}… · {activeRunStatus ?? "…"}
            </Badge>
          ) : (
            <span className="text-[10px] text-muted-foreground">No run selected — expand for logs and inputs.</span>
          )}
        </div>
      ) : (
        <div
          className="flex shrink-0 flex-col overflow-hidden border-t border-border/80 bg-card/40 shadow-[0_-8px_32px_-12px_rgba(0,0,0,0.45)] backdrop-blur-md"
          style={{ height: runPanelHeight }}
        >
          <div
            role="separator"
            aria-orientation="horizontal"
            aria-label="Drag to resize run console"
            onPointerDown={onRunPanelResizePointerDown}
            className="group flex h-3 shrink-0 cursor-row-resize touch-none items-center justify-center border-b border-border/50 bg-muted/30 hover:bg-muted/50"
          >
            <GripHorizontal
              className="size-4 text-muted-foreground/50 group-hover:text-muted-foreground"
              aria-hidden
            />
          </div>
          <Tabs
            value={runPanelTab}
            onValueChange={(v) => setRunPanelTab(v as typeof runPanelTab)}
            className="flex min-h-0 flex-1 flex-col overflow-hidden"
          >
            <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border/50 px-3 py-1.5 md:px-4">
              <span className="hidden items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground sm:flex">
                <Terminal className="size-3.5 text-emerald-500/90" aria-hidden />
                Console
              </span>
              <TabsList className="h-8">
                <TabsTrigger value="logs" className="px-2.5 text-xs">
                  Logs
                </TabsTrigger>
                <TabsTrigger value="steps" className="px-2.5 text-xs">
                  Steps
                </TabsTrigger>
                <TabsTrigger value="json" className="px-2.5 text-xs">
                  Result JSON
                </TabsTrigger>
                <TabsTrigger value="inputs" className="px-2.5 text-xs">
                  Inputs
                </TabsTrigger>
              </TabsList>
              <div className="ml-auto flex flex-wrap items-center gap-1.5 text-[10px] text-muted-foreground">
                {selectedRunId ? (
                  <span className="hidden font-mono sm:inline">
                    {selectedRunId.slice(0, 10)}… · {activeRunStatus ?? "…"}
                  </span>
                ) : (
                  <span className="hidden sm:inline">No run selected</span>
                )}
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="size-7 shrink-0 text-muted-foreground"
                  aria-label="Minimize run console"
                  title="Minimize (saves canvas space)"
                  onClick={() => setRunPanelCollapsed(true)}
                >
                  <ChevronDown className="size-4" />
                </Button>
              </div>
            </div>
            <TabsContent
              value="logs"
              className="mt-0 flex min-h-0 flex-1 flex-col overflow-hidden px-3 pb-2 pt-2 data-[state=inactive]:hidden md:px-4"
            >
              <div className="min-h-0 flex-1 overflow-y-auto rounded-md border border-border/50 bg-muted/15 px-2 py-2">
                <pre className="whitespace-pre-wrap font-mono text-[11px] leading-relaxed text-foreground/90">
                  {runDetail?.logs?.length
                    ? runDetail.logs.join("\n")
                    : selectedRunId
                      ? "Waiting for log lines…"
                      : "Kick off a run to stream node-by-node logs from the node."}
                </pre>
              </div>
              {runDetail?.error ? (
                <p className="mt-2 shrink-0 rounded-md border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-xs text-destructive">
                  {runDetail.error}
                </p>
              ) : null}
            </TabsContent>
            <TabsContent
              value="steps"
              className="mt-0 flex min-h-0 flex-1 flex-col overflow-hidden px-3 pb-2 pt-2 data-[state=inactive]:hidden md:px-4"
            >
              <div className="min-h-0 flex-1 overflow-y-auto rounded-md border border-border/50 bg-muted/15 px-2 py-2">
                <ol className="list-decimal space-y-2 pl-4 text-xs">
                  {(runDetail?.output?.steps as unknown[] | undefined)?.length ? (
                    (runDetail!.output!.steps as Record<string, unknown>[]).map((s, i) => (
                      <li key={i} className="text-muted-foreground">
                        <span className="font-medium text-foreground">
                          {(s.kind as string) ?? (s.id as string) ?? "step"}
                        </span>
                        {s.id != null && s.kind != null ? (
                          <span className="text-muted-foreground"> · {String(s.id)}</span>
                        ) : null}
                        {s.branch != null ? (
                          <Badge variant="outline" className="ml-2 align-middle text-[9px]">
                            {String(s.branch)}
                          </Badge>
                        ) : null}
                      </li>
                    ))
                  ) : (
                    <li className="list-none pl-0 text-muted-foreground">No steps recorded yet.</li>
                  )}
                </ol>
              </div>
            </TabsContent>
            <TabsContent
              value="json"
              className="mt-0 flex min-h-0 flex-1 flex-col overflow-hidden px-3 pb-2 pt-2 data-[state=inactive]:hidden md:px-4"
            >
              <div className="min-h-0 flex-1 overflow-y-auto rounded-md border border-border/50 bg-muted/15 px-2 py-2">
                <pre className="whitespace-pre-wrap break-all font-mono text-[10px] text-foreground/85">
                  {runDetail?.output
                    ? JSON.stringify(runDetail.output, null, 2)
                    : runDetail?.status === "failed"
                      ? JSON.stringify({ error: runDetail.error }, null, 2)
                      : "{}"}
                </pre>
              </div>
            </TabsContent>
            <TabsContent
              value="inputs"
              className="mt-0 flex min-h-0 flex-1 flex-col gap-2 overflow-hidden px-3 pb-2 pt-2 data-[state=inactive]:hidden md:px-4"
            >
              <Label className="shrink-0 text-xs">Run inputs (JSON)</Label>
              <Textarea
                className="min-h-[5.5rem] flex-1 resize-y font-mono text-[11px]"
                value={inputsJson}
                onChange={(e) => setInputsJson(e.target.value)}
              />
              {validateMsg ? (
                <p
                  className={cn(
                    "shrink-0 text-xs",
                    validateMsg.includes("valid") ? "text-emerald-500" : "text-destructive",
                  )}
                >
                  {validateMsg}
                </p>
              ) : null}
              {runMsg ? <p className="shrink-0 text-xs text-muted-foreground">{runMsg}</p> : null}
            </TabsContent>
            <div className="shrink-0 border-t border-border/50 px-3 py-1.5 md:px-4">
              <p className="mb-1 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
                Recent runs
              </p>
              <div className="flex gap-1 overflow-x-auto overflow-y-hidden pb-0.5 [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                {runs.length === 0 ? (
                  <span className="text-[10px] text-muted-foreground">None yet — use Run or Evaluate.</span>
                ) : (
                  runs
                    .slice()
                    .reverse()
                    .slice(0, 16)
                    .map((r) => (
                      <button
                        key={r.id}
                        type="button"
                        className={cn(
                          "shrink-0 rounded-md border px-2 py-0.5 text-left text-[10px] transition-colors",
                          selectedRunId === r.id
                            ? "border-primary/50 bg-primary/10"
                            : "border-border/60 hover:bg-muted/50",
                        )}
                        onClick={() => {
                          setSelectedRunId(r.id)
                          setRunPanelTab("logs")
                        }}
                      >
                        <span className="font-mono">{r.id.slice(0, 8)}</span>{" "}
                        <span className="text-muted-foreground">{r.status}</span>
                      </button>
                    ))
                )}
              </div>
            </div>
          </Tabs>
        </div>
      )}

      <Dialog open={codeOpen} onOpenChange={setCodeOpen}>
        <DialogContent className="max-h-[85vh] max-w-2xl">
          <DialogHeader>
            <DialogTitle>Flow JSON</DialogTitle>
          </DialogHeader>
          <Textarea
            className="min-h-[320px] font-mono text-[11px]"
            value={codeText}
            onChange={(e) => setCodeText(e.target.value)}
          />
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" size="sm" onClick={() => setCodeOpen(false)}>
              Close
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={() => {
                try {
                  const p = JSON.parse(codeText) as FlowSpecJson
                  const { nodes: nn, edges: ee } = flowSpecToReactFlow(p)
                  setNodes(nn)
                  setEdges(ee)
                  setCodeOpen(false)
                  setValidateMsg("Imported workflow JSON.")
                } catch {
                  setValidateMsg("Invalid JSON")
                }
              }}
            >
              Import
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function EdgeInspector({
  edge,
  disabled,
  onLabelChange,
}: {
  edge: Edge
  disabled: boolean
  onLabelChange: (l: string) => void
}) {
  const [v, setV] = useState(() => (edge.data as { label?: string })?.label ?? edge.sourceHandle ?? "")
  useEffect(() => {
    setV((edge.data as { label?: string })?.label ?? edge.sourceHandle ?? "")
  }, [edge])
  return (
    <div className="space-y-2 text-xs">
      <p className="text-[11px] leading-relaxed text-muted-foreground">
        Set the branch label to match the source node handles:{" "}
        <span className="text-foreground/90">true</span> /{" "}
        <span className="text-foreground/90">false</span>,{" "}
        <span className="text-foreground/90">loop</span> /{" "}
        <span className="text-foreground/90">exit</span>,{" "}
        <span className="text-foreground/90">pass</span> /{" "}
        <span className="text-foreground/90">fail</span>,{" "}
        <span className="text-foreground/90">approve</span> /{" "}
        <span className="text-foreground/90">reject</span>.
      </p>
      <Label>Label (branch)</Label>
      <Input
        className="font-mono text-xs"
        disabled={disabled}
        placeholder="true / false / loop / exit / pass / fail / approve / reject"
        value={v}
        onChange={(e) => setV(e.target.value)}
        onBlur={() => onLabelChange(v)}
      />
    </div>
  )
}

export function AgentBuilderPage() {
  return (
    <ReactFlowProvider>
      <AgentBuilderInner />
    </ReactFlowProvider>
  )
}
