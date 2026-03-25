import { useEffect, useState } from "react"

import { fetchJobs, submitJob, type WebJobInfo } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"

export function ConsoleJobsPage() {
  const [jobs, setJobs] = useState<WebJobInfo[]>([])
  const [jobType, setJobType] = useState("inference")
  const [budget, setBudget] = useState(1)
  const [payload, setPayload] = useState("")
  const [busy, setBusy] = useState(false)

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

  const onSubmit = async () => {
    setBusy(true)
    try {
      await submitJob({ job_type: jobType, budget, payload })
      setPayload("")
      await load()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="grid gap-6 lg:grid-cols-2">
      <Card>
        <CardHeader>
          <CardTitle>Submit job</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label>Job type</Label>
              <select
                className="flex h-9 w-full rounded-md border border-input bg-background px-2 text-sm"
                value={jobType}
                onChange={(e) => setJobType(e.target.value)}
              >
                <option value="inference">Inference</option>
                <option value="web_fetch">Web fetch</option>
                <option value="wasm">WASM</option>
              </select>
            </div>
            <div className="space-y-2">
              <Label>Budget (PCLAW)</Label>
              <Input type="number" step={0.1} min={0.1} value={budget} onChange={(e) => setBudget(parseFloat(e.target.value) || 1)} />
            </div>
          </div>
          <div className="space-y-2">
            <Label>Prompt / URL / tool</Label>
            <Textarea value={payload} onChange={(e) => setPayload(e.target.value)} rows={4} placeholder="Inference prompt, URL, or tool name…" />
          </div>
          <Button disabled={busy || !payload.trim()} onClick={() => void onSubmit()}>
            Submit to network
          </Button>

          <div className="rounded-lg border border-border p-4">
            <div className="text-xs font-medium text-muted-foreground">Distribution</div>
            <div className="mt-2 space-y-2 text-sm">
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
