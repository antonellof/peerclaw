import { useEffect, useState } from "react"

import { fetchProviderConfig, fetchProviders, setProviderConfig, type ProviderInfo } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Separator } from "@/components/ui/separator"

export function ConsoleProvidersPage() {
  const [providers, setProviders] = useState<ProviderInfo[]>([])
  const [enabled, setEnabled] = useState(false)
  const [priceMult, setPriceMult] = useState(1)
  const [maxReq, setMaxReq] = useState(60)
  const [maxTok, setMaxTok] = useState(100000)

  const load = async () => {
    try {
      const [plist, cfg] = await Promise.all([fetchProviders(), fetchProviderConfig()])
      setProviders(plist)
      setEnabled(cfg.enabled)
      setPriceMult(cfg.price_multiplier)
      setMaxReq(cfg.max_requests_per_hour)
      setMaxTok(cfg.max_tokens_per_day)
    } catch {
      setProviders([])
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const save = async () => {
    await setProviderConfig({
      enabled,
      price_multiplier: priceMult,
      max_requests_per_hour: maxReq,
      max_tokens_per_day: maxTok,
    })
    await load()
  }

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle>Local provider</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4 sm:grid-cols-2">
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
            Share inference with network
          </label>
          <div className="space-y-2">
            <Label>Price multiplier</Label>
            <Input type="number" step={0.1} value={priceMult} onChange={(e) => setPriceMult(parseFloat(e.target.value) || 1)} />
          </div>
          <div className="space-y-2">
            <Label>Max requests / hour</Label>
            <Input type="number" value={maxReq} onChange={(e) => setMaxReq(parseInt(e.target.value, 10) || 0)} />
          </div>
          <div className="space-y-2">
            <Label>Max tokens / day</Label>
            <Input type="number" value={maxTok} onChange={(e) => setMaxTok(parseInt(e.target.value, 10) || 0)} />
          </div>
          <div className="sm:col-span-2">
            <Button onClick={() => void save()}>Save settings</Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Network providers</CardTitle>
          <span className="text-xs text-muted-foreground">{providers.length} online</span>
        </CardHeader>
        <CardContent className="space-y-4">
          {providers.length === 0 ? (
            <p className="text-sm text-muted-foreground">No providers discovered.</p>
          ) : (
            providers.map((p) => (
              <div key={p.peer_id} className="rounded-lg border border-border p-4">
                <div className="flex justify-between text-sm">
                  <code className="text-xs">…{p.peer_id.slice(-12)}</code>
                  <span className="text-xs text-muted-foreground">{p.max_requests_per_hour} req/hr</span>
                </div>
                <Separator className="my-3" />
                {p.models.map((m) => (
                  <div key={m.model_name} className="flex justify-between py-1 text-xs">
                    <span className="text-primary">{m.model_name}</span>
                    <span className="text-muted-foreground">
                      {m.price_per_1k_tokens} µ/1k · ctx {m.context_size}
                    </span>
                  </div>
                ))}
              </div>
            ))
          )}
        </CardContent>
      </Card>
    </div>
  )
}
