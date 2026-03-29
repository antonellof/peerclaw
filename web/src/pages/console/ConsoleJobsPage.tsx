import { useEffect, useState } from "react"

import { fetchJobs, type WebJobInfo } from "@/lib/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

export function ConsoleJobsPage() {
  const [jobs, setJobs] = useState<WebJobInfo[]>([])

  const load = async () => {
    try {
      setJobs(await fetchJobs())
    } catch {
      setJobs([])
    }
  }

  useEffect(() => {
    void load()
    const i = setInterval(load, 20000)
    return () => clearInterval(i)
  }, [])

  const localN = jobs.filter((j) => j.location?.toLowerCase() === "local").length
  const remoteN = jobs.length - localN

  return (
    <div className="grid gap-6 lg:grid-cols-2">
      <Card>
        <CardHeader>
          <CardTitle>P2P jobs</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4 text-sm text-muted-foreground">
          <p>
            Marketplace jobs are submitted by the <strong className="text-foreground">agent</strong> via the{" "}
            <code className="rounded bg-muted px-1 py-0.5 text-xs text-foreground">job_submit</code> tool (from chat
            or a workflow), not from a form here. Use{" "}
            <code className="rounded bg-muted px-1 py-0.5 text-xs text-foreground">job_status</code> with the returned{" "}
            <code className="rounded bg-muted px-1 py-0.5 text-xs text-foreground">job_id</code> to poll until work
            completes on the network.
          </p>
          <p className="text-xs">
            Types: inference, web_fetch, wasm, compute, storage — budget in PCLAW. GossipSub propagates requests; bids
            and execution follow the node job protocol.
          </p>

          <div className="rounded-lg border border-border p-4">
            <div className="text-xs font-medium text-muted-foreground">Distribution</div>
            <div className="mt-2 space-y-2 text-sm text-foreground">
              <div className="flex justify-between">
                <span>Local</span>
                <span>{localN}</span>
              </div>
              <div className="h-2 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full bg-primary transition-all"
                  style={{ width: jobs.length ? `${(localN / jobs.length) * 100}%` : "0%" }}
                />
              </div>
              <div className="flex justify-between">
                <span>Network</span>
                <span>{remoteN}</span>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Recent jobs</CardTitle>
          <span className="text-xs text-muted-foreground">{jobs.length} total</span>
        </CardHeader>
        <CardContent className="max-h-[560px] space-y-3 overflow-y-auto">
          {jobs.length === 0 ? (
            <p className="text-sm text-muted-foreground">No jobs yet.</p>
          ) : (
            jobs.map((j) => (
              <div key={j.id} className="rounded-lg border border-border p-3 text-sm">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <code className="text-xs text-primary">{j.id.slice(0, 14)}…</code>
                  <Badge variant="outline">{j.status}</Badge>
                </div>
                <div className="mt-1 text-xs text-muted-foreground">
                  {j.job_type} · {j.price_micro} µPCLAW
                </div>
              </div>
            ))
          )}
        </CardContent>
      </Card>
    </div>
  )
}
