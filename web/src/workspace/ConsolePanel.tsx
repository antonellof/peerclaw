import type { WorkspaceView } from "./views"
import { ConsoleHelpPage } from "@/pages/console/ConsoleHelpPage"
import { ConsoleJobsPage } from "@/pages/console/ConsoleJobsPage"
import { ConsoleOverviewPage } from "@/pages/console/ConsoleOverviewPage"
import { ConsoleProvidersPage } from "@/pages/console/ConsoleProvidersPage"
import { ConsoleSkillsPage } from "@/pages/console/ConsoleSkillsPage"
import { ConsoleToolsPage } from "@/pages/console/ConsoleToolsPage"
import { ConsoleMcpPage } from "@/pages/console/ConsoleMcpPage"
import { AgentBuilderPage } from "@/pages/console/agent-builder/AgentBuilderPage"

export function ConsolePanel({ view }: { view: Exclude<WorkspaceView, "chat"> }) {
  return (
    <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
      <div
        className={
          view === "workflows"
            ? "min-h-0 flex-1 overflow-hidden"
            : "min-h-0 flex-1 overflow-y-auto px-4 py-5 md:px-6"
        }
      >
        {view === "help" && <ConsoleHelpPage />}
        {view === "overview" && <ConsoleOverviewPage />}
        {view === "workflows" && (
          <div className="h-full min-h-0">
            <AgentBuilderPage />
          </div>
        )}
        {view === "jobs" && <ConsoleJobsPage />}
        {view === "providers" && <ConsoleProvidersPage />}
        {view === "skills" && <ConsoleSkillsPage />}
        {view === "tools" && <ConsoleToolsPage />}
        {view === "mcp" && <ConsoleMcpPage />}
      </div>
    </div>
  )
}
