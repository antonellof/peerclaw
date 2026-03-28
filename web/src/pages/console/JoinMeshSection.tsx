import { forwardRef, useEffect, useState } from "react"
import { Copy, RefreshCw } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { fetchStatus, fetchSwarmTopology } from "@/lib/api"

/** Mesh onboarding: stats + copy-paste serve commands (embedded in P2P Network). */
export const JoinMeshSection = forwardRef<HTMLElement>(function JoinMeshSection(_, ref) {
  const [peerId, setPeerId] = useState("…")
  const [peers, setPeers] = useState(0)
  const [agents, setAgents] = useState(0)
  const [balance, setBalance] = useState(0)
  const [err, setErr] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = async () => {
    setLoading(true)
    setErr(null)
    try {
      const [st, topo] = await Promise.all([fetchStatus(), fetchSwarmTopology()])
      setPeerId(st.peer_id)
      setPeers(st.connected_peers)
      setAgents(topo.nodes.length)
      setBalance(st.balance)
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Failed to load network stats")
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
    const t = setInterval(() => void load(), 15000)
    return () => clearInterval(t)
  }, [])

  const cmd =
    "peerclaw serve --web 127.0.0.1:8080 --share-inference\n# Optional crew worker:\npeerclaw serve --web 127.0.0.1:8080 --crew-worker"

  return (
    <section
      ref={ref}
      id="join-mesh"
      className="scroll-mt-28 space-y-4"
      aria-labelledby="join-mesh-heading"
    >
      <div>
        <h2 id="join-mesh-heading" className="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          Join the mesh
        </h2>
        <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
          Share inference capacity, earn PCLAW, and optionally run as a <code className="rounded bg-muted px-1 py-0.5 font-mono text-[10px]">--crew-worker</code> so other peers can claim distributed crew steps.
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-3">
        <Card className="border-border/80 bg-card/40">
          <CardHeader className="pb-2">
            <CardDescription>Connected peers</CardDescription>
            <CardTitle className="font-mono text-2xl tabular-nums">
              {loading ? "…" : peers}
            </CardTitle>
          </CardHeader>
        </Card>
        <Card className="border-border/80 bg-card/40">
          <CardHeader className="pb-2">
            <CardDescription>Swarm agents</CardDescription>
            <CardTitle className="font-mono text-2xl tabular-nums">
              {loading ? "…" : agents}
            </CardTitle>
          </CardHeader>
        </Card>
        <Card className="border-border/80 bg-card/40">
          <CardHeader className="pb-2">
            <CardDescription>Wallet balance</CardDescription>
            <CardTitle className="font-mono text-2xl tabular-nums">
              {loading ? "…" : `${balance.toFixed(2)}`}
            </CardTitle>
          </CardHeader>
        </Card>
      </div>

      {err ? (
        <p className="text-sm text-destructive" role="alert">
          {err}
        </p>
      ) : null}

      <Card className="border-primary/20 bg-gradient-to-br from-primary/5 to-transparent">
        <CardHeader>
          <CardTitle className="text-lg">Run a contributing node</CardTitle>
          <CardDescription>
            Your peer id on this dashboard:{" "}
            <span className="font-mono text-xs text-foreground/90">{peerId}</span>
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <pre className="overflow-x-auto rounded-md border border-border/80 bg-muted/40 p-3 text-[11px] leading-relaxed text-foreground">
            {cmd}
          </pre>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="gap-2"
              onClick={() => void navigator.clipboard.writeText(cmd)}
            >
              <Copy className="size-3.5" />
              Copy commands
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="gap-2"
              onClick={() => void load()}
              disabled={loading}
            >
              <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />
              Refresh stats
            </Button>
          </div>
        </CardContent>
      </Card>
    </section>
  )
})
