import {
  apiUrl,
  fetchJobs,
  fetchMcpStatus,
  fetchPeers,
  fetchSkillsLocal,
  fetchSkillsNetwork,
  fetchStatus,
} from "@/lib/api"
import type { WorkspaceView } from "@/workspace/views"

export type SlashCommandDef = { cmd: string; desc: string; args?: string; category: string }

export const SLASH_COMMANDS: SlashCommandDef[] = [
  { cmd: "/help", desc: "Show available commands", category: "General" },
  { cmd: "/guide", desc: "Open help (agents & commands)", category: "General" },
  { cmd: "/open", desc: "Open workspace panel", args: "overview|jobs|…", category: "Workspace" },
  { cmd: "/overview", desc: "Open P2P Network (mesh & swarm)", category: "Workspace" },
  { cmd: "/home", desc: "Open Home (starters)", category: "Workspace" },
  { cmd: "/providers", desc: "Open Providers", category: "Workspace" },
  { cmd: "/status", desc: "Show runtime status", category: "General" },
  { cmd: "/doctor", desc: "Health check", category: "General" },
  { cmd: "/clear", desc: "Clear conversation & new session", category: "General" },
  { cmd: "/model", desc: "Show or switch model", args: "[name]", category: "Model" },
  { cmd: "/temperature", desc: "Set temperature", args: "<0-2>", category: "Model" },
  { cmd: "/max-tokens", desc: "Set max tokens", args: "<n>", category: "Model" },
  { cmd: "/tools", desc: "List available tools", category: "Tools" },
  { cmd: "/skills", desc: "List skills", category: "Skills" },
  { cmd: "/skill", desc: "Skill info", args: "info <name>", category: "Skills" },
  { cmd: "/mcp", desc: "MCP status", category: "Tools" },
  { cmd: "/peers", desc: "Connected peers", category: "Network" },
  { cmd: "/balance", desc: "Wallet balance", category: "Network" },
  { cmd: "/jobs", desc: "Recent jobs", category: "Network" },
  { cmd: "/distributed", desc: "Toggle distributed", args: "on|off", category: "Network" },
  { cmd: "/cost", desc: "Session token stats", category: "Session" },
]

export type ChatSettings = {
  temperature: number
  maxTokens: number
  distributed: boolean
}

export type SlashContext = {
  settings: ChatSettings
  setSettings: (s: Partial<ChatSettings>) => void
  toggleDistributed: () => void
  model: string
  setModel: (m: string) => void
  sessionStats: { tokens: number; requests: number; startTime: number }
  onClearSession: () => void
  /** When set (main app shell), /open and /guide can switch panels. */
  setWorkspaceView?: (v: WorkspaceView) => void
  openHelp?: () => void
}

function formatHelp(): string {
  const by: Record<string, SlashCommandDef[]> = {}
  for (const c of SLASH_COMMANDS) {
    ;(by[c.category] ??= []).push(c)
  }
  let help = "=== PeerClaw Chat Commands ===\n\n"
  for (const [cat, cmds] of Object.entries(by)) {
    help += `${cat}:\n`
    for (const c of cmds) {
      const s = c.cmd + (c.args ? " " + c.args : "")
      help += `  ${s.padEnd(26)} ${c.desc}\n`
    }
    help += "\n"
  }
  return help.trim()
}

export async function runSlashCommand(input: string, ctx: SlashContext): Promise<string> {
  const parts = input.slice(1).split(/\s+/)
  const cmd = parts[0]?.toLowerCase() ?? ""
  const args = parts.slice(1)

  switch (cmd) {
    case "help":
    case "h":
    case "?":
      return formatHelp()

    case "guide":
      ctx.openHelp?.()
      return ctx.openHelp
        ? "Opened help."
        : "Help is available from the sidebar (Help button)."

    case "overview":
      ctx.setWorkspaceView?.("overview")
      return ctx.setWorkspaceView ? "Opened P2P Network." : "Use the sidebar → P2P Network."

    case "providers":
      ctx.setWorkspaceView?.("providers")
      return ctx.setWorkspaceView ? "Opened Providers." : "Use the sidebar → Providers."

    case "home":
      ctx.setWorkspaceView?.("home")
      return ctx.setWorkspaceView ? "Opened Home." : "Use the sidebar → Home."

    case "open":
    case "nav": {
      const target = (args[0] || "").toLowerCase()
      const map: Record<string, WorkspaceView> = {
        chat: "chat",
        home: "home",
        overview: "overview",
        mesh: "overview",
        p2p: "overview",
        swarm: "overview",
        tasks: "chat",
        agent: "chat",
        jobs: "jobs",
        providers: "providers",
        provider: "providers",
        skills: "skills",
      }
      const v = map[target]
      if (!v) {
        return "Usage: /open chat|home|overview|jobs|providers|skills"
      }
      if (!ctx.setWorkspaceView) {
        return "Workspace navigation is only available in the main app shell."
      }
      ctx.setWorkspaceView(v)
      if (v === "chat" && (target === "tasks" || target === "agent")) {
        return "Opened Chat. Switch the composer to **Agent goal** for multi-step agent runs."
      }
      return `Opened ${v === "chat" ? "chat" : v} panel.`
    }

    case "clear":
    case "c":
      ctx.onClearSession()
      return "Conversation cleared. New chat session started for server-side memory."

    case "status":
    case "s": {
      const status = await fetchStatus()
      return `Peer ID: ${status.peer_id.slice(0, 16)}…
Connected peers: ${status.connected_peers}
Balance: ${status.balance.toFixed(6)} PCLAW
CPU: ${(status.cpu_usage * 100).toFixed(1)}%
RAM: ${status.ram_used_mb}/${status.ram_total_mb} MB
Active jobs: ${status.active_jobs}`
    }

    case "model":
    case "m":
      if (args[0]) {
        ctx.setModel(args[0])
        return `Model set to: ${args[0]}`
      }
      return `Current model: ${ctx.model}`

    case "temperature":
    case "temp":
      if (args[0]) {
        const t = parseFloat(args[0]) || 0.7
        ctx.setSettings({ temperature: t })
        return `Temperature set to: ${t}`
      }
      return `Current temperature: ${ctx.settings.temperature}`

    case "max-tokens":
    case "tokens":
      if (args[0]) {
        const n = parseInt(args[0], 10) || 500
        ctx.setSettings({ maxTokens: n })
        return `Max tokens set to: ${n}`
      }
      return `Current max tokens: ${ctx.settings.maxTokens}`

    case "tools":
    case "tool": {
      try {
        const r = await fetch(apiUrl("/api/tools"))
        if (!r.ok) throw new Error(`HTTP ${r.status}`)
        const j = await r.json() as { tools: { name: string; description: string; location: string }[]; count: number }
        if (!j.tools.length) return "No tools available (start node with `peerclaw serve`)."
        const lines = j.tools.map((t: { name: string; description: string }) => `  • **${t.name}** — ${t.description}`)
        return `**${j.count} tools available:**\n${lines.join("\n")}`
      } catch {
        return "Could not fetch tools (node not running?)."
      }
    }

    case "skills":
    case "skill":
      return await handleSkillArgs(args)

    case "peers":
    case "p": {
      const peers = await fetchPeers()
      if (!peers.length) return "No peers connected."
      return (
        `Connected peers (${peers.length}):\n` +
        peers
          .slice(0, 10)
          .map((p) => `  • ${p.id.slice(0, 16)}…`)
          .join("\n") +
        (peers.length > 10 ? `\n  … and ${peers.length - 10} more` : "")
      )
    }

    case "balance":
    case "bal":
    case "wallet": {
      const bal = await fetchStatus()
      return `Wallet balance: ${bal.balance.toFixed(6)} PCLAW`
    }

    case "jobs":
    case "j": {
      const jobs = await fetchJobs()
      if (!jobs.length) return "No jobs."
      return (
        `Jobs (${jobs.length}):\n` +
        jobs
          .slice(0, 5)
          .map((j) => `  • ${j.id.slice(0, 12)}… — ${j.job_type} — ${j.status}`)
          .join("\n")
      )
    }

    case "distributed":
    case "dist":
      if (args[0]) {
        const on = ["on", "true", "1", "yes"].includes(args[0].toLowerCase())
        ctx.setSettings({ distributed: on })
        return `Distributed mode: ${on ? "On" : "Off"}`
      }
      ctx.toggleDistributed()
      return "Distributed mode toggled for this session (UI preference)."

    case "cost": {
      const elapsed = Math.floor((Date.now() - ctx.sessionStats.startTime) / 1000)
      const mins = Math.floor(elapsed / 60)
      const secs = elapsed % 60
      return `Session stats:
  Tokens used: ${ctx.sessionStats.tokens}
  Requests: ${ctx.sessionStats.requests}
  Session time: ${mins}m ${secs}s`
    }

    case "doctor": {
      const status = await fetchStatus()
      const checks: string[] = []
      checks.push(
        status.active_inference > 0 || status.completed_jobs > 0
          ? "✓ Inference path may be warm"
          : "✗ No recent inference — ensure models are available",
      )
      checks.push(
        status.connected_peers > 0
          ? `✓ Network: ${status.connected_peers} peers`
          : "! Network: no peers (standalone)",
      )
      checks.push(
        status.balance > 0 ? `✓ Wallet: ${status.balance.toFixed(2)} PCLAW` : "! Wallet: zero balance",
      )
      return "Health check:\n" + checks.join("\n")
    }

    case "mcp": {
      try {
        const m = await fetchMcpStatus()
        const n = m.config?.servers?.length ?? 0
        const tc = m.tool_count ?? 0
        const cs = m.connected_servers?.length ? m.connected_servers.join(", ") : "none"
        return `MCP mode: ${m.mode}
In core: ${m.in_core}
Servers in config: ${n}
Connected: ${cs}
Tools available: ${tc}
Timeout: ${m.config?.timeout_secs ?? "—"}s

${m.hint}

Spec: ${m.spec_url}`
      } catch (e) {
        return "Could not load MCP status: " + (e instanceof Error ? e.message : String(e))
      }
    }

    default:
      return `Unknown command: /${cmd}\nType /help for available commands.`
  }
}

async function handleSkillArgs(args: string[]): Promise<string> {
  const sub = (args[0] || "list").toLowerCase()

  if (sub === "list" || sub === "local") {
    try {
      const items = await fetchSkillsLocal()
      if (!items.length) return "No local skills. Add SKILL.md under the skills directory."
      return (
        "Local & installed skills:\n" +
        items.map((s) => `• ${s.name} (${s.version}) [${s.trust}] — ${s.description || ""}`).join("\n")
      )
    } catch (e) {
      return "Could not fetch skills: " + (e instanceof Error ? e.message : String(e))
    }
  }

  if (sub === "network" || sub === "p2p") {
    try {
      const items = await fetchSkillsNetwork()
      if (!items.length) return "No peer-advertised skills yet."
      return (
        "Network skills:\n" +
        items
          .map((s) => `• ${s.name} (${s.version}) from ${(s.provider || "").slice(0, 14)}… — ${s.description || ""}`)
          .join("\n")
      )
    } catch (e) {
      return "Error: " + (e instanceof Error ? e.message : String(e))
    }
  }

  if (sub === "info") {
    const name = args[1]
    if (!name) return "Usage: /skill info <name>"
    const [lr, nr] = await Promise.all([fetchSkillsLocal(), fetchSkillsNetwork()])
    const s = [...lr, ...nr].find((x) => x.name === name)
    if (!s) return `Skill "${name}" not found.`
    return `Skill: ${s.name}
Version: ${s.version}
Trust: ${s.trust}
Provider: ${s.provider}
Available: ${s.available}
Price: ${s.price}

${s.description || ""}`
  }

  return "Usage:\n  /skills — local\n  /skills network — P2P\n  /skill info <name>"
}
