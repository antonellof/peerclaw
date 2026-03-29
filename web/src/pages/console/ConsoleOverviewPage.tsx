import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { Link, useLocation, useNavigate } from "react-router-dom"

import { workspaceHref } from "@/workspace/views"

import { useControlWebSocket } from "@/hooks/useControlWebSocket"
import {
  dialPeer,
  fetchNodeDetail,
  fetchPeers,
  fetchPeersNetwork,
  fetchStatus,
  fetchSwarmAgents,
  fetchSwarmTimeline,
  fetchSwarmTopology,
  type NodeDetailResponse,
  type P2pNetworkResponse,
  type SwarmActionInfo,
  type SwarmAgentInfo,
  type TopologyEdge,
  type TopologyNode,
} from "@/lib/api"
import { ForceGraph } from "@/components/graphs/ForceGraph"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { cn } from "@/lib/utils"
import { JoinMeshSection } from "@/pages/console/JoinMeshSection"

const SECTIONS = [
  { id: "health", label: "Resources" },
  { id: "join-mesh", label: "Join the mesh" },
  { id: "p2p", label: "P2P mesh" },
  { id: "swarm", label: "Swarm" },
] as const

function ResourceMeter({
  label,
  valueLabel,
  pct,
  barClass,
}: {
  label: string
  valueLabel: string
  pct: number
  barClass: string
}) {
  const clamped = Math.min(100, Math.max(0, pct))
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-xs font-medium text-muted-foreground">{label}</span>
        <span className="font-mono text-xs tabular-nums text-foreground">{valueLabel}</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-muted">
        <div
          className={cn("h-full rounded-full transition-[width] duration-500", barClass)}
          style={{ width: `${clamped}%` }}
        />
      </div>
    </div>
  )
}

/** Console overview: node resources, libp2p mesh, and swarm — metrics deduped vs header; swarm as tabs. */
export function ConsoleOverviewPage() {
  const location = useLocation()
  const navigate = useNavigate()
  const [st, setSt] = useState<Awaited<ReturnType<typeof fetchStatus>> | null>(null)
  const [peerList, setPeerList] = useState<{ id: string }[]>([])
  const [p2pNet, setP2pNet] = useState<P2pNetworkResponse | null>(null)
  const [peerFilter, setPeerFilter] = useState("")
  const [dialAddr, setDialAddr] = useState("")
  const [dialBusy, setDialBusy] = useState(false)
  const [dialHint, setDialHint] = useState<string | null>(null)
  const [agents, setAgents] = useState<SwarmAgentInfo[]>([])
  const [topoNodes, setTopoNodes] = useState<TopologyNode[]>([])
  const [topoEdges, setTopoEdges] = useState<TopologyEdge[]>([])
  const [actions, setActions] = useState<SwarmActionInfo[]>([])

  const [nodeOpen, setNodeOpen] = useState(false)
  const [nodeDetail, setNodeDetail] = useState<NodeDetailResponse | null>(null)
  const [loadingNode, setLoadingNode] = useState(false)

  const [activeSection, setActiveSection] = useState<string>("health")
  const sectionRefs = useRef<Record<string, HTMLElement | null>>({})

  const displayPeers = useMemo(() => {
    const q = peerFilter.trim().toLowerCase()
    if (!q) return peerList
    return peerList.filter((p) => p.id.toLowerCase().includes(q))
  }, [peerList, peerFilter])

  const load = async () => {
    try {
      const [s, p, net, a, t, tl] = await Promise.all([
        fetchStatus(),
        fetchPeers(),
        fetchPeersNetwork().catch(() => null),
        fetchSwarmAgents(),
        fetchSwarmTopology(),
        fetchSwarmTimeline(),
      ])
      setSt(s)
      setPeerList(p.map((x) => ({ id: x.id })))
      setP2pNet(net)
      setAgents(a.agents)
      setTopoNodes(t.nodes)
      setTopoEdges(t.edges)
      setActions(tl.actions)
    } catch {
      setSt(null)
    }
  }

  useEffect(() => {
    void load()
    const i = setInterval(load, 12000)
    return () => clearInterval(i)
  }, [])

  useEffect(() => {
    const id = location.hash.replace(/^#/, "")
    if (id && SECTIONS.some((s) => s.id === id)) {
      requestAnimationFrame(() => {
        document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" })
        setActiveSection(id)
      })
    }
  }, [location.pathname, location.hash])

  useEffect(() => {
    const els = SECTIONS.map((s) => sectionRefs.current[s.id]).filter(Boolean) as HTMLElement[]
    if (els.length === 0) return

    const obs = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => b.intersectionRatio - a.intersectionRatio)[0]
        if (visible?.target.id) setActiveSection(visible.target.id)
      },
      { rootMargin: "-20% 0px -55% 0px", threshold: [0, 0.25, 0.5, 1] },
    )

    els.forEach((el) => obs.observe(el))
    return () => obs.disconnect()
  }, [st, peerList.length, agents.length])

  useControlWebSocket({
    onStatus: () => void load(),
  })

  const scrollToSection = useCallback(
    (id: string) => {
      navigate(`${location.pathname}#${id}`, { replace: true })
      requestAnimationFrame(() => {
        document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" })
      })
      setActiveSection(id)
    },
    [location.pathname, navigate],
  )

  const openNode = async (id: string) => {
    setNodeOpen(true)
    setLoadingNode(true)
    setNodeDetail(null)
    try {
      setNodeDetail(await fetchNodeDetail(id))
    } catch {
      setNodeDetail(null)
    } finally {
      setLoadingNode(false)
    }
  }

  const localId = st?.peer_id ?? "local"

  const p2pNodes = [
    { id: localId, type: "local" as const, label: "You" },
    ...displayPeers.map((p) => ({ id: p.id, type: "peer" as const, label: "…" + p.id.slice(-8) })),
  ]
  const p2pLinks = displayPeers.map((p) => ({ source: localId, target: p.id }))

  const queueDial = async (multiaddr: string) => {
    const trimmed = multiaddr.trim()
    if (!trimmed) return
    setDialBusy(true)
    setDialHint(null)
    try {
      const res = await dialPeer(trimmed)
      if (res.ok) {
        setDialHint("Dial queued — check connected peers in a few seconds.")
        void load()
      } else {
        setDialHint(res.error ?? "Dial failed")
      }
    } catch (e) {
      setDialHint(e instanceof Error ? e.message : "Dial failed")
    } finally {
      setDialBusy(false)
    }
  }

  const swarmFgNodes = topoNodes.map((n) => ({
    id: n.id,
    label: n.name?.slice(0, 12) || n.id.slice(0, 8),
    type: n.is_local ? ("local" as const) : ("peer" as const),
  }))
  const swarmFgLinks = topoEdges.map((e) => ({ source: e.source, target: e.target }))

  const cpuPct = st ? Math.round(st.cpu_usage * 100) : 0
  const ramPct =
    st && st.ram_total_mb > 0 ? Math.round((st.ram_used_mb / st.ram_total_mb) * 100) : 0
  const gpuPct = st?.gpu_usage != null ? Math.round(st.gpu_usage * 100) : null

  const online = st != null

  return (
    <div className="space-y-8 pb-12">
      {/* Hero + in-page nav */}
      <div className="relative overflow-hidden rounded-2xl border border-border bg-gradient-to-br from-card via-card to-muted/30 p-6 md:p-8">
        <div className="pointer-events-none absolute -right-20 -top-20 size-64 rounded-full bg-primary/5 blur-3xl" />
        <div className="pointer-events-none absolute -bottom-16 left-1/3 size-48 rounded-full bg-violet-500/10 blur-3xl" />
        <div className="relative flex flex-col gap-5 md:flex-row md:items-end md:justify-between">
          <div>
            <div className="flex flex-wrap items-center gap-2">
              <h1 className="text-xl font-semibold tracking-tight md:text-2xl">P2P Network</h1>
              <Badge variant={online ? "default" : "secondary"} className="font-normal">
                {online ? "Node online" : "Offline"}
              </Badge>
            </div>
            <p className="mt-2 max-w-xl text-sm leading-relaxed text-muted-foreground">
              Resources, how to join the mesh, libp2p connectivity, and swarm agents. Peer count and balance stay in the
              header so they are not repeated here.
            </p>
          </div>
          <div className="flex flex-wrap gap-2 md:justify-end">
            <Button variant="outline" size="sm" asChild>
              <Link to={workspaceHref("jobs")}>Jobs</Link>
            </Button>
            <Button variant="outline" size="sm" asChild>
              <Link to={workspaceHref("providers")}>Providers</Link>
            </Button>
            <Button variant="outline" size="sm" asChild>
              <Link to="/">Chat</Link>
            </Button>
            <Button variant="outline" size="sm" asChild>
              <Link to={workspaceHref("workflows")}>Workflows</Link>
            </Button>
          </div>
        </div>

        <nav
          className="relative mt-6 flex flex-wrap gap-1.5 border-t border-border/60 pt-5"
          aria-label="P2P Network sections"
        >
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => scrollToSection(s.id)}
              className={cn(
                "rounded-full px-3 py-1.5 text-xs font-medium transition-colors",
                activeSection === s.id
                  ? "bg-foreground text-background"
                  : "bg-muted/80 text-muted-foreground hover:bg-muted hover:text-foreground",
              )}
            >
              {s.label}
            </button>
          ))}
        </nav>
      </div>

      {/* —— Resources (no duplicate peers / balance / agent count) —— */}
      <section
        ref={(el) => {
          sectionRefs.current.health = el
        }}
        id="health"
        className="scroll-mt-28 space-y-4"
      >
        <h2 className="text-sm font-semibold uppercase tracking-wider text-muted-foreground">Node resources</h2>
        <div className="grid gap-4 lg:grid-cols-3">
          <Card className="border-border/80 lg:col-span-2">
            <CardHeader className="pb-3">
              <CardTitle className="text-base">Load</CardTitle>
            </CardHeader>
            <CardContent className="space-y-5">
              <ResourceMeter
                label="CPU"
                valueLabel={st ? `${cpuPct}%` : "—"}
                pct={cpuPct}
                barClass="bg-primary"
              />
              <ResourceMeter
                label="RAM"
                valueLabel={st ? `${st.ram_used_mb} / ${st.ram_total_mb} MB (${ramPct}%)` : "—"}
                pct={ramPct}
                barClass="bg-emerald-500/80"
              />
              <ResourceMeter
                label="GPU"
                valueLabel={
                  gpuPct != null ? `${gpuPct}%` : st ? "Not reported" : "—"
                }
                pct={gpuPct ?? 0}
                barClass={gpuPct != null ? "bg-amber-500/80" : "bg-muted-foreground/20"}
              />
            </CardContent>
          </Card>
          <Card className="border-border/80">
            <CardHeader className="pb-3">
              <CardTitle className="text-base">Marketplace</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4 text-sm">
              <div className="flex items-center justify-between rounded-lg border border-border/60 bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Active jobs</span>
                <span className="font-mono font-semibold">{st?.active_jobs ?? "—"}</span>
              </div>
              <div className="flex items-center justify-between rounded-lg border border-border/60 bg-muted/20 px-3 py-2">
                <span className="text-muted-foreground">Completed</span>
                <span className="font-mono font-semibold text-emerald-600 dark:text-emerald-400">
                  {st?.completed_jobs ?? "—"}
                </span>
              </div>
              <Button variant="secondary" className="w-full" size="sm" asChild>
                <Link to={workspaceHref("jobs")}>Open jobs</Link>
              </Button>
            </CardContent>
          </Card>
        </div>
      </section>

      <JoinMeshSection
        ref={(el) => {
          sectionRefs.current["join-mesh"] = el
        }}
      />

      {/* —— P2P —— */}
      <section
        ref={(el) => {
          sectionRefs.current.p2p = el
        }}
        id="p2p"
        className="scroll-mt-28 space-y-4"
      >
        <div>
          <h2 className="text-sm font-semibold uppercase tracking-wider text-muted-foreground">Libp2p mesh</h2>
          <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
            Transport-level peers (Noise, Kademlia, GossipSub). This is not the same graph as swarm agent topology below.
          </p>
        </div>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Grow your network</CardTitle>
            <p className="text-sm text-muted-foreground">
              Dial a multiaddr, reuse bootstraps from config, or rely on LAN discovery when mDNS is on.
            </p>
          </CardHeader>
          <CardContent className="space-y-4 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              {p2pNet ? (
                <>
                  <Badge variant={p2pNet.mdns_enabled ? "default" : "secondary"} className="font-normal">
                    mDNS {p2pNet.mdns_enabled ? "on" : "off"}
                  </Badge>
                  <Badge variant={p2pNet.kademlia_enabled ? "default" : "secondary"} className="font-normal">
                    Kademlia {p2pNet.kademlia_enabled ? "on" : "off"}
                  </Badge>
                  {!p2pNet.dial_supported && (
                    <Badge variant="outline" className="font-normal">
                      Dial API unavailable (not full node web)
                    </Badge>
                  )}
                </>
              ) : (
                <span className="text-xs text-muted-foreground">P2P settings load with the overview…</span>
              )}
            </div>

            <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
              <div className="min-w-0 flex-1 space-y-1">
                <label htmlFor="dial-multiaddr" className="text-xs font-medium text-muted-foreground">
                  Peer multiaddr
                </label>
                <Input
                  id="dial-multiaddr"
                  placeholder="/ip4/…/tcp/…/p2p/…"
                  value={dialAddr}
                  onChange={(e) => setDialAddr(e.target.value)}
                  disabled={dialBusy || p2pNet?.dial_supported === false}
                  className="font-mono text-xs"
                />
              </div>
              <Button
                type="button"
                size="sm"
                disabled={dialBusy || !dialAddr.trim() || p2pNet?.dial_supported === false}
                onClick={() => void queueDial(dialAddr)}
              >
                {dialBusy ? "Connecting…" : "Connect"}
              </Button>
            </div>
            {dialHint && <p className="text-xs text-muted-foreground">{dialHint}</p>}

            {p2pNet && p2pNet.bootstrap_peers.length > 0 && (
              <div className="space-y-2">
                <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  Config bootstraps
                </div>
                <ul className="space-y-2">
                  {p2pNet.bootstrap_peers.map((addr) => (
                    <li
                      key={addr}
                      className="flex flex-col gap-2 rounded-lg border border-border/60 bg-muted/10 p-2 sm:flex-row sm:items-center sm:justify-between"
                    >
                      <span className="break-all font-mono text-[11px] text-foreground">{addr}</span>
                      <Button
                        type="button"
                        variant="secondary"
                        size="sm"
                        className="shrink-0"
                        disabled={dialBusy || !p2pNet.dial_supported}
                        onClick={() => void queueDial(addr)}
                      >
                        Dial
                      </Button>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            <div className="space-y-2">
              <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                Public directory
              </div>
              {p2pNet && p2pNet.community_peers.length > 0 ? (
                <ul className="space-y-2">
                  {p2pNet.community_peers.map((c) => (
                    <li
                      key={c.multiaddr}
                      className="flex flex-col gap-2 rounded-lg border border-border/60 bg-muted/10 p-2 sm:flex-row sm:items-center sm:justify-between"
                    >
                      <div className="min-w-0">
                        <div className="text-xs font-medium text-foreground">{c.label}</div>
                        <span className="break-all font-mono text-[11px] text-muted-foreground">
                          {c.multiaddr}
                        </span>
                      </div>
                      <Button
                        type="button"
                        variant="secondary"
                        size="sm"
                        className="shrink-0"
                        disabled={dialBusy || !p2pNet.dial_supported}
                        onClick={() => void queueDial(c.multiaddr)}
                      >
                        Connect
                      </Button>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="rounded-lg border border-dashed border-border/70 bg-muted/5 p-3 text-xs text-muted-foreground">
                  No curated public relays in this build. Share a multiaddr out-of-band, add bootstraps in{" "}
                  <code className="rounded bg-muted px-1 py-0.5 font-mono text-[10px]">config.toml</code>, or use{" "}
                  <code className="rounded bg-muted px-1 py-0.5 font-mono text-[10px]">
                    peerclaw peers join &lt;multiaddr&gt;
                  </code>{" "}
                  from the CLI.
                </p>
              )}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2 pb-2">
            <CardTitle className="text-base">Connected peers</CardTitle>
            <span className="text-xs text-muted-foreground">
              {peerList.length} remote
              {peerFilter.trim() ? ` · ${displayPeers.length} shown` : ""}
            </span>
          </CardHeader>
          <CardContent>
            <div className="grid gap-6 lg:grid-cols-5">
              <div className="lg:col-span-3">
                <ForceGraph nodes={p2pNodes} links={p2pLinks} height={320} variant="network" />
              </div>
              <div className="flex flex-col gap-2 lg:col-span-2">
                <div className="rounded-lg border border-dashed border-border/80 bg-muted/10 px-3 py-2">
                  <div className="text-[10px] font-medium uppercase tracking-wide text-muted-foreground">Local peer</div>
                  <p className="mt-1 break-all font-mono text-[11px] leading-snug text-foreground">{localId}</p>
                </div>
                <div className="space-y-1.5">
                  <label htmlFor="peer-filter" className="text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
                    Filter peer IDs
                  </label>
                  <Input
                    id="peer-filter"
                    placeholder="Search connected peer id…"
                    value={peerFilter}
                    onChange={(e) => setPeerFilter(e.target.value)}
                    className="h-8 font-mono text-[11px]"
                  />
                </div>
                <div className="min-h-0 flex-1 space-y-2 overflow-y-auto rounded-lg border border-border/60 bg-muted/10 p-2 max-h-[220px]">
                  {peerList.length === 0 ? (
                    <p className="p-2 text-sm text-muted-foreground">No remote peers. Try a bootstrap multiaddr.</p>
                  ) : displayPeers.length === 0 ? (
                    <p className="p-2 text-sm text-muted-foreground">No peers match this filter.</p>
                  ) : (
                    displayPeers.map((p) => (
                      <div key={p.id} className="rounded-md border border-border/50 bg-background/50 px-2.5 py-2">
                        <p className="break-all font-mono text-[11px] leading-snug text-muted-foreground">{p.id}</p>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      </section>

      {/* —— Swarm (tabs reduce vertical duplicate feel) —— */}
      <section
        ref={(el) => {
          sectionRefs.current.swarm = el
        }}
        id="swarm"
        className="scroll-mt-28 space-y-4"
      >
        <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <h2 className="text-sm font-semibold uppercase tracking-wider text-muted-foreground">Swarm</h2>
            <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
              Registered agents and their relationships.{" "}
              <Link to="/" className="text-primary underline-offset-4 hover:underline">
                Run agent goals
              </Link>{" "}
              from Chat (Agent goal); jobs and providers are separate flows.
            </p>
          </div>
          <Badge variant="outline" className="w-fit font-mono text-xs">
            {agents.length} agent{agents.length === 1 ? "" : "s"}
          </Badge>
        </div>

        <Tabs defaultValue="agents" className="w-full">
          <TabsList className="h-auto w-full flex-wrap justify-start gap-1 bg-muted/50 p-1 sm:w-auto">
            <TabsTrigger value="agents" className="text-xs sm:text-sm">
              Agents
            </TabsTrigger>
            <TabsTrigger value="topology" className="text-xs sm:text-sm">
              Topology
            </TabsTrigger>
            <TabsTrigger value="activity" className="text-xs sm:text-sm">
              Activity
            </TabsTrigger>
          </TabsList>

          <TabsContent value="agents" className="mt-4 focus-visible:outline-none">
            {agents.length === 0 ? (
              <Card>
                <CardContent className="py-10 text-center text-sm text-muted-foreground">
                  No agents registered. Example:{" "}
                  <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-primary">
                    peerclaw serve --web 127.0.0.1:8080 --agent templates/agents/coder.toml
                  </code>
                </CardContent>
              </Card>
            ) : (
              <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
                {agents.map((a) => (
                  <Card
                    key={a.id}
                    className="cursor-pointer transition-colors hover:border-violet-500/35"
                    onClick={() => void openNode(a.id)}
                  >
                    <CardHeader className="pb-2">
                      <div className="flex items-start justify-between gap-2">
                        <CardTitle className="text-base leading-tight">{a.name}</CardTitle>
                        <Badge variant={a.is_local ? "default" : "secondary"} className="shrink-0">
                          {a.is_local ? "Local" : "Remote"}
                        </Badge>
                      </div>
                    </CardHeader>
                    <CardContent className="grid grid-cols-2 gap-x-2 gap-y-1 text-xs">
                      <span className="text-muted-foreground">State</span>
                      <span className="text-right text-foreground">{a.state}</span>
                      <span className="text-muted-foreground">Actions</span>
                      <span className="text-right font-mono text-foreground">{a.action_count}</span>
                      <span className="text-muted-foreground">Success</span>
                      <span className="text-right font-mono text-foreground">
                        {(a.success_rate * 100).toFixed(0)}%
                      </span>
                    </CardContent>
                  </Card>
                ))}
              </div>
            )}
          </TabsContent>

          <TabsContent value="topology" className="mt-4 focus-visible:outline-none">
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-base">Agent graph</CardTitle>
                <p className="text-xs font-normal text-muted-foreground">
                  Click a node to open details (different from the libp2p mesh above).
                </p>
              </CardHeader>
              <CardContent>
                {swarmFgNodes.length === 0 ? (
                  <p className="py-8 text-center text-sm text-muted-foreground">No swarm topology data yet.</p>
                ) : (
                  <ForceGraph
                    nodes={swarmFgNodes}
                    links={swarmFgLinks}
                    height={340}
                    variant="swarm"
                    onNodeClick={(id) => void openNode(id)}
                  />
                )}
              </CardContent>
            </Card>
          </TabsContent>

          <TabsContent value="activity" className="mt-4 focus-visible:outline-none">
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-base">Recent actions</CardTitle>
              </CardHeader>
              <CardContent className="max-h-80 space-y-2 overflow-y-auto pr-1">
                {actions.length === 0 ? (
                  <p className="py-6 text-center text-sm text-muted-foreground">No recent activity.</p>
                ) : (
                  actions.map((ac) => (
                    <div
                      key={ac.id}
                      className="rounded-lg border border-border/70 bg-muted/15 px-3 py-2.5 text-sm transition-colors hover:bg-muted/25"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground">
                        <span className="font-medium text-foreground/90">{ac.agent_name}</span>
                        <time dateTime={new Date(ac.timestamp).toISOString()} className="font-mono tabular-nums">
                          {new Date(ac.timestamp).toLocaleString()}
                        </time>
                      </div>
                      <div className="mt-1 font-mono text-xs text-primary">{ac.action_type}</div>
                      {ac.details ? (
                        <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{ac.details}</p>
                      ) : null}
                    </div>
                  ))
                )}
              </CardContent>
            </Card>
          </TabsContent>
        </Tabs>
      </section>

      <Dialog open={nodeOpen} onOpenChange={setNodeOpen}>
        <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{nodeDetail?.name ?? "Node"}</DialogTitle>
            <DialogDescription className="break-all font-mono text-xs">{nodeDetail?.id}</DialogDescription>
          </DialogHeader>
          {loadingNode && <p className="text-sm text-muted-foreground">Loading…</p>}
          {nodeDetail && (
            <div className="space-y-4 text-sm">
              <div className="flex flex-wrap gap-3 text-xs">
                <span>State: {nodeDetail.state}</span>
                <span>Local: {nodeDetail.is_local ? "yes" : "no"}</span>
                <span>Success: {(nodeDetail.success_rate * 100).toFixed(0)}%</span>
              </div>
              {nodeDetail.models.length > 0 && (
                <div>
                  <div className="text-xs font-medium text-muted-foreground">Models</div>
                  <div className="mt-2 space-y-1 border-t border-border pt-2">
                    {nodeDetail.models.map((m) => (
                      <div key={m.model_name} className="flex justify-between py-1 text-xs">
                        <span className="text-primary">{m.model_name}</span>
                        <span>{m.price_per_1k_tokens} µ/1k</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
              {nodeDetail.tasks.length > 0 && (
                <div>
                  <div className="text-xs font-medium text-muted-foreground">Tasks</div>
                  <div className="mt-2 space-y-2 border-t border-border pt-2">
                    {nodeDetail.tasks.slice(0, 8).map((t) => (
                      <div key={t.id} className="text-xs">
                        <Badge variant="outline" className="mb-1">
                          {t.status}
                        </Badge>
                        <p className="line-clamp-3 text-muted-foreground">{t.description}</p>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  )
}
