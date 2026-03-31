import { useCallback, useEffect, useState } from "react"
import { DEFAULT_MODEL } from "@/lib/defaults"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { Loader2, Sparkles, Wand2 } from "lucide-react"

import {
  fetchOpenAiModels,
  fetchSkillStudio,
  fetchSkillStudioList,
  saveSkillStudio,
  scanSkills,
  skillStudioAi,
  type SkillStudioListEntry,
} from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

const SLUG_RE = /^[a-zA-Z0-9][a-zA-Z0-9._-]{0,63}$/

export const DEFAULT_SKILL_TEMPLATE = `---
name: my-skill
version: "0.1.0"
description: Short description used for skill routing and discovery.
---

# Instructions

When this skill is active, follow these steps for the user's task.

- Be specific about tools and outputs.
- Keep context within the agent budget.
`

/** If the model wrapped output in \`\`\` fences, unwrap once. */
function unwrapSkillMarkdown(raw: string): string {
  const t = raw.trim()
  const m = /^```(?:markdown|md|yaml)?\s*\n([\s\S]*?)\n```\s*$/i.exec(t)
  if (m?.[1]) return m[1].trim()
  return t
}

export type SkillStudioPanelProps = {
  /** Called after a successful save and registry rescan (parent can refresh skill lists). */
  onSaved?: () => void
}

export function SkillStudioPanel({ onSaved }: SkillStudioPanelProps) {
  const [list, setList] = useState<SkillStudioListEntry[]>([])
  const [mode, setMode] = useState<"pick" | "new">("pick")
  const [slug, setSlug] = useState("")
  const [content, setContent] = useState(DEFAULT_SKILL_TEMPLATE)
  const [layout, setLayout] = useState<string>("nested")
  const [loadError, setLoadError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [loading, setLoading] = useState(false)
  const [aiBusy, setAiBusy] = useState(false)
  const [aiNote, setAiNote] = useState<string | null>(null)
  const [instruction, setInstruction] = useState("Review for clarity, valid YAML frontmatter, and PeerClaw SKILL.md conventions.")
  const [models, setModels] = useState<string[]>([])
  const [aiModel, setAiModel] = useState(DEFAULT_MODEL)
  const [saveMsg, setSaveMsg] = useState<string | null>(null)

  const refreshList = useCallback(async () => {
    try {
      setList(await fetchSkillStudioList())
    } catch {
      setList([])
    }
  }, [])

  useEffect(() => {
    void refreshList()
  }, [refreshList])

  useEffect(() => {
    void (async () => {
      try {
        const m = await fetchOpenAiModels()
        const ids = m.map((x) => x.id).filter(Boolean)
        if (ids.length) {
          setModels(ids)
          setAiModel((cur) => (ids.includes(cur) ? cur : ids[0]!))
        }
      } catch {
        setModels([DEFAULT_MODEL])
      }
    })()
  }, [])

  const loadSlug = async (s: string) => {
    setLoadError(null)
    setLoading(true)
    setSaveMsg(null)
    try {
      const r = await fetchSkillStudio(s)
      setSlug(r.slug)
      setContent(r.content)
      setLayout(r.layout)
      setMode("pick")
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : "Load failed")
    } finally {
      setLoading(false)
    }
  }

  const startNew = () => {
    setMode("new")
    setSlug("")
    setContent(DEFAULT_SKILL_TEMPLATE)
    setLayout("nested")
    setLoadError(null)
    setSaveMsg(null)
  }

  const onSave = async () => {
    const key = slug.trim()
    if (!SLUG_RE.test(key)) {
      setSaveMsg("Invalid slug: use letters, numbers, dot, underscore, hyphen (1–64 chars).")
      return
    }
    setSaving(true)
    setSaveMsg(null)
    try {
      await saveSkillStudio(key, content)
      const scan = await scanSkills()
      setSaveMsg(
        scan.ok
          ? `Saved. Registry rescanned (${scan.loaded ?? 0} skill(s)).`
          : "Saved. Rescan failed — run Rescan from the Library tab.",
      )
      setSlug(key)
      setMode("pick")
      await refreshList()
      onSaved?.()
    } catch (e) {
      setSaveMsg(e instanceof Error ? e.message : "Save failed")
    } finally {
      setSaving(false)
    }
  }

  const runAi = async (preset?: string) => {
    const inst = preset ?? instruction
    if (!inst.trim()) {
      setAiNote("Add an instruction first.")
      return
    }
    setAiBusy(true)
    setAiNote(null)
    try {
      const r = await skillStudioAi({
        content,
        instruction: inst,
        model: aiModel,
        max_tokens: 3072,
        temperature: 0.25,
      })
      setContent(unwrapSkillMarkdown(r.text))
      setAiNote(`Applied model output (${r.tokens} tokens). Review then Save.`)
    } catch (e) {
      setAiNote(e instanceof Error ? e.message : "AI request failed")
    } finally {
      setAiBusy(false)
    }
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Skill studio</CardTitle>
          <CardDescription>
            Create or edit <code className="text-foreground">SKILL.md</code> under your skills directory (nested{" "}
            <code className="text-foreground">{"{slug}/SKILL.md"}</code>). Use AI to draft or refine; always review before
            saving.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-end">
            <div className="min-w-[12rem] flex-1 space-y-1.5">
              <Label className="text-xs">Existing skill</Label>
              <select
                className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                value={mode === "pick" && list.some((x) => x.slug === slug) ? slug : ""}
                onChange={(e) => {
                  const v = e.target.value
                  if (!v) return
                  void loadSlug(v)
                }}
                disabled={loading}
              >
                <option value="">Select to load…</option>
                {list.map((x) => (
                  <option key={x.slug} value={x.slug}>
                    {x.slug} ({x.layout})
                  </option>
                ))}
              </select>
            </div>
            <Button type="button" variant="secondary" size="sm" onClick={startNew}>
              New skill
            </Button>
            {mode === "new" && (
              <div className="min-w-[10rem] flex-1 space-y-1.5">
                <Label className="text-xs">New slug (folder name)</Label>
                <Input
                  placeholder="e.g. my-api-helper"
                  value={slug}
                  onChange={(e) => setSlug(e.target.value)}
                  className="font-mono text-sm"
                />
              </div>
            )}
            {mode === "pick" && slug && (
              <p className="text-xs text-muted-foreground">
                On disk: <span className="font-mono text-foreground">{layout}</span>
              </p>
            )}
          </div>
          {loadError && <p className="text-sm text-destructive">{loadError}</p>}
          {saveMsg && (
            <p className={cn("text-sm", saveMsg.startsWith("Saved") ? "text-emerald-400" : "text-amber-400")}>
              {saveMsg}
            </p>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-base">AI assist</CardTitle>
          <CardDescription>Uses the same node inference as chat. Low temperature recommended.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label className="text-xs">Model</Label>
              <select
                className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                value={aiModel}
                onChange={(e) => setAiModel(e.target.value)}
              >
                {(models.length ? models : [DEFAULT_MODEL]).map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-wrap items-end gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={aiBusy}
                onClick={() =>
                  void runAi("Review this SKILL.md: fix frontmatter (name, version, description), tighten instructions, fix markdown structure. Output the full file only.")
                }
              >
                <Wand2 className="mr-1.5 size-3.5" />
                Review
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={aiBusy}
                onClick={() =>
                  void runAi(
                    "Expand the body with concrete steps, safety notes, and one short example. Keep frontmatter consistent. Output the full file only.",
                  )
                }
              >
                Expand
              </Button>
            </div>
          </div>
          <div className="space-y-1.5">
            <Label className="text-xs">Custom instruction</Label>
            <Textarea
              rows={2}
              value={instruction}
              onChange={(e) => setInstruction(e.target.value)}
              className="text-sm"
              placeholder="What should the model change?"
            />
          </div>
          <Button type="button" size="sm" disabled={aiBusy} onClick={() => void runAi()}>
            {aiBusy ? <Loader2 className="mr-2 size-4 animate-spin" /> : <Sparkles className="mr-2 size-4" />}
            Apply AI to editor
          </Button>
          {aiNote && <p className="text-xs text-muted-foreground">{aiNote}</p>}
        </CardContent>
      </Card>

      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-medium">Editor</h3>
        <Button type="button" size="sm" disabled={saving || !content} onClick={() => void onSave()}>
          {saving ? <Loader2 className="mr-2 size-4 animate-spin" /> : null}
          Save to disk
        </Button>
      </div>

      <Tabs defaultValue="edit" className="w-full">
        <TabsList>
          <TabsTrigger value="edit">Markdown</TabsTrigger>
          <TabsTrigger value="preview">Preview</TabsTrigger>
        </TabsList>
        <TabsContent value="edit" className="mt-3 focus-visible:outline-none">
          <Textarea
            value={content}
            onChange={(e) => setContent(e.target.value)}
            className="min-h-[min(60vh,520px)] font-mono text-xs leading-relaxed"
            spellCheck={false}
          />
        </TabsContent>
        <TabsContent value="preview" className="mt-3 focus-visible:outline-none">
          <div className="skill-md-preview max-w-none min-h-[min(50vh,400px)] rounded-xl border border-border bg-card/40 p-4 text-sm leading-relaxed text-foreground [&_a]:text-primary [&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_h1]:mb-3 [&_h1]:text-lg [&_h1]:font-semibold [&_h2]:mb-2 [&_h2]:mt-4 [&_h2]:text-base [&_h2]:font-medium [&_li]:my-0.5 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:bg-muted [&_pre]:p-3 [&_pre]:text-xs [&_ul]:list-inside [&_ul]:list-disc">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
          </div>
        </TabsContent>
      </Tabs>
    </div>
  )
}
