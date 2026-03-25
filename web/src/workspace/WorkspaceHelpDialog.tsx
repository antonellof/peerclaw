import { BookOpen } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { ScrollArea } from "@/components/ui/scroll-area"
import { cn } from "@/lib/utils"

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  onOpenSettings?: () => void
}

export function WorkspaceHelpDialog({ open, onOpenChange, onOpenSettings }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className={cn(
          "flex max-h-[min(88dvh,640px)] w-[min(100%,420px)] max-w-[calc(100vw-1.5rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-md",
        )}
      >
        <DialogHeader className="shrink-0 border-b border-border px-5 py-4 pr-12 text-left">
          <DialogTitle className="flex items-center gap-2 text-base">
            <BookOpen className="size-4 text-primary" />
            Help
          </DialogTitle>
          <DialogDescription className="text-xs">
            Getting started with chat and agent goals. Slash commands, model defaults, Jobs, Providers, and Skills live
            in <strong className="font-medium text-foreground">Settings</strong> (gear icon below).
          </DialogDescription>
        </DialogHeader>

        <ScrollArea className="h-[min(52dvh,400px)] min-h-[10rem] w-full">
          <div className="space-y-4 px-5 py-4 pr-3 text-sm text-muted-foreground">
            <section className="space-y-2">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-foreground">Running an agent</h3>
              <p className="text-xs leading-relaxed">
                Start the node with an agent spec so <strong className="text-foreground">Agent goal</strong> in Chat can
                run multi-step tasks:
              </p>
              <pre className="max-w-full overflow-x-auto whitespace-pre-wrap break-all rounded-lg border border-border bg-muted/50 p-3 font-mono text-[10px] leading-relaxed text-primary">
                peerclaw serve --web 127.0.0.1:8080 --agent examples/agents/assistant.toml
              </pre>
            </section>
            <section className="space-y-2">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-foreground">Chat vs agent</h3>
              <ul className="list-inside list-disc space-y-1.5 text-xs leading-relaxed">
                <li>
                  <strong className="text-foreground">Chat</strong> — streaming assistant; session memory when a session
                  id is set.
                </li>
                <li>
                  <strong className="text-foreground">Agent goal</strong> — submits a task; logs and results stay in the
                  thread; past runs are listed under <strong className="text-foreground">Agent runs</strong> in the left
                  sidebar (like recent chats).
                </li>
              </ul>
            </section>
            <section className="space-y-2">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-foreground">Quick tips</h3>
              <p className="text-xs leading-relaxed">
                Type <kbd className="rounded border border-border px-1 font-mono">/</kbd> in the message box for commands
                (full list under Settings → Commands). Use the sidebar for <strong className="text-foreground">Home</strong>{" "}
                and <strong className="text-foreground">P2P Network</strong>.
              </p>
            </section>
            {onOpenSettings && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="w-full"
                onClick={() => {
                  onOpenChange(false)
                  onOpenSettings()
                }}
              >
                Open Settings
              </Button>
            )}
          </div>
        </ScrollArea>

        <div className="shrink-0 border-t border-border px-5 py-3">
          <Button variant="secondary" className="w-full" size="sm" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
