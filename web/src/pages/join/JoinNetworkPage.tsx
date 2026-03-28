import { useEffect, useState } from "react"
import { Copy, RefreshCw } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { fetchPeers, fetchStatus, fetchSwarmTopology } from "@/lib/api"

export function JoinNetworkPage() {
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
      const [st, topo, plist] = await Promise.all([
        fetchStatus(),
        fetchSwarmTopology(),
        fetchPeers(),
      ])
      setPeerId(st.peer_id)
      setPeers(st.connected_peers)
      setAgents(topo.nodes.length)
      setBalance(st.balance)
      void plist
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
    <div className="mx-auto flex max-w-2xl flex-col gap-8">
      <div>
        <p className="text-xs font-medium uppercase tracking-[0.2em] text-muted-foreground">
          Distributed agents
        </p>
        <h2 className="mt-2 font-serif text-3xl font-semibold tracking-tight text-foreground md:text-4xl">
          Add your node to the mesh
        </h2>
        <p className="mt-3 text-sm leading-relaxed text-muted-foreground">
          PeerClaw runs inference, crew orchestration, and P2P tasks across many machines. Share capacity,
          earn PCLAW, and let other peers route work to you when you are online.
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
          <CardTitle className="text-lg">Run a node</CardTitle>
          <CardDescription>
            Your peer id (from this dashboard):{" "}
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
    </div>
  )
}
