import { BrowserRouter, Navigate, Route, Routes, useLocation } from "react-router-dom"

import { WorkspaceShell } from "@/workspace/WorkspaceShell"
import { LEGACY_CONSOLE_REDIRECT } from "@/workspace/views"

/** Preserve bookmarks and shared links to `/console/...`. */
function ConsoleRoutesRedirect() {
  const { pathname, hash } = useLocation()
  const seg = pathname.replace(/^\/console\/?/, "").split("/").filter(Boolean)[0] ?? ""
  if (seg === "join") {
    return <Navigate to={{ pathname: "/", search: "?view=overview", hash: "join-mesh" }} replace />
  }
  const v = LEGACY_CONSOLE_REDIRECT[seg] ?? "home"
  const search = v === "chat" ? "" : `?view=${v}`
  const bareHash = hash.startsWith("#") ? hash.slice(1) : hash
  return <Navigate to={{ pathname: "/", search: search || undefined, hash: bareHash || undefined }} replace />
}

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<WorkspaceShell />} />
        <Route path="/console/*" element={<ConsoleRoutesRedirect />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </BrowserRouter>
  )
}
