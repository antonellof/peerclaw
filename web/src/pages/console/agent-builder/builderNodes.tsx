/* eslint-disable react-refresh/only-export-components -- nodeTypes map is required by React Flow */
import { memo } from "react"
import { Handle, Position, type NodeProps } from "@xyflow/react"
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
} from "lucide-react"

import { cn } from "@/lib/utils"

import type { FlowNodeData } from "./flowCompile"

/* ── Shared pill node shell ─────────────────────────────────────────── */

function shell(
  selected: boolean,
  accent: string,
  icon: React.ReactNode,
  label: string,
) {
  return (
    <div
      className={cn(
        "flex items-center gap-2.5 rounded-full border bg-card px-3 py-1.5 shadow-sm transition-all",
        "border-border/60 hover:shadow-md",
        selected && "ring-2 ring-primary ring-offset-2 ring-offset-background",
      )}
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
    </div>
  )
}

/* ── Handle styles ──────────────────────────────────────────────────── */

const hTarget = "!size-2.5 !rounded-full !border-2 !border-background !bg-muted-foreground/60"
const hSource = "!size-2.5 !rounded-full !border-2 !border-background !bg-primary"
const hGreen = "!size-2.5 !rounded-full !border-2 !border-background !bg-emerald-500"
const hRed = "!size-2.5 !rounded-full !border-2 !border-background !bg-rose-500"
const hCyan = "!size-2.5 !rounded-full !border-2 !border-background !bg-cyan-500"

/* ── Nodes ──────────────────────────────────────────────────────────── */

export const StartNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-emerald-500 text-white",
        <CirclePlay className="size-3.5" />,
        d.title || "Start",
      )}
    </>
  )
})
StartNode.displayName = "StartNode"

export const EndNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      {shell(
        !!selected,
        "bg-slate-400 text-white dark:bg-slate-500",
        <Flag className="size-3.5" />,
        d.title || "End",
      )}
    </>
  )
})
EndNode.displayName = "EndNode"

export const NoteNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <div
      className={cn(
        "max-w-[240px] rounded-lg bg-amber-100 px-3.5 py-2.5 shadow-sm dark:bg-amber-400/20",
        "border border-amber-300/50 dark:border-amber-500/30",
        selected && "ring-2 ring-amber-400/60 ring-offset-2 ring-offset-background",
      )}
    >
      <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold text-amber-800 dark:text-amber-200">
        <StickyNote className="size-3.5" />
        Sticky note
      </div>
      <p className="whitespace-pre-wrap text-[11px] leading-snug text-amber-900/80 dark:text-amber-100/80">
        {d.prompt || d.title || "Annotation"}
      </p>
    </div>
  )
})
NoteNode.displayName = "NoteNode"

export const ClassifyNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-amber-500 text-white",
        <LayoutList className="size-3.5" />,
        d.title || "Classify",
      )}
    </>
  )
})
ClassifyNode.displayName = "ClassifyNode"

export const UserApprovalNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
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
      )}
    </>
  )
})
UserApprovalNode.displayName = "UserApprovalNode"

export const LlmNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-violet-500 text-white",
        <MessageSquare className="size-3.5" />,
        d.title || "LLM",
      )}
    </>
  )
})
LlmNode.displayName = "LlmNode"

export const AgentNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-amber-500 text-white",
        <Bot className="size-3.5" />,
        d.title || "Agent",
      )}
    </>
  )
})
AgentNode.displayName = "AgentNode"

export const IfNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
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
      )}
    </>
  )
})
IfNode.displayName = "IfNode"

export const WhileNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
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
      )}
    </>
  )
})
WhileNode.displayName = "WhileNode"

export const GuardrailsNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
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
      )}
    </>
  )
})
GuardrailsNode.displayName = "GuardrailsNode"

export const McpNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-indigo-500 text-white",
        <Wrench className="size-3.5" />,
        d.title || d.mcpToolId || "MCP",
      )}
    </>
  )
})
McpNode.displayName = "McpNode"

export const FileSearchNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-teal-500 text-white",
        <FileSearch className="size-3.5" />,
        d.title || d.vectorCollection || "File search",
      )}
    </>
  )
})
FileSearchNode.displayName = "FileSearchNode"

export const SetStateNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-lime-500 text-white",
        <Diamond className="size-3.5" />,
        d.title || d.stateKey || "Set state",
      )}
    </>
  )
})
SetStateNode.displayName = "SetStateNode"

export const TransformNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className={hTarget} />
      <Handle type="source" position={Position.Bottom} className={hSource} />
      {shell(
        !!selected,
        "bg-amber-600 text-white",
        <Diamond className="size-3.5" />,
        d.title || "Transform",
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
