import { createContext, useContext, type ReactNode } from "react"

import type { WorkspaceChatPreferences } from "@/workspace/workspacePreferences"
import type { WorkspaceView } from "./views"

export type ChatControls = { clearChat: () => void; refreshModels: () => void }

export type WorkspaceNavValue = {
  view: WorkspaceView
  setView: (v: WorkspaceView, hash?: string) => void
  openHelp: () => void
  openSettings: () => void
  chatPreferences: WorkspaceChatPreferences
  setChatPreferences: (u: Partial<WorkspaceChatPreferences>) => void
  registerChatControls: (api: ChatControls | null) => void
}

const WorkspaceNavContext = createContext<WorkspaceNavValue | null>(null)

export function WorkspaceNavProvider({ value, children }: { value: WorkspaceNavValue; children: ReactNode }) {
  return <WorkspaceNavContext.Provider value={value}>{children}</WorkspaceNavContext.Provider>
}

/* eslint-disable react-refresh/only-export-components -- hook is intentionally co-located with Provider */
export function useWorkspaceNav(): WorkspaceNavValue {
  const v = useContext(WorkspaceNavContext)
  if (!v) throw new Error("useWorkspaceNav must be used inside WorkspaceNavProvider")
  return v
}
