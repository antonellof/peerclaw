import type { Edge, Node } from "@xyflow/react"

import type { FlowEdgeJson, FlowNodeJson, FlowSpecJson } from "@/lib/api"

function newRfId() {
  return `n_${crypto.randomUUID().slice(0, 8)}`
}

/** Default Start → Agent → End graph for the agent builder canvas. */
export function demoGraph(): { nodes: Node[]; edges: Edge[] } {
  const s = newRfId()
  const a = newRfId()
  const e = newRfId()
  return {
    nodes: [
      { id: s, type: "start", position: { x: 40, y: 80 }, data: { title: "Start" } },
      {
        id: a,
        type: "agent",
        position: { x: 320, y: 60 },
        data: {
          title: "Research agent",
          instructions: "You research topics accurately and cite uncertainty.",
          prompt: "Research the topic: {{topic}}. Return bullet facts.",
          model: "",
          toolsStr: "",
        },
      },
      { id: e, type: "end", position: { x: 600, y: 80 }, data: { title: "End" } },
    ],
    edges: [
      { id: `e_${s}_${a}`, source: s, target: a, type: "smoothstep" },
      { id: `e_${a}_${e}`, source: a, target: e, type: "smoothstep" },
    ],
  }
}

/** React Flow `data` payload per node. */
export type FlowNodeData = {
  title?: string
  instructions?: string
  model?: string
  toolsStr?: string
  prompt?: string
  temperature?: number
  maxTokens?: number
  includeChatHistory?: boolean
  outputFormat?: string
  agentSessionKey?: string
  conditionCel?: string
  ifCaseName?: string
  maxIterations?: number
  sourceNodeId?: string
  guardrailInputTemplate?: string
  guardrailContinueOnError?: boolean
  guardrailCustomSubstring?: string
  guardrailChecksStr?: string
  mcpToolId?: string
  mcpArgsJson?: string
  vectorCollection?: string
  vectorQuery?: string
  vectorTopK?: number
  categoriesStr?: string
  classifyModel?: string
  classifyInputTemplate?: string
  classifyExamplesJson?: string
  approvalMessage?: string
  transformFrom?: string
  transformMode?: string
  transformExpressionsJson?: string
  transformObjectJson?: string
  stateKey?: string
  stateValueJson?: string
  stateValueCel?: string
}

const SKIP_EXPORT = new Set(["note"])

function mapRfKindToRust(kind: string): string {
  switch (kind) {
    case "fileSearch":
      return "file_search"
    case "setState":
      return "set_state"
    default:
      return kind
  }
}

function toolsArray(toolsStr?: string): string[] | undefined {
  if (!toolsStr?.trim()) return undefined
  const t = toolsStr
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean)
  return t.length ? t : undefined
}

function guardrailList(s?: string): string[] | undefined {
  if (!s?.trim()) return undefined
  const t = s
    .split(",")
    .map((x) => x.trim().toLowerCase())
    .filter(Boolean)
  return t.length ? t : undefined
}

function compileOne(n: Node): FlowNodeJson | null {
  if (!n.type || SKIP_EXPORT.has(n.type)) return null
  const d = (n.data || {}) as FlowNodeData
  const kind = mapRfKindToRust(n.type)
  const base: FlowNodeJson = {
    id: n.id,
    kind,
    name: d.title ?? "",
    prompt: d.prompt ?? "",
  }
  switch (n.type) {
    case "agent":
      return {
        ...base,
        instructions: d.instructions ?? "",
        model: d.model ?? "",
        tools: toolsArray(d.toolsStr),
        temperature: d.temperature,
        max_tokens: d.maxTokens,
        include_chat_history: d.includeChatHistory === true,
        output_format: d.outputFormat ?? "",
        agent_session_key: d.agentSessionKey ?? "",
      }
    case "llm":
      return {
        ...base,
        model: d.model ?? "",
        temperature: d.temperature,
        max_tokens: d.maxTokens,
        output_format: d.outputFormat ?? "",
      }
    case "if":
      return {
        ...base,
        condition_cel: d.conditionCel ?? "true",
        if_case_name: d.ifCaseName ?? "",
      }
    case "while":
      return {
        ...base,
        condition_cel: d.conditionCel ?? "iteration < 3",
        max_iterations: d.maxIterations && d.maxIterations > 0 ? d.maxIterations : 100,
      }
    case "guardrails":
      return {
        ...base,
        source_node_id: d.sourceNodeId ?? "",
        guardrail_checks: guardrailList(d.guardrailChecksStr),
        guardrail_input_template: d.guardrailInputTemplate ?? "",
        guardrail_continue_on_error: d.guardrailContinueOnError === true,
        guardrail_custom_substring: d.guardrailCustomSubstring ?? "",
      }
    case "mcp":
      return {
        ...base,
        mcp_tool_id: d.mcpToolId ?? "",
        mcp_arguments_json: d.mcpArgsJson ?? "{}",
      }
    case "classify": {
      const cats = (d.categoriesStr ?? "")
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean)
      return {
        ...base,
        kind: "classify",
        classify_categories: cats,
        classify_model: d.classifyModel ?? "",
        classify_input_template: d.classifyInputTemplate ?? "",
        classify_examples_json: d.classifyExamplesJson ?? "",
        output_format: d.outputFormat ?? "",
        temperature: d.temperature,
        max_tokens: d.maxTokens,
        prompt: d.prompt ?? "",
      }
    }
    case "userApproval":
      return {
        ...base,
        kind: "user_approval",
        prompt: d.approvalMessage ?? d.prompt ?? "",
      }
    case "fileSearch":
      return {
        ...base,
        vector_collection: d.vectorCollection ?? "",
        vector_query_template: d.vectorQuery ?? "",
        vector_top_k:
          d.vectorTopK != null && d.vectorTopK > 0 ? Math.min(100, Math.floor(d.vectorTopK)) : 0,
      }
    case "setState":
      return {
        ...base,
        state_key: d.stateKey ?? "",
        state_value_json: d.stateValueJson ?? "null",
        state_value_cel: d.stateValueCel ?? "",
      }
    case "transform":
      return {
        ...base,
        transform_from_node_id: d.transformFrom ?? "",
        state_key: d.stateKey ?? "",
        transform_mode: d.transformMode ?? "",
        transform_expressions_json: d.transformExpressionsJson ?? "",
        transform_object_json: d.transformObjectJson ?? "",
      }
    case "start":
    case "end":
    default:
      return base
  }
}

function edgeLabel(e: Edge): string | undefined {
  const h = e.sourceHandle
  if (h && h.trim()) return h.trim().toLowerCase()
  const lab = (e.data as { label?: string } | undefined)?.label?.trim()
  return lab ? lab.toLowerCase() : undefined
}

export function compileFlowSpec(nodes: Node[], edges: Edge[], flowName: string): FlowSpecJson {
  const flowNodes = nodes.map(compileOne).filter((x): x is FlowNodeJson => x !== null)
  const ids = new Set(flowNodes.map((n) => n.id))
  const flowEdges: FlowEdgeJson[] = []
  for (const e of edges) {
    if (!ids.has(e.source) || !ids.has(e.target)) continue
    const label = edgeLabel(e)
    flowEdges.push({
      from: e.source,
      to: e.target,
      label: label ?? null,
    })
  }
  return { name: flowName, nodes: flowNodes, edges: flowEdges }
}

function rustKindToRf(k: string): string {
  switch (k) {
    case "file_search":
      return "fileSearch"
    case "set_state":
      return "setState"
    case "user_approval":
      return "userApproval"
    case "classify":
      return "classify"
    default:
      return k || "llm"
  }
}

export function flowSpecToReactFlow(spec: FlowSpecJson): { nodes: Node[]; edges: Edge[] } {
  const nodes: Node[] = (spec.nodes || []).map((n, i) => {
    const type = rustKindToRf(n.kind ?? "llm")
    const data: FlowNodeData = {
      title: n.name ?? "",
      prompt: n.prompt ?? "",
      instructions: n.instructions,
      model: n.model,
      toolsStr: n.tools?.join(", "),
      temperature: n.temperature ?? undefined,
      maxTokens: n.max_tokens ?? undefined,
      includeChatHistory: n.include_chat_history === true,
      outputFormat: n.output_format ?? "",
      agentSessionKey: n.agent_session_key ?? "",
      conditionCel: n.condition_cel,
      ifCaseName: n.if_case_name ?? "",
      maxIterations: n.max_iterations,
      sourceNodeId: n.source_node_id,
      guardrailInputTemplate: n.guardrail_input_template ?? "",
      guardrailContinueOnError: n.guardrail_continue_on_error === true,
      guardrailCustomSubstring: n.guardrail_custom_substring ?? "",
      guardrailChecksStr: n.guardrail_checks?.join(", "),
      mcpToolId: n.mcp_tool_id,
      mcpArgsJson: n.mcp_arguments_json,
      vectorCollection: n.vector_collection,
      vectorQuery: n.vector_query_template,
      vectorTopK: n.vector_top_k,
      categoriesStr: n.classify_categories?.join(", ") ?? "",
      classifyModel: n.classify_model ?? "",
      classifyInputTemplate: n.classify_input_template ?? "",
      classifyExamplesJson: n.classify_examples_json ?? "",
      approvalMessage: n.kind === "user_approval" ? n.prompt : undefined,
      transformFrom: n.transform_from_node_id,
      transformMode: n.transform_mode ?? "",
      transformExpressionsJson: n.transform_expressions_json ?? "",
      transformObjectJson: n.transform_object_json ?? "",
      stateKey: n.state_key,
      stateValueJson: n.state_value_json,
      stateValueCel: n.state_value_cel ?? "",
    }
    return {
      id: n.id,
      type,
      position: { x: i * 260, y: 40 + (i % 2) * 40 },
      data,
    }
  })
  const edges: Edge[] = (spec.edges || []).map((e, i) => ({
    id: `import-${i}`,
    source: e.from,
    target: e.to,
    sourceHandle: e.label ?? undefined,
    type: "smoothstep",
    data: e.label ? { label: e.label } : {},
  }))
  return { nodes, edges }
}
