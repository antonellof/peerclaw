/* eslint-disable react-refresh/only-export-components -- nodeTypes map is required by React Flow */
import { memo, useState } from "react"
import { Handle, Position, useReactFlow, type NodeProps } from "@xyflow/react"
import {
  Bot,
  CirclePlay,
  Diamond,
  FileSearch,
  Flag,
  GitBranch,
  LayoutList,
  MessageSquare,
  Repeat,
  Shield,
  StickyNote,
  UserCheck,
  Wrench,
  X,
} from "lucide-react"

import { cn } from "@/lib/utils"

import type { FlowNodeData } from "./flowCompile"

/* ── Shared pill node shell ─────────────────────────────────────────── */

function shell(
  selected: boolean,
  accent: string,
  icon: React.ReactNode,
  label: string,
  onDelete?: () => void,
) {
  const [hovered, setHovered] = useState(false)
  return (
    <div
      className={cn(
        "group relative flex items-center gap-2.5 rounded-full border bg-card px-3 py-1.5 shadow-sm transition-all",
        "border-border/60 hover:shadow-md",
        selected && "ring-2 ring-primary ring-offset-2 ring-offset-background",
      )}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <span
        className={cn(
          "flex size-7 shrink-0 items-center justify-center rounded-full",
          accent,
        )}
      >
        {icon}
      </span>
      <span className="pr-1 text-[13px] font-medium leading-tight text-foreground">
        {label}
      </span>
      {/* Delete button — appears on hover */}
      {onDelete && hovered && (
        <button
          type="button"
          className="absolute -right-1.5 -top-1.5 flex size-5 items-center justify-center rounded-full border border-border bg-card text-muted-foreground shadow-sm transition-colors hover:bg-destructive hover:text-destructive-foreground"
          onClick={(e) => {
            e.stopPropagation()
            onDelete()
          }}
          title="Remove node"
        >
          <X className="size-3" />
        </button>
      )}
    </div>
  )
}

/* ── Handle styles ──────────────────────────────────────────────────── */

const hTarget = "!size-2.5 !rounded-full !border-2 !border-background !bg-muted-foreground/60"
const hSource = "!size-2.5 !rounded-full !border-2 !border-background !bg-primary"
const hGreen = "!size-2.5 !rounded-full !border-2 !border-background !bg-emerald-500"
const hRed = "!size-2.5 !rounded-full !border-2 !border-background !bg-rose-500"
const hCyan = "!size-2.5 !rounded-full !border-2 !border-background !bg-cyan-500"

/* ── Hook: delete this node ─────────────────────────────────────────── */

function useDeleteNode(id: string) {
  const { deleteElements } = useReactFlow()
  return () => {
    void deleteElements({ nodes: [{ id }] })
  }
}

/* ── Nodes ──────────────────────────────────────────────────────────── */

export const StartNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-emerald-500 text-white",
        <CirclePlay className="size-3.5" />,
        d.title || "Start",
        del,
      )}
    </>
  )
})
StartNode.displayName = "StartNode"

export const EndNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      {shell(
        !!selected,
        "bg-slate-400 text-white dark:bg-slate-500",
        <Flag className="size-3.5" />,
        d.title || "End",
        del,
      )}
    </>
  )
})
EndNode.displayName = "EndNode"

export const NoteNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  const [hovered, setHovered] = useState(false)
  return (
    <div
      className={cn(
        "group relative max-w-[240px] rounded-lg bg-amber-100 px-3.5 py-2.5 shadow-sm dark:bg-amber-400/20",
        "border border-amber-300/50 dark:border-amber-500/30",
        selected && "ring-2 ring-amber-400/60 ring-offset-2 ring-offset-background",
      )}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold text-amber-800 dark:text-amber-200">
        <StickyNote className="size-3.5" />
        Sticky note
      </div>
      <p className="whitespace-pre-wrap text-[11px] leading-snug text-amber-900/80 dark:text-amber-100/80">
        {d.prompt || d.title || "Annotation"}
      </p>
      {hovered && (
        <button
          type="button"
          className="absolute -right-1.5 -top-1.5 flex size-5 items-center justify-center rounded-full border border-border bg-card text-muted-foreground shadow-sm transition-colors hover:bg-destructive hover:text-destructive-foreground"
          onClick={(e) => {
            e.stopPropagation()
            del()
          }}
          title="Remove note"
        >
          <X className="size-3" />
        </button>
      )}
    </div>
  )
})
NoteNode.displayName = "NoteNode"

export const ClassifyNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-amber-500 text-white",
        <LayoutList className="size-3.5" />,
        d.title || "Classify",
        del,
      )}
    </>
  )
})
ClassifyNode.displayName = "ClassifyNode"

export const UserApprovalNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle
        type="source"
        position={Position.Bottom}
        id="approve"
        className={hGreen}
        style={{ left: "35%" }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        id="reject"
        className={hRed}
        style={{ left: "65%" }}
      />
      {shell(
        !!selected,
        "bg-orange-500 text-white",
        <UserCheck className="size-3.5" />,
        d.title || "User approval",
        del,
      )}
    </>
  )
})
UserApprovalNode.displayName = "UserApprovalNode"

export const LlmNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-violet-500 text-white",
        <MessageSquare className="size-3.5" />,
        d.title || "LLM",
        del,
      )}
    </>
  )
})
LlmNode.displayName = "LlmNode"

export const AgentNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-amber-500 text-white",
        <Bot className="size-3.5" />,
        d.title || "Agent",
        del,
      )}
    </>
  )
})
AgentNode.displayName = "AgentNode"

export const IfNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle
        type="source"
        position={Position.Bottom}
        id="true"
        className={hGreen}
        style={{ left: "35%" }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        id="false"
        className={hRed}
        style={{ left: "65%" }}
      />
      {shell(
        !!selected,
        "bg-fuchsia-500 text-white",
        <GitBranch className="size-3.5" />,
        d.title || "If / else",
        del,
      )}
    </>
  )
})
IfNode.displayName = "IfNode"

export const WhileNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle
        type="source"
        position={Position.Bottom}
        id="loop"
        className={hCyan}
        style={{ left: "35%" }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        id="exit"
        className={hTarget}
        style={{ left: "65%" }}
      />
      {shell(
        !!selected,
        "bg-cyan-500 text-white",
        <Repeat className="size-3.5" />,
        d.title || "While",
        del,
      )}
    </>
  )
})
WhileNode.displayName = "WhileNode"

export const GuardrailsNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle
        type="source"
        position={Position.Bottom}
        id="pass"
        className={hGreen}
        style={{ left: "35%" }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        id="fail"
        className={hRed}
        style={{ left: "65%" }}
      />
      {shell(
        !!selected,
        "bg-orange-500 text-white",
        <Shield className="size-3.5" />,
        d.title || "Guardrails",
        del,
      )}
    </>
  )
})
GuardrailsNode.displayName = "GuardrailsNode"

export const McpNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-indigo-500 text-white",
        <Wrench className="size-3.5" />,
        d.title || d.mcpToolId || "MCP",
        del,
      )}
    </>
  )
})
McpNode.displayName = "McpNode"

export const FileSearchNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-teal-500 text-white",
        <FileSearch className="size-3.5" />,
        d.title || d.vectorCollection || "File search",
        del,
      )}
    </>
  )
})
FileSearchNode.displayName = "FileSearchNode"

export const SetStateNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-lime-500 text-white",
        <Diamond className="size-3.5" />,
        d.title || d.stateKey || "Set state",
        del,
      )}
    </>
  )
})
SetStateNode.displayName = "SetStateNode"

export const TransformNode = memo(({ id, data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  const del = useDeleteNode(id)
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-amber-600 text-white",
        <Diamond className="size-3.5" />,
        d.title || "Transform",
        del,
      )}
    </>
  )
})
TransformNode.displayName = "TransformNode"

export const builderNodeTypes = {
  start: StartNode,
  end: EndNode,
  note: NoteNode,
  llm: LlmNode,
  agent: AgentNode,
  classify: ClassifyNode,
  userApproval: UserApprovalNode,
  if: IfNode,
  while: WhileNode,
  guardrails: GuardrailsNode,
  mcp: McpNode,
  fileSearch: FileSearchNode,
  setState: SetStateNode,
  transform: TransformNode,
}
