/* eslint-disable react-refresh/only-export-components -- nodeTypes map is required by React Flow */
import { memo } from "react"
import { Handle, Position, type NodeProps } from "@xyflow/react"
import { Bot, CirclePlay, Diamond, FileSearch, Flag, GitBranch, MessageSquare, Repeat, Shield, StickyNote, Wrench } from "lucide-react"

import { cn } from "@/lib/utils"

import type { FlowNodeData } from "./flowCompile"

function shell(
  selected: boolean,
  accent: string,
  icon: React.ReactNode,
  subtitle: string,
  title: string,
  children?: React.ReactNode,
) {
  return (
    <div
      className={cn(
        "min-w-[168px] rounded-xl border border-border bg-card px-3 py-2.5 shadow-sm",
        selected && "ring-2 ring-primary ring-offset-2 ring-offset-background",
      )}
    >
      <div className="flex items-start gap-2">
        <span className={cn("mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg", accent)}>{icon}</span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-xs font-semibold leading-tight text-foreground">{title}</p>
          <p className="text-[10px] text-muted-foreground">{subtitle}</p>
        </div>
      </div>
      {children}
    </div>
  )
}

export const StartNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="source" position={Position.Bottom} className="!size-2 !border-border !bg-primary" />
      {shell(
        !!selected,
        "bg-emerald-500/15 text-emerald-400",
        <CirclePlay className="size-4" />,
        "Start",
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
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      {shell(
        !!selected,
        "bg-slate-500/15 text-slate-300",
        <Flag className="size-4" />,
        "End",
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
        "max-w-[220px] rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-100/90",
        selected && "ring-2 ring-amber-400/50",
      )}
    >
      <div className="mb-1 flex items-center gap-1.5 font-medium text-amber-200/90">
        <StickyNote className="size-3.5" />
        Note
      </div>
      <p className="whitespace-pre-wrap text-[11px] leading-snug">{d.prompt || d.title || "Annotation"}</p>
    </div>
  )
})
NoteNode.displayName = "NoteNode"

export const LlmNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      <Handle type="source" position={Position.Bottom} className="!size-2 !border-border !bg-primary" />
      {shell(
        !!selected,
        "bg-violet-500/15 text-violet-300",
        <MessageSquare className="size-4" />,
        "LLM",
        d.title || "LLM step",
      )}
    </>
  )
})
LlmNode.displayName = "LlmNode"

export const AgentNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      <Handle type="source" position={Position.Bottom} className="!size-2 !border-border !bg-primary" />
      {shell(
        !!selected,
        "bg-sky-500/15 text-sky-300",
        <Bot className="size-4" />,
        "Agent",
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
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      <Handle
        type="source"
        position={Position.Bottom}
        id="true"
        className="!size-2 !border-border !bg-emerald-500"
        style={{ left: "35%" }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        id="false"
        className="!size-2 !border-border !bg-rose-500"
        style={{ left: "65%" }}
      />
      {shell(
        !!selected,
        "bg-fuchsia-500/15 text-fuchsia-300",
        <GitBranch className="size-4" />,
        "If / else",
        d.title || "Condition",
        <p className="mt-1 line-clamp-2 font-mono text-[9px] text-muted-foreground">{d.conditionCel || "true"}</p>,
      )}
    </>
  )
})
IfNode.displayName = "IfNode"

export const WhileNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      <Handle
        type="source"
        position={Position.Bottom}
        id="loop"
        className="!size-2 !border-border !bg-cyan-500"
        style={{ left: "35%" }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        id="exit"
        className="!size-2 !border-border !bg-muted-foreground"
        style={{ left: "65%" }}
      />
      {shell(
        !!selected,
        "bg-cyan-500/15 text-cyan-300",
        <Repeat className="size-4" />,
        "While",
        d.title || "Loop",
        <p className="mt-1 line-clamp-2 font-mono text-[9px] text-muted-foreground">{d.conditionCel || ""}</p>,
      )}
    </>
  )
})
WhileNode.displayName = "WhileNode"

export const GuardrailsNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      <Handle
        type="source"
        position={Position.Bottom}
        id="pass"
        className="!size-2 !border-border !bg-emerald-500"
        style={{ left: "35%" }}
      />
      <Handle
        type="source"
        position={Position.Bottom}
        id="fail"
        className="!size-2 !border-border !bg-rose-500"
        style={{ left: "65%" }}
      />
      {shell(
        !!selected,
        "bg-orange-500/15 text-orange-300",
        <Shield className="size-4" />,
        "Guardrails",
        d.title || "Safety check",
      )}
    </>
  )
})
GuardrailsNode.displayName = "GuardrailsNode"

export const McpNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      <Handle type="source" position={Position.Bottom} className="!size-2 !border-border !bg-primary" />
      {shell(
        !!selected,
        "bg-indigo-500/15 text-indigo-300",
        <Wrench className="size-4" />,
        "MCP",
        d.title || d.mcpToolId || "MCP tool",
      )}
    </>
  )
})
McpNode.displayName = "McpNode"

export const FileSearchNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      <Handle type="source" position={Position.Bottom} className="!size-2 !border-border !bg-primary" />
      {shell(
        !!selected,
        "bg-teal-500/15 text-teal-300",
        <FileSearch className="size-4" />,
        "File search",
        d.title || d.vectorCollection || "Vector search",
      )}
    </>
  )
})
FileSearchNode.displayName = "FileSearchNode"

export const SetStateNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      <Handle type="source" position={Position.Bottom} className="!size-2 !border-border !bg-primary" />
      {shell(
        !!selected,
        "bg-lime-500/15 text-lime-300",
        <Diamond className="size-4" />,
        "Set state",
        d.title || d.stateKey || "state",
      )}
    </>
  )
})
SetStateNode.displayName = "SetStateNode"

export const TransformNode = memo(({ data, selected }: NodeProps) => {
  const d = (data || {}) as FlowNodeData
  return (
    <>
      <Handle type="target" position={Position.Top} className="!size-2 !border-border !bg-muted-foreground" />
      <Handle type="source" position={Position.Bottom} className="!size-2 !border-border !bg-primary" />
      {shell(
        !!selected,
        "bg-amber-500/15 text-amber-300",
        <Diamond className="size-4" />,
        "Transform",
        d.title || "Copy to state",
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
  if: IfNode,
  while: WhileNode,
  guardrails: GuardrailsNode,
  mcp: McpNode,
  fileSearch: FileSearchNode,
  setState: SetStateNode,
  transform: TransformNode,
}
