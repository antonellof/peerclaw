import { useCallback, useEffect, useState } from "react"
import { Briefcase, BookOpen, Cpu, Home, LayoutGrid, Terminal } from "lucide-react"

import { fetchOpenAiModels } from "@/lib/api"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Label } from "@/components/ui/label"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { SLASH_COMMANDS } from "@/pages/chat/slashCommands"
import { cn } from "@/lib/utils"
import type { WorkspaceChatPreferences } from "@/workspace/workspacePreferences"
import type { WorkspaceView } from "@/workspace/views"

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  chatPreferences: WorkspaceChatPreferences
  setChatPreferences: (u: Partial<WorkspaceChatPreferences>) => void
  onNavigate: (view: WorkspaceView) => void
}

export function WorkspaceSettingsDialog({
  open,
  onOpenChange,
  chatPreferences,
  setChatPreferences,
  onNavigate,
}: Props) {
  const [models, setModels] = useState<string[]>([])

  const loadModels = useCallback(async () => {
    try {
      const list = await fetchOpenAiModels()
      const ids = list.map((m) => m.id).filter(Boolean)
      setModels(ids.length ? ids : ["llama-3.2-3b", "llama-3.2-1b", "phi-3-mini"])
    } catch {
      setModels(["llama-3.2-3b", "llama-3.2-1b", "phi-3-mini"])
    }
  }, [])

  useEffect(() => {
    if (open) void loadModels()
  }, [open, loadModels])

  const go = (v: WorkspaceView) => {
    onNavigate(v)
    onOpenChange(false)
  }

  const byCat = SLASH_COMMANDS.reduce<Record<string, typeof SLASH_COMMANDS>>((acc, c) => {
    ;(acc[c.category] ??= []).push(c)
    return acc
  }, {})

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className={cn(
          "flex max-h-[min(90dvh,720px)] w-[min(100%,520px)] max-w-[calc(100vw-1.5rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-lg",
        )}
      >
        <DialogHeader className="shrink-0 border-b border-border px-5 py-4 pr-12 text-left">
          <DialogTitle className="text-base">Settings</DialogTitle>
          <DialogDescription className="text-xs">
            Workspace panels, chat defaults, and command reference (also available via{" "}
            <kbd className="rounded border border-border px-1 font-mono">/</kbd> in chat).
          </DialogDescription>
        </DialogHeader>

        <Tabs defaultValue="workspace" className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="shrink-0 border-b border-border px-4 pt-1">
            <TabsList className="h-auto w-full flex-wrap justify-start gap-1 bg-transparent p-0 pb-2">
              <TabsTrigger value="workspace" className="text-xs">
                Workspace
              </TabsTrigger>
              <TabsTrigger value="chat" className="text-xs">
                Chat &amp; models
              </TabsTrigger>
              <TabsTrigger value="reference" className="text-xs">
                <Terminal className="mr-1 inline size-3" />
                Commands
              </TabsTrigger>
            </TabsList>
          </div>

          <TabsContent value="workspace" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-4 px-5 py-4 pr-3">
                <p className="text-xs text-muted-foreground">
                  Open console panels. Same destinations as <code className="text-primary">/open</code> slash routes.
                </p>
                <div className="grid gap-2 sm:grid-cols-2">
                  <Button variant="outline" className="h-auto justify-start gap-2 py-3 text-left" onClick={() => go("home")}>
                    <Home className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">Home</span>
                  </Button>
                  <Button variant="outline" className="h-auto justify-start gap-2 py-3 text-left" onClick={() => go("jobs")}>
                    <Briefcase className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">Jobs</span>
                  </Button>
                  <Button
                    variant="outline"
                    className="h-auto justify-start gap-2 py-3 text-left"
                    onClick={() => go("providers")}
                  >
                    <Cpu className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">Providers</span>
                  </Button>
                  <Button variant="outline" className="h-auto justify-start gap-2 py-3 text-left" onClick={() => go("skills")}>
                    <BookOpen className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">Skills</span>
                  </Button>
                  <Button
                    variant="outline"
                    className="h-auto justify-start gap-2 py-3 text-left"
                    onClick={() => go("overview")}
                  >
                    <LayoutGrid className="size-4 shrink-0 opacity-80" />
                    <span className="text-sm font-medium">Overview</span>
                  </Button>
                </div>
              </div>
            </ScrollArea>
          </TabsContent>

          <TabsContent value="chat" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-4 px-5 py-4 pr-3">
                <p className="text-xs text-muted-foreground">
                  Defaults for the chat composer. Slash commands like <code className="text-primary">/model</code> still
                  override for the session.
                </p>
                <div className="space-y-1.5">
                  <Label className="text-xs">Model</Label>
                  <select
                    className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                    value={chatPreferences.model}
                    onChange={(e) => setChatPreferences({ model: e.target.value })}
                  >
                    {(models.length ? models : [chatPreferences.model]).map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="grid gap-4 sm:grid-cols-2">
                  <div className="space-y-1.5">
                    <Label className="text-xs">Temperature</Label>
                    <input
                      type="number"
                      step={0.05}
                      min={0}
                      max={2}
                      className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                      value={chatPreferences.temperature}
                      onChange={(e) => setChatPreferences({ temperature: parseFloat(e.target.value) || 0.7 })}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label className="text-xs">Max tokens</Label>
                    <input
                      type="number"
                      min={16}
                      max={32000}
                      step={1}
                      className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                      value={chatPreferences.maxTokens}
                      onChange={(e) => setChatPreferences({ maxTokens: parseInt(e.target.value, 10) || 500 })}
                    />
                  </div>
                </div>
                <label className="flex cursor-pointer items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    className="size-4 rounded border-input"
                    checked={chatPreferences.distributed}
                    onChange={(e) => setChatPreferences({ distributed: e.target.checked })}
                  />
                  <span>Prefer distributed inference when available</span>
                </label>
              </div>
            </ScrollArea>
          </TabsContent>

          <TabsContent value="reference" className="m-0 mt-0 flex min-h-0 flex-1 flex-col overflow-hidden focus-visible:outline-none">
            <ScrollArea className="h-[min(52vh,420px)] min-h-[12rem]">
              <div className="space-y-4 px-5 py-4 pr-3">
                <section className="space-y-2">
                  <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">CLI (serve)</h3>
                  <pre className="overflow-x-auto whitespace-pre-wrap break-words rounded-lg border border-border bg-muted/40 p-3 font-mono text-[10px] leading-relaxed text-primary">
                    {`peerclaw serve --web 127.0.0.1:8080 \\
  [--agent path/to/agent.toml] [--ollama] [--gpu] \\
  [--share-inference] [--provider-max-requests N]`}
                  </pre>
                  <p className="text-[11px] text-muted-foreground">
                    Run <code className="text-foreground">peerclaw --help</code> and{" "}
                    <code className="text-foreground">peerclaw serve --help</code> for full flags.
                  </p>
                </section>
                {Object.entries(byCat).map(([cat, cmds]) => (
                  <div key={cat}>
                    <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{cat}</div>
                    <ul className="space-y-1.5 text-xs">
                      {cmds.map((c) => (
                        <li
                          key={c.cmd}
                          className="break-words rounded-md border border-border/50 bg-muted/15 px-2 py-1.5"
                        >
                          <code className="font-mono text-primary">{c.cmd}</code>
                          {c.args ? (
                            <span className="text-muted-foreground"> {c.args}</span>
                          ) : null}
                          <div className="mt-0.5 text-[11px] text-muted-foreground">{c.desc}</div>
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </div>
            </ScrollArea>
          </TabsContent>
        </Tabs>

        <div className="shrink-0 border-t border-border px-5 py-3">
          <Button variant="secondary" className="w-full" size="sm" onClick={() => onOpenChange(false)}>
            Done
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
