import type { WorkspaceView } from "./views"
import { ConsoleHelpPage } from "@/pages/console/ConsoleHelpPage"
import { ConsoleJobsPage } from "@/pages/console/ConsoleJobsPage"
import { ConsoleOverviewPage } from "@/pages/console/ConsoleOverviewPage"
import { ConsoleProvidersPage } from "@/pages/console/ConsoleProvidersPage"
import { ConsoleSkillsPage } from "@/pages/console/ConsoleSkillsPage"
import { ConsoleMcpPage } from "@/pages/console/ConsoleMcpPage"
import { AgentBuilderPage } from "@/pages/console/agent-builder/AgentBuilderPage"

const TITLES: Partial<Record<WorkspaceView, string>> = {
  help: "Help & getting started",
  overview: "P2P Network",
  crews: "Agent builder",
  jobs: "Jobs",
  providers: "Providers",
  skills: "Skills",
  mcp: "MCP",
}

const SHOW_TOP_BAR: Record<Exclude<WorkspaceView, "chat">, boolean> = {
  help: true,
  overview: false,
  crews: false,
  jobs: true,
  providers: true,
  skills: true,
  mcp: true,
}

export function ConsolePanel({ view }: { view: Exclude<WorkspaceView, "chat"> }) {
  const title = TITLES[view] ?? "Console"
  const showBar = SHOW_TOP_BAR[view]

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
      {showBar ? (
        <header className="flex h-12 shrink-0 items-center border-b border-border/80 px-4 md:px-6">
          <h1 className="text-sm font-semibold tracking-tight text-foreground">{title}</h1>
        </header>
      ) : null}
      <div
        className={
          view === "crews"
            ? "min-h-0 flex-1 overflow-hidden"
            : "min-h-0 flex-1 overflow-y-auto px-4 py-5 md:px-6"
        }
      >
        {view === "help" && <ConsoleHelpPage />}
        {view === "overview" && <ConsoleOverviewPage />}
        {view === "crews" && (
          <div className="h-full min-h-0">
            <AgentBuilderPage />
          </div>
        )}
        {view === "jobs" && <ConsoleJobsPage />}
        {view === "providers" && <ConsoleProvidersPage />}
        {view === "skills" && <ConsoleSkillsPage />}
        {view === "mcp" && <ConsoleMcpPage />}
      </div>
    </div>
  )
}
