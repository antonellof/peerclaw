import { useMemo, useState } from "react"
import type { Node } from "@xyflow/react"
import { Plug, RefreshCw } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { Separator } from "@/components/ui/separator"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import type { McpToolListItem } from "@/lib/api"
import { cn } from "@/lib/utils"

import type { FlowNodeData } from "./flowCompile"
import { GUARD_OPTIONS, guardrailSetFromStr, stringifyGuardSet } from "./flowGuardrailOptions"

const NODE_INSPECTOR_HELP: Partial<Record<string, string>> = {
  start: "Single entry point for the graph interpreter. Wire one outgoing edge to your first real step.",
  agent: "Runs the full agent loop with tools. Model blank = first model available on the node.",
  llm: "One-shot completion. Earlier node outputs are appended as context.",
  classify: "Dedicated classifier step: categories, optional few-shot JSON, and input template ({{vars}}).",
  end: "Stops execution and returns recorded steps.",
  fileSearch: "Queries the shared vectX store. Collection id = vector collection name.",
  guardrails: "SafetyLayer checks. Use Input template ({{topic}}) or Source node id. Pass / fail edges.",
  mcp: "Runs a tool from the node MCP catalog (same servers as Settings → MCP and the MCP console). Use server:tool ids.",
  if: "CEL expression over inputs, outputs, state, input_as_text. Use true / false edge labels.",
  while: "CEL with iteration variable. Use loop / exit edge labels.",
  userApproval:
    "Branches on inputs.human_approval (approve | reject) until real HITL ships. Wire approve and reject edges.",
  transform: "Copy mode, CEL expressions array, or JSON object merged into state.",
  setState: "Set state from JSON (with {{var}} interpolation) or from a CEL expression.",
  note: "Canvas-only annotation; not exported to FlowSpec.",
}

function selectCls(disabled: boolean) {
  return cn(
    "mt-1 flex h-8 w-full rounded-md border border-input bg-background px-2 text-xs shadow-sm",
    disabled && "cursor-not-allowed opacity-60",
  )
}

const MCP_CUSTOM_SELECT = "__peerclaw_mcp_custom__"

function groupMcpToolsByServer(tools: McpToolListItem[]): [string, McpToolListItem[]][] {
  const m = new Map<string, McpToolListItem[]>()
  for (const t of tools) {
    const i = t.id.indexOf(":")
    const server = i >= 0 ? t.id.slice(0, i) : "other"
    const arr = m.get(server)
    if (arr) arr.push(t)
    else m.set(server, [t])
  }
  return Array.from(m.entries()).sort(([a], [b]) => a.localeCompare(b))
}

function McpToolSection({
  disabled,
  mcpToolId,
  mcpArgsJson,
  mcpTools,
  mcpEnabled,
  mcpHint,
  onChangeData,
  onOpenMcpConsole,
  onRefreshMcpCatalog,
}: {
  disabled: boolean
  mcpToolId: string
  mcpArgsJson: string
  mcpTools: McpToolListItem[]
  mcpEnabled: boolean
  mcpHint: string | null
  onChangeData: (patch: Partial<FlowNodeData>) => void
  onOpenMcpConsole?: () => void
  onRefreshMcpCatalog?: () => void
}) {
  const [filter, setFilter] = useState("")
  const q = filter.trim().toLowerCase()
  const filtered = useMemo(() => {
    if (!q) return mcpTools
    return mcpTools.filter(
      (t) =>
        t.id.toLowerCase().includes(q) || (t.description ?? "").toLowerCase().includes(q),
    )
  }, [mcpTools, q])

  const grouped = useMemo(() => groupMcpToolsByServer(filtered), [filtered])
  const ids = useMemo(() => new Set(mcpTools.map((t) => t.id)), [mcpTools])
  const current = mcpToolId.trim()
  const inCatalog = current.length > 0 && ids.has(current)
  const selectValue = !current ? "" : inCatalog ? current : MCP_CUSTOM_SELECT

  return (
    <>
      <div className="space-y-2 rounded-md border border-border/50 bg-muted/15 p-2">
        <div className="flex flex-wrap items-center gap-1.5">
          <Plug className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
          <span className="text-[10px] font-medium uppercase tracking-wide text-muted-foreground">Node MCP</span>
          {onRefreshMcpCatalog ? (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="size-7 shrink-0"
              disabled={disabled}
              title="Reload tool list from the node"
              onClick={() => onRefreshMcpCatalog()}
            >
              <RefreshCw className="size-3.5" />
            </Button>
          ) : null}
        </div>
        {mcpHint ? <p className="text-[10px] leading-relaxed text-muted-foreground">{mcpHint}</p> : null}
        {!mcpEnabled ? (
          <p className="text-[10px] text-amber-600/90 dark:text-amber-400/90">
            MCP is disabled in config. Enable it in Settings → MCP or the MCP console.
          </p>
        ) : null}
        {onOpenMcpConsole ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 w-full text-[11px]"
            disabled={disabled}
            onClick={() => onOpenMcpConsole()}
          >
            Open MCP console
          </Button>
        ) : null}
      </div>

      {mcpTools.length > 8 ? (
        <div>
          <Label className="text-[10px] text-muted-foreground">Filter tools</Label>
          <Input
            className="mt-1 h-7 text-[11px]"
            disabled={disabled}
            placeholder="server, tool name, description…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        </div>
      ) : null}

      <div>
        <Label>Tool (from catalog)</Label>
        <select
          className={selectCls(disabled)}
          disabled={disabled || mcpTools.length === 0}
          value={selectValue}
          onChange={(e) => {
            const v = e.target.value
            if (v === "" || v === MCP_CUSTOM_SELECT) {
              if (v === "") onChangeData({ mcpToolId: "" })
              return
            }
            onChangeData({ mcpToolId: v })
          }}
        >
          <option value="">{mcpTools.length === 0 ? "No tools — configure MCP" : "Select tool…"}</option>
          {grouped.map(([server, list]) => (
            <optgroup key={server} label={server}>
              {list.map((t) => {
                const short = t.id.startsWith(`${server}:`) ? t.id.slice(server.length + 1) : t.id
                return (
                  <option key={t.id} value={t.id} title={t.description ?? undefined}>
                    {short}
                    {t.description ? ` — ${t.description.slice(0, 72)}${t.description.length > 72 ? "…" : ""}` : ""}
                  </option>
                )
              })}
            </optgroup>
          ))}
          <option value={MCP_CUSTOM_SELECT}>Custom id (edit below)…</option>
        </select>
      </div>
      <div>
        <Label>Tool id <span className="font-normal text-muted-foreground">(server:tool)</span></Label>
        <Input
          className="mt-1 h-8 font-mono text-[11px]"
          disabled={disabled}
          placeholder="e.g. filesystem:read_file"
          value={mcpToolId}
          onChange={(e) => onChangeData({ mcpToolId: e.target.value })}
        />
      </div>
      <div>
        <Label>Arguments JSON</Label>
        <Textarea
          className="mt-1 font-mono text-[11px]"
          rows={4}
          disabled={disabled}
          placeholder='{"query":"{{topic}}"}'
          value={mcpArgsJson}
          onChange={(e) => onChangeData({ mcpArgsJson: e.target.value })}
        />
      </div>
    </>
  )
}

function ModelSelect({
  value,
  disabled,
  modelIds,
  onChange,
  id,
}: {
  value: string
  disabled: boolean
  modelIds: string[]
  onChange: (v: string) => void
  id: string
}) {
  return (
    <select
      id={id}
      className={selectCls(disabled)}
      disabled={disabled}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    >
      <option value="">Default (first model on node)</option>
      {modelIds.map((mid) => (
        <option key={mid} value={mid}>
          {mid}
        </option>
      ))}
    </select>
  )
}

export function FlowNodeInspector({
  node,
  disabled,
  onChangeData,
  modelIds,
  mcpTools = [],
  mcpEnabled = false,
  mcpHint = null,
  onOpenMcpConsole,
  onRefreshMcpCatalog,
}: {
  node: Node
  disabled: boolean
  onChangeData: (patch: Partial<FlowNodeData>) => void
  modelIds: string[]
  /** Live catalog from GET /api/mcp/status (same as chat and Settings → MCP). */
  mcpTools?: McpToolListItem[]
  mcpEnabled?: boolean
  mcpHint?: string | null
  onOpenMcpConsole?: () => void
  onRefreshMcpCatalog?: () => void
}) {
  const d = (node.data || {}) as FlowNodeData
  const nt = String(node.type ?? "")
  const blurb = NODE_INSPECTOR_HELP[nt]

  const outFmt = useMemo(
    () => (d.outputFormat === "json" ? "json" : "text"),
    [d.outputFormat],
  )

  return (
    <div className="space-y-3 text-xs">
      {blurb ? <p className="text-[11px] leading-relaxed text-muted-foreground">{blurb}</p> : null}
      <div>
        <Label>Name</Label>
        <Input
          className="mt-1 h-8 text-xs"
          disabled={disabled}
          value={d.title ?? ""}
          onChange={(e) => onChangeData({ title: e.target.value })}
        />
      </div>

      <Separator className="my-1 bg-border/60" />

      {node.type === "agent" && (
        <>
          <div>
            <Label>Instructions</Label>
            <Textarea
              className="mt-1 text-xs"
              rows={4}
              disabled={disabled}
              placeholder="Describe tone, tool usage, response style…"
              value={d.instructions ?? ""}
              onChange={(e) => onChangeData({ instructions: e.target.value })}
            />
          </div>
          <label className="flex cursor-pointer items-center gap-2 text-[11px]">
            <input
              type="checkbox"
              className="size-3.5 rounded border-border accent-primary"
              disabled={disabled}
              checked={d.includeChatHistory === true}
              onChange={(e) => onChangeData({ includeChatHistory: e.target.checked })}
            />
            Include chat history (session)
          </label>
          {d.includeChatHistory ? (
            <div>
              <Label>Session key</Label>
              <Input
                className="mt-1 h-8 font-mono text-[11px]"
                disabled={disabled}
                placeholder="empty = per–node-id default"
                value={d.agentSessionKey ?? ""}
                onChange={(e) => onChangeData({ agentSessionKey: e.target.value })}
              />
            </div>
          ) : null}
          <div>
            <Label>Model</Label>
            <ModelSelect
              id="agent-model"
              value={d.model ?? ""}
              disabled={disabled}
              modelIds={modelIds}
              onChange={(v) => onChangeData({ model: v })}
            />
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <Label>Temperature</Label>
              <Input
                type="number"
                step={0.1}
                min={0}
                max={2}
                className="mt-1 h-8"
                disabled={disabled}
                value={d.temperature ?? 0.5}
                onChange={(e) =>
                  onChangeData({ temperature: parseFloat(e.target.value) || 0.5 })
                }
              />
            </div>
            <div>
              <Label>Max tokens</Label>
              <Input
                type="number"
                min={1}
                className="mt-1 h-8"
                disabled={disabled}
                value={d.maxTokens ?? 2048}
                onChange={(e) =>
                  onChangeData({ maxTokens: parseInt(e.target.value, 10) || 2048 })
                }
              />
            </div>
          </div>
          <div>
            <Label>Tools (comma-separated builtins)</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              placeholder="empty = all builtins"
              value={d.toolsStr ?? ""}
              onChange={(e) => onChangeData({ toolsStr: e.target.value })}
            />
          </div>
          <div>
            <Label>Output format</Label>
            <select
              className={selectCls(disabled)}
              disabled={disabled}
              value={outFmt}
              onChange={(e) => onChangeData({ outputFormat: e.target.value })}
            >
              <option value="text">Text</option>
              <option value="json">JSON</option>
            </select>
          </div>
          <div>
            <Label>Task / user message</Label>
            <Textarea
              className="mt-1 text-xs"
              rows={3}
              disabled={disabled}
              placeholder="{{topic}} — uses flow inputs"
              value={d.prompt ?? ""}
              onChange={(e) => onChangeData({ prompt: e.target.value })}
            />
          </div>
        </>
      )}

      {node.type === "llm" && (
        <>
          <div>
            <Label>Model</Label>
            <ModelSelect
              id="llm-model"
              value={d.model ?? ""}
              disabled={disabled}
              modelIds={modelIds}
              onChange={(v) => onChangeData({ model: v })}
            />
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <Label>Temperature</Label>
              <Input
                type="number"
                step={0.1}
                min={0}
                max={2}
                className="mt-1 h-8"
                disabled={disabled}
                value={d.temperature ?? 0.7}
                onChange={(e) =>
                  onChangeData({ temperature: parseFloat(e.target.value) || 0.7 })
                }
              />
            </div>
            <div>
              <Label>Max tokens</Label>
              <Input
                type="number"
                min={1}
                className="mt-1 h-8"
                disabled={disabled}
                value={d.maxTokens ?? 512}
                onChange={(e) =>
                  onChangeData({ maxTokens: parseInt(e.target.value, 10) || 512 })
                }
              />
            </div>
          </div>
          <div>
            <Label>Output format</Label>
            <select
              className={selectCls(disabled)}
              disabled={disabled}
              value={outFmt}
              onChange={(e) => onChangeData({ outputFormat: e.target.value })}
            >
              <option value="text">Text</option>
              <option value="json">JSON</option>
            </select>
          </div>
          <div>
            <Label>Prompt</Label>
            <Textarea
              className="mt-1 text-xs"
              rows={4}
              disabled={disabled}
              value={d.prompt ?? ""}
              onChange={(e) => onChangeData({ prompt: e.target.value })}
            />
          </div>
        </>
      )}

      {node.type === "classify" && (
        <>
          <div>
            <Label>Categories (comma-separated)</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              placeholder="pricing, support, bug, other"
              value={d.categoriesStr ?? ""}
              onChange={(e) => onChangeData({ categoriesStr: e.target.value })}
            />
          </div>
          <div>
            <Label>Input template</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              placeholder="{{input_as_text}} or {{topic}}"
              value={d.classifyInputTemplate ?? ""}
              onChange={(e) => onChangeData({ classifyInputTemplate: e.target.value })}
            />
            <p className="mt-0.5 text-[10px] text-muted-foreground">
              Empty = use flow <code className="text-foreground/80">input_as_text</code>.
            </p>
          </div>
          <div>
            <Label>Classifier model</Label>
            <ModelSelect
              id="classify-model"
              value={d.classifyModel ?? ""}
              disabled={disabled}
              modelIds={modelIds}
              onChange={(v) => onChangeData({ classifyModel: v })}
            />
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <Label>Temperature</Label>
              <Input
                type="number"
                step={0.05}
                min={0}
                max={2}
                className="mt-1 h-8"
                disabled={disabled}
                value={d.temperature ?? 0.2}
                onChange={(e) =>
                  onChangeData({ temperature: parseFloat(e.target.value) || 0.2 })
                }
              />
            </div>
            <div>
              <Label>Max tokens</Label>
              <Input
                type="number"
                min={8}
                className="mt-1 h-8"
                disabled={disabled}
                value={d.maxTokens ?? 128}
                onChange={(e) =>
                  onChangeData({ maxTokens: parseInt(e.target.value, 10) || 128 })
                }
              />
            </div>
          </div>
          <div>
            <Label>Output format</Label>
            <select
              className={selectCls(disabled)}
              disabled={disabled}
              value={outFmt}
              onChange={(e) => onChangeData({ outputFormat: e.target.value })}
            >
              <option value="text">Plain label</option>
              <option value="json">JSON (category field)</option>
            </select>
          </div>
          <div>
            <Label>Few-shot examples (JSON array)</Label>
            <Textarea
              className="mt-1 font-mono text-[11px]"
              rows={4}
              disabled={disabled}
              placeholder='[{"input":"...","category":"support"}]'
              value={d.classifyExamplesJson ?? ""}
              onChange={(e) => onChangeData({ classifyExamplesJson: e.target.value })}
            />
          </div>
          <div>
            <Label>Extra instructions</Label>
            <Textarea
              className="mt-1 text-xs"
              rows={2}
              disabled={disabled}
              value={d.prompt ?? ""}
              onChange={(e) => onChangeData({ prompt: e.target.value })}
            />
          </div>
        </>
      )}

      {node.type === "userApproval" && (
        <div>
          <Label>Message for reviewer</Label>
          <Textarea
            className="mt-1 text-xs"
            rows={3}
            disabled={disabled}
            placeholder="Shown in run output; branch from inputs.human_approval"
            value={d.approvalMessage ?? ""}
            onChange={(e) => onChangeData({ approvalMessage: e.target.value })}
          />
        </div>
      )}

      {(node.type === "if" || node.type === "while") && (
        <>
          {node.type === "if" ? (
            <div>
              <Label>Case name (optional)</Label>
              <Input
                className="mt-1 h-8 text-xs"
                disabled={disabled}
                placeholder="Documentation only"
                value={d.ifCaseName ?? ""}
                onChange={(e) => onChangeData({ ifCaseName: e.target.value })}
              />
            </div>
          ) : null}
          <div>
            <Label>Condition (CEL)</Label>
            <Textarea
              className="mt-1 font-mono text-[11px]"
              rows={2}
              disabled={disabled}
              placeholder={node.type === "while" ? "e.g. iteration < 3" : "e.g. inputs.topic != ''"}
              value={d.conditionCel ?? ""}
              onChange={(e) => onChangeData({ conditionCel: e.target.value })}
            />
            <p className="mt-1 text-[10px] text-muted-foreground">
              Variables: <code className="text-foreground/80">inputs</code>,{" "}
              <code className="text-foreground/80">outputs</code>, <code className="text-foreground/80">state</code>,{" "}
              <code className="text-foreground/80">input_as_text</code>
              {node.type === "while" ? (
                <>
                  , <code className="text-foreground/80">iteration</code>
                </>
              ) : null}
              .
            </p>
          </div>
        </>
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
            <Label>Input template</Label>
            <Textarea
              className="mt-1 font-mono text-[11px]"
              rows={2}
              disabled={disabled}
              placeholder="{{input_as_text}} — overrides source node when non-empty"
              value={d.guardrailInputTemplate ?? ""}
              onChange={(e) => onChangeData({ guardrailInputTemplate: e.target.value })}
            />
          </div>
          <div>
            <Label>Source node id (when template empty)</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              placeholder="Node id whose output is stringified"
              value={d.sourceNodeId ?? ""}
              onChange={(e) => onChangeData({ sourceNodeId: e.target.value })}
            />
          </div>
          <div className="space-y-2">
            <Label>Safety checks</Label>
            <div className="max-h-48 space-y-2 overflow-y-auto rounded-md border border-border/60 bg-muted/20 p-2">
              {GUARD_OPTIONS.map((opt) => {
                const set = guardrailSetFromStr(d.guardrailChecksStr)
                const on = set.has(opt.id)
                return (
                  <label key={opt.id} className="flex cursor-pointer items-center gap-2 text-[11px]">
                    <input
                      type="checkbox"
                      className="size-3.5 shrink-0 rounded border-border accent-primary"
                      disabled={disabled}
                      checked={on}
                      onChange={(e) => {
                        const next = new Set(set)
                        if (e.target.checked) next.add(opt.id)
                        else next.delete(opt.id)
                        onChangeData({ guardrailChecksStr: stringifyGuardSet(next) })
                      }}
                    />
                    <span>{opt.label}</span>
                  </label>
                )
              })}
            </div>
            <p className="text-[10px] text-muted-foreground">
              Leave all unchecked for default: leak, injection, policy.
            </p>
          </div>
          <div>
            <Label>Custom block substring</Label>
            <Input
              className="mt-1 h-8 text-xs"
              disabled={disabled}
              placeholder="If “custom” is checked, fail when text contains this"
              value={d.guardrailCustomSubstring ?? ""}
              onChange={(e) => onChangeData({ guardrailCustomSubstring: e.target.value })}
            />
          </div>
          <label className="flex cursor-pointer items-center gap-2 text-[11px]">
            <input
              type="checkbox"
              className="size-3.5 rounded border-border accent-primary"
              disabled={disabled}
              checked={d.guardrailContinueOnError === true}
              onChange={(e) => onChangeData({ guardrailContinueOnError: e.target.checked })}
            />
            Continue on error (follow pass edge anyway)
          </label>
        </>
      )}

      {node.type === "mcp" && (
        <McpToolSection
          key={node.id}
          disabled={disabled}
          mcpToolId={d.mcpToolId ?? ""}
          mcpArgsJson={d.mcpArgsJson ?? "{}"}
          mcpTools={mcpTools}
          mcpEnabled={mcpEnabled}
          mcpHint={mcpHint}
          onChangeData={onChangeData}
          onOpenMcpConsole={onOpenMcpConsole}
          onRefreshMcpCatalog={onRefreshMcpCatalog}
        />
      )}

      {node.type === "fileSearch" && (
        <>
          <div>
            <Label>Vector collection (vectX id)</Label>
            <Input
              className="mt-1 h-8 font-mono text-[11px]"
              disabled={disabled}
              placeholder="my_docs"
              value={d.vectorCollection ?? ""}
              onChange={(e) => onChangeData({ vectorCollection: e.target.value })}
            />
          </div>
          <div>
            <Label>Max results</Label>
            <Input
              type="number"
              min={1}
              max={100}
              className="mt-1 h-8"
              disabled={disabled}
              value={d.vectorTopK && d.vectorTopK > 0 ? d.vectorTopK : 10}
              onChange={(e) =>
                onChangeData({
                  vectorTopK: Math.min(100, Math.max(1, parseInt(e.target.value, 10) || 10)),
                })
              }
            />
          </div>
          <div>
            <Label>Query</Label>
            <Textarea
              className="mt-1 font-mono text-[11px]"
              rows={3}
              disabled={disabled}
              placeholder="{{input_as_text}} — uses {{var}} from inputs and prior outputs"
              value={d.vectorQuery ?? ""}
              onChange={(e) => onChangeData({ vectorQuery: e.target.value })}
            />
          </div>
        </>
      )}

      {(node.type === "setState" || node.type === "transform") && (
        <>
          {node.type === "transform" ? (
            <Tabs defaultValue="mode" className="w-full">
              <TabsList className="h-8 w-full">
                <TabsTrigger value="mode" className="flex-1 text-[10px]">
                  Mode
                </TabsTrigger>
                <TabsTrigger value="expr" className="flex-1 text-[10px]">
                  Expressions
                </TabsTrigger>
                <TabsTrigger value="obj" className="flex-1 text-[10px]">
                  Object
                </TabsTrigger>
              </TabsList>
              <TabsContent value="mode" className="mt-2 space-y-2">
                <Label>Transform mode</Label>
                <select
                  className={selectCls(disabled)}
                  disabled={disabled}
                  value={d.transformMode ?? "copy"}
                  onChange={(e) => onChangeData({ transformMode: e.target.value })}
                >
                  <option value="copy">Copy from node → state key</option>
                  <option value="expressions">CEL expressions → state keys</option>
                  <option value="object">Merge JSON object into state</option>
                </select>
                <div>
                  <Label>From node id (copy mode)</Label>
                  <Input
                    className="mt-1 h-8 font-mono text-[11px]"
                    disabled={disabled}
                    value={d.transformFrom ?? ""}
                    onChange={(e) => onChangeData({ transformFrom: e.target.value })}
                  />
                </div>
                <div>
                  <Label>State key (copy mode)</Label>
                  <Input
                    className="mt-1 h-8 font-mono text-[11px]"
                    disabled={disabled}
                    value={d.stateKey ?? ""}
                    onChange={(e) => onChangeData({ stateKey: e.target.value })}
                  />
                </div>
              </TabsContent>
              <TabsContent value="expr" className="mt-2 space-y-2">
                <Label>Expressions JSON</Label>
                <Textarea
                  className="font-mono text-[11px]"
                  rows={6}
                  disabled={disabled}
                  placeholder={`[\n  { "key": "result", "cel": "inputs.foo + 1" }\n]`}
                  value={d.transformExpressionsJson ?? "[]"}
                  onChange={(e) => onChangeData({ transformExpressionsJson: e.target.value })}
                />
                <p className="text-[10px] text-muted-foreground">
                  CEL over <code className="text-foreground/80">inputs</code>,{" "}
                  <code className="text-foreground/80">outputs</code>,{" "}
                  <code className="text-foreground/80">state</code>,{" "}
                  <code className="text-foreground/80">input_as_text</code>.
                </p>
              </TabsContent>
              <TabsContent value="obj" className="mt-2 space-y-2">
                <Label>Object JSON</Label>
                <Textarea
                  className="font-mono text-[11px]"
                  rows={5}
                  disabled={disabled}
                  placeholder='{"foo": 1, "bar": "{{topic}}"}'
                  value={d.transformObjectJson ?? "{}"}
                  onChange={(e) => onChangeData({ transformObjectJson: e.target.value })}
                />
              </TabsContent>
            </Tabs>
          ) : null}
          {node.type === "setState" ? (
            <>
              <div>
                <Label>State key</Label>
                <Input
                  className="mt-1 h-8 font-mono text-[11px]"
                  disabled={disabled}
                  value={d.stateKey ?? ""}
                  onChange={(e) => onChangeData({ stateKey: e.target.value })}
                />
              </div>
              <div>
                <Label>Value (CEL)</Label>
                <Textarea
                  className="mt-1 font-mono text-[11px]"
                  rows={2}
                  disabled={disabled}
                  placeholder="e.g. inputs.count + 1 — when set, overrides JSON value below"
                  value={d.stateValueCel ?? ""}
                  onChange={(e) => onChangeData({ stateValueCel: e.target.value })}
                />
              </div>
              <div>
                <Label>Value JSON (if CEL empty)</Label>
                <Textarea
                  className="mt-1 font-mono text-[11px]"
                  rows={2}
                  disabled={disabled}
                  placeholder='"hello" or {"x":1} — {{var}} interpolation'
                  value={d.stateValueJson ?? "null"}
                  onChange={(e) => onChangeData({ stateValueJson: e.target.value })}
                />
              </div>
            </>
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
