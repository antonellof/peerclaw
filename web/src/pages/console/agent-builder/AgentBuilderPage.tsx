import { useCallback, useEffect, useMemo, useState } from "react"
import {
  addEdge,
  Background,
  BackgroundVariant,
  Controls,
  type Connection,
  type Edge,
  MiniMap,
  type Node,
  Panel,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  useReactFlow,
  useStore,
} from "@xyflow/react"
import { Code2, Loader2, Play } from "lucide-react"

import {
  fetchFlowRun,
  fetchFlowRuns,
  kickoffFlow,
  validateFlow,
  type FlowRunRecordJson,
  type FlowSpecJson,
} from "@/lib/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

import { builderNodeTypes } from "./builderNodes"
import { compileFlowSpec, demoGraph, flowSpecToReactFlow, type FlowNodeData } from "./flowCompile"

const DEMO_INPUTS = '{\n  "topic": "Rust async",\n  "input_as_text": "Summarize async in Rust"\n}'

function newId() {
  return `n_${crypto.randomUUID().slice(0, 8)}`
}

type PaletteItem = { type: string; label: string; section: string }

const PALETTE: PaletteItem[] = [
  { section: "Core", type: "start", label: "Start" },
  { section: "Core", type: "agent", label: "Agent" },
  { section: "Core", type: "llm", label: "LLM" },
  { section: "Core", type: "end", label: "End" },
  { section: "Core", type: "note", label: "Note" },
  { section: "Tools", type: "fileSearch", label: "File search" },
  { section: "Tools", type: "guardrails", label: "Guardrails" },
  { section: "Tools", type: "mcp", label: "MCP" },
  { section: "Logic", type: "if", label: "If / else" },
  { section: "Logic", type: "while", label: "While" },
  { section: "Data", type: "transform", label: "Transform" },
  { section: "Data", type: "setState", label: "Set state" },
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
    <ScrollArea className="flex-1 px-2 pb-4">
      {["Core", "Tools", "Logic", "Data"].map((sec) => (
        <div key={sec} className="mb-3">
          <p className="mb-1.5 px-1 text-[10px] font-medium text-muted-foreground">{sec}</p>
          <div className="flex flex-col gap-1">
            {PALETTE.filter((p) => p.section === sec).map((p) => (
              <Button
                key={p.type}
                type="button"
                variant="outline"
                size="sm"
                className="h-auto justify-start py-2 text-left text-xs"
                disabled={preview}
                onClick={() => onAdd(p.type)}
              >
                {p.label}
              </Button>
            ))}
          </div>
        </div>
      ))}
    </ScrollArea>
  )
}

function AgentBuilderInner() {
  const starter = useMemo(() => demoGraph(), [])
  const [preview, setPreview] = useState(false)
  const [flowName, setFlowName] = useState("New workflow")
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
  const [inputsJson, setInputsJson] = useState(DEMO_INPUTS)
  const [codeOpen, setCodeOpen] = useState(false)
  const [codeText, setCodeText] = useState("")
  const [validateMsg, setValidateMsg] = useState<string | null>(null)
  const [runMsg, setRunMsg] = useState<string | null>(null)
  const [busy, setBusy] = useState<"v" | "r" | null>(null)
  const [runs, setRuns] = useState<FlowRunRecordJson[]>([])
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null)
  const [runDetail, setRunDetail] = useState<FlowRunRecordJson | null>(null)

  const spec = useMemo(
    () => compileFlowSpec(nodesSnap, edgesSnap, flowName.trim() || "workflow"),
    [nodesSnap, edgesSnap, flowName],
  )

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
    const t = setInterval(() => void loadRuns(), 3000)
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

  const openCode = () => {
    setCodeText(JSON.stringify(spec, null, 2))
    setCodeOpen(true)
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border/80 px-3 py-2 md:px-4">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <Input
            className="h-8 max-w-[220px] border-0 bg-transparent px-1 text-sm font-semibold focus-visible:ring-0"
            value={flowName}
            onChange={(e) => setFlowName(e.target.value)}
          />
          <Badge variant="secondary" className="text-[10px] font-normal">
            Draft
          </Badge>
        </div>
        <div className="flex flex-wrap items-center gap-1">
          <Button
            type="button"
            size="sm"
            variant={preview ? "default" : "outline"}
            className="h-8 gap-1 text-xs"
            onClick={() => setPreview((p) => !p)}
          >
            <Play className="size-3.5" />
            {preview ? "Preview" : "Edit"}
          </Button>
          <Button type="button" size="sm" variant="outline" className="h-8 gap-1 text-xs" onClick={openCode}>
            <Code2 className="size-3.5" />
            Code
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-8 text-xs"
            disabled={busy !== null}
            onClick={() => {
              setBusy("v")
              setValidateMsg(null)
              void validateFlow(spec).then((r) => {
                setValidateMsg(r.ok ? "Flow is valid." : r.error ?? "Invalid")
                setBusy(null)
              })
            }}
          >
            {busy === "v" ? <Loader2 className="size-3.5 animate-spin" /> : null}
            Validate
          </Button>
          <Button
            type="button"
            size="sm"
            className="h-8 gap-1 text-xs"
            disabled={busy !== null}
            onClick={() => {
              setBusy("r")
              setRunMsg(null)
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
                  setRunMsg(`Started ${r.run_id}`)
                  setSelectedRunId(r.run_id)
                } else {
                  setRunMsg(r.error ?? "Kickoff failed")
                }
                setBusy(null)
                void loadRuns()
              })
            }}
          >
            {busy === "r" ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
            Run
          </Button>
        </div>
      </header>

      <div className="flex min-h-0 flex-1">
        <aside className="hidden w-48 shrink-0 flex-col border-r border-border/80 bg-card/30 md:flex">
          <p className="px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            Add nodes
          </p>
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
            fitViewOptions={{ padding: 0.2 }}
            deleteKeyCode={preview ? null : ["Backspace", "Delete"]}
            className="bg-muted/20"
          >
            <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
            <Controls className="!bg-card !border-border !shadow-md" />
            <MiniMap
              className="!bg-card/90 !border-border"
              maskColor="hsl(240 6% 10% / 0.65)"
              nodeColor={() => "hsl(142 76% 36%)"}
            />
            <Panel position="top-left" className="m-2 md:hidden">
              <div className="max-h-[50vh] w-40 overflow-hidden rounded-lg border border-border bg-card/95 p-1 shadow-md">
                <PaletteSidebar preview={preview} onAdd={addPaletteNode} />
              </div>
            </Panel>
          </ReactFlow>
        </div>

        <aside className="hidden w-80 shrink-0 flex-col border-l border-border/80 bg-card/30 lg:flex">
          <p className="border-b border-border/60 px-3 py-2 text-xs font-semibold">Inspector</p>
          <ScrollArea className="flex-1 p-3">
            {selectedEdge ? (
              <EdgeInspector
                edge={selectedEdge}
                disabled={preview}
                onLabelChange={(lab) => updateEdgeLabel(selectedEdge.id, lab)}
              />
            ) : selectedNode ? (
              <NodeInspector
                node={selectedNode}
                disabled={preview}
                onChangeData={(patch) => updateNodeData(selectedNode.id, patch)}
              />
            ) : (
              <div className="space-y-2 text-xs text-muted-foreground">
                <p>Select a node or edge.</p>
                <p className="text-[10px] leading-relaxed">
                  Workflow must include one <strong className="text-foreground/90">Start</strong> for branching mode. CEL:
                  variables <code className="text-foreground/80">inputs</code>,{" "}
                  <code className="text-foreground/80">outputs</code>, <code className="text-foreground/80">state</code>,{" "}
                  <code className="text-foreground/80">input_as_text</code>,{" "}
                  <code className="text-foreground/80">iteration</code>.
                </p>
              </div>
            )}
          </ScrollArea>
        </aside>
      </div>

      <div className="shrink-0 border-t border-border/80 bg-card/20 px-3 py-3 md:px-4">
        <Label className="text-xs">Run inputs (JSON)</Label>
        <Textarea
          className="mt-1 font-mono text-[11px]"
          rows={3}
          value={inputsJson}
          onChange={(e) => setInputsJson(e.target.value)}
        />
        {validateMsg ? (
          <p className={cn("mt-1 text-xs", validateMsg.includes("valid") ? "text-emerald-500" : "text-destructive")}>
            {validateMsg}
          </p>
        ) : null}
        {runMsg ? <p className="mt-1 text-xs text-muted-foreground">{runMsg}</p> : null}
        <div className="mt-2 max-h-28 overflow-auto text-[10px] text-muted-foreground">
          {runs.length === 0 ? (
            <span>No flow runs yet.</span>
          ) : (
            <ul className="space-y-1">
              {runs
                .slice()
                .reverse()
                .slice(0, 8)
                .map((r) => (
                  <li key={r.id}>
                    <button
                      type="button"
                      className={cn(
                        "w-full rounded border px-2 py-1 text-left",
                        selectedRunId === r.id ? "border-primary/40 bg-primary/5" : "border-border/60",
                      )}
                      onClick={() => setSelectedRunId(r.id)}
                    >
                      <span className="font-mono">{r.id.slice(0, 8)}…</span> {r.status}
                    </button>
                  </li>
                ))}
            </ul>
          )}
        </div>
        {runDetail?.output ? (
          <pre className="mt-2 max-h-24 overflow-auto rounded border border-border/60 bg-muted/20 p-2 font-mono text-[9px]">
            {JSON.stringify(runDetail.output, null, 2)}
          </pre>
        ) : null}
      </div>

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
      <p className="text-muted-foreground">Edge</p>
      <Label>Label (branch)</Label>
      <Input
        className="font-mono text-xs"
        disabled={disabled}
        placeholder="true / false / loop / exit / pass / fail"
        value={v}
        onChange={(e) => setV(e.target.value)}
        onBlur={() => onLabelChange(v)}
      />
    </div>
  )
}

function NodeInspector({
  node,
  disabled,
  onChangeData,
}: {
  node: Node
  disabled: boolean
  onChangeData: (patch: Partial<FlowNodeData>) => void
}) {
  const d = (node.data || {}) as FlowNodeData
  return (
    <div className="space-y-3 text-xs">
      <div>
        <Label>Name</Label>
        <Input
          className="mt-1 h-8 text-xs"
          disabled={disabled}
          value={d.title ?? ""}
          onChange={(e) => onChangeData({ title: e.target.value })}
        />
      </div>
      {(node.type === "agent" || node.type === "llm") && (
        <>
          {node.type === "agent" ? (
            <div>
              <Label>Instructions</Label>
              <Textarea
                className="mt-1 text-xs"
                rows={4}
                disabled={disabled}
                value={d.instructions ?? ""}
                onChange={(e) => onChangeData({ instructions: e.target.value })}
              />
            </div>
          ) : null}
          <div>
            <Label>Model (empty = node default)</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              value={d.model ?? ""}
              onChange={(e) => onChangeData({ model: e.target.value })}
            />
          </div>
          {node.type === "agent" ? (
            <div>
              <Label>Tools (comma-separated)</Label>
              <Input
                className="mt-1 h-8 font-mono text-[11px]"
                disabled={disabled}
                value={d.toolsStr ?? ""}
                onChange={(e) => onChangeData({ toolsStr: e.target.value })}
              />
            </div>
          ) : null}
          <div>
            <Label>Prompt / task</Label>
            <Textarea
              className="mt-1 text-xs"
              rows={3}
              disabled={disabled}
              value={d.prompt ?? ""}
              onChange={(e) => onChangeData({ prompt: e.target.value })}
            />
          </div>
        </>
      )}
      {(node.type === "if" || node.type === "while") && (
        <div>
          <Label>Condition (CEL)</Label>
          <Textarea
            className="mt-1 font-mono text-[11px]"
            rows={2}
            disabled={disabled}
            value={d.conditionCel ?? ""}
            onChange={(e) => onChangeData({ conditionCel: e.target.value })}
          />
        </div>
      )}
      {node.type === "while" && (
        <div>
          <Label>Max iterations</Label>
          <Input
            type="number"
            className="mt-1 h-8"
            disabled={disabled}
            value={d.maxIterations ?? 100}
            onChange={(e) => onChangeData({ maxIterations: parseInt(e.target.value, 10) || 100 })}
          />
        </div>
      )}
      {node.type === "guardrails" && (
        <>
          <div>
            <Label>Source node id</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              value={d.sourceNodeId ?? ""}
              onChange={(e) => onChangeData({ sourceNodeId: e.target.value })}
            />
          </div>
          <div>
            <Label>Checks (comma)</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              value={d.guardrailChecksStr ?? ""}
              onChange={(e) => onChangeData({ guardrailChecksStr: e.target.value })}
            />
          </div>
        </>
      )}
      {node.type === "mcp" && (
        <>
          <div>
            <Label>Tool id (server:tool)</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              value={d.mcpToolId ?? ""}
              onChange={(e) => onChangeData({ mcpToolId: e.target.value })}
            />
          </div>
          <div>
            <Label>Arguments JSON</Label>
            <Textarea
              className="mt-1 font-mono text-[11px]"
              rows={3}
              disabled={disabled}
              value={d.mcpArgsJson ?? "{}"}
              onChange={(e) => onChangeData({ mcpArgsJson: e.target.value })}
            />
          </div>
        </>
      )}
      {node.type === "fileSearch" && (
        <>
          <div>
            <Label>Collection</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              value={d.vectorCollection ?? ""}
              onChange={(e) => onChangeData({ vectorCollection: e.target.value })}
            />
          </div>
          <div>
            <Label>Query template</Label>
            <Textarea
              className="mt-1 font-mono text-[11px]"
              rows={2}
              disabled={disabled}
              value={d.vectorQuery ?? ""}
              onChange={(e) => onChangeData({ vectorQuery: e.target.value })}
            />
          </div>
        </>
      )}
      {(node.type === "setState" || node.type === "transform") && (
        <>
          {node.type === "transform" ? (
            <div>
              <Label>From node id</Label>
              <Input
                className="mt-1 h-8 font-mono text-[11px]"
                disabled={disabled}
                value={d.transformFrom ?? ""}
                onChange={(e) => onChangeData({ transformFrom: e.target.value })}
              />
            </div>
          ) : null}
          <div>
            <Label>State key</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              value={d.stateKey ?? ""}
              onChange={(e) => onChangeData({ stateKey: e.target.value })}
            />
          </div>
          {node.type === "setState" ? (
            <div>
              <Label>Value JSON</Label>
              <Textarea
                className="mt-1 font-mono text-[11px]"
                rows={2}
                disabled={disabled}
                value={d.stateValueJson ?? "null"}
                onChange={(e) => onChangeData({ stateValueJson: e.target.value })}
              />
            </div>
          ) : null}
        </>
      )}
      {node.type === "note" && (
        <div>
          <Label>Note text</Label>
          <Textarea
            className="mt-1 text-xs"
            rows={4}
            disabled={disabled}
            value={d.prompt ?? ""}
            onChange={(e) => onChangeData({ prompt: e.target.value })}
          />
        </div>
      )}
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
