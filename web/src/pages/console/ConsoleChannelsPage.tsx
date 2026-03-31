import { useEffect, useState, useCallback } from "react"

import {
  fetchChannels,
  createChannel,
  toggleChannel,
  deleteChannel,
  type ChannelInfo,
} from "@/lib/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"

const PLATFORMS = [
  {
    id: "telegram", label: "Telegram",
    desc: "Native Bot API integration. Create a bot via @BotFather on Telegram to get a token.",
    configFields: [{ key: "bot_token", label: "Bot token", placeholder: "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11" }],
  },
{
    id: "webhook", label: "Webhook",
    desc: "Generic HTTP endpoint. Send POST requests with JSON messages; the agent responds inline.",
    configFields: [{ key: "url", label: "Endpoint URL", placeholder: "https://..." }],
  },
  {
    id: "websocket", label: "WebSocket",
    desc: "Real-time bidirectional channel. Used by the web dashboard chat.",
    configFields: [{ key: "url", label: "WS URL", placeholder: "ws://..." }],
  },
] as const

export function ConsoleChannelsPage() {
  const [channels, setChannels] = useState<ChannelInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  // Add channel form state
  const [selectedPlatform, setSelectedPlatform] = useState<string>(PLATFORMS[0].id)
  const [channelName, setChannelName] = useState("")
  const [configValues, setConfigValues] = useState<Record<string, string>>({})
  const [creating, setCreating] = useState(false)

  const load = useCallback(async () => {
    try {
      setChannels(await fetchChannels())
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load channels")
      setChannels([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
    const i = setInterval(load, 15000)
    return () => clearInterval(i)
  }, [load])

  const platform = PLATFORMS.find((p) => p.id as string === selectedPlatform) ?? PLATFORMS[0]

  const handleCreate = async () => {
    if (!channelName.trim()) return
    setCreating(true)
    try {
      const res = await createChannel({
        platform: selectedPlatform,
        name: channelName.trim(),
        config: configValues,
      })
      if (res.ok) {
        setChannelName("")
        setConfigValues({})
        await load()
      } else {
        setError(res.error ?? "Failed to create channel")
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to create channel")
    } finally {
      setCreating(false)
    }
  }

  const handleToggle = async (ch: ChannelInfo) => {
    const action = ch.status === "connected" ? "disconnect" : "connect"
    try {
      await toggleChannel(ch.id, action)
      await load()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Toggle failed")
    }
  }

  const handleDelete = async (id: string) => {
    try {
      await deleteChannel(id)
      await load()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Delete failed")
    }
  }

  return (
    <div className="grid gap-6 lg:grid-cols-2">
      {/* Channel list */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Active channels</CardTitle>
          <span className="text-xs text-muted-foreground">{channels.length} total</span>
        </CardHeader>
        <CardContent className="max-h-[600px] space-y-3 overflow-y-auto">
          {loading ? (
            <p className="text-sm text-muted-foreground">Loading...</p>
          ) : error && channels.length === 0 ? (
            <p className="text-sm text-muted-foreground">{error}</p>
          ) : channels.length === 0 ? (
            <p className="text-sm text-muted-foreground">No channels configured yet.</p>
          ) : (
            channels.map((ch) => (
              <div
                key={ch.id}
                className="rounded-lg border border-border p-4 text-sm"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-foreground">{ch.name}</span>
                    <Badge variant="outline" className="text-[10px]">
                      {ch.platform}
                    </Badge>
                  </div>
                  <Badge
                    variant={ch.status === "connected" ? "default" : "secondary"}
                    className={
                      ch.status === "connected"
                        ? "bg-emerald-600/20 text-emerald-400 hover:bg-emerald-600/20"
                        : ""
                    }
                  >
                    {ch.status}
                  </Badge>
                </div>
                <div className="mt-2 flex items-center gap-4 text-xs text-muted-foreground">
                  <span>{ch.message_count} messages</span>
                  {ch.last_active && (
                    <span>Last active: {new Date(ch.last_active).toLocaleString()}</span>
                  )}
                </div>
                <div className="mt-3 flex gap-2">
                  <Button
                    size="sm"
                    variant={ch.status === "connected" ? "secondary" : "default"}
                    onClick={() => handleToggle(ch)}
                  >
                    {ch.status === "connected" ? "Disconnect" : "Connect"}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="text-red-400 hover:text-red-300"
                    onClick={() => handleDelete(ch.id)}
                  >
                    Remove
                  </Button>
                </div>
              </div>
            ))
          )}
        </CardContent>
      </Card>

      {/* Add channel */}
      <Card>
        <CardHeader>
          <CardTitle>Add channel</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label>Platform</Label>
            <div className="flex flex-wrap gap-2">
              {PLATFORMS.map((p) => (
                <Button
                  key={p.id}
                  size="sm"
                  variant={selectedPlatform === p.id ? "default" : "outline"}
                  onClick={() => {
                    setSelectedPlatform(p.id)
                    setConfigValues({})
                  }}
                >
                  {p.label}
                </Button>
              ))}
            </div>
          </div>

          {platform.desc && (
            <p className="text-xs text-muted-foreground">{platform.desc}</p>
          )}

          <div className="space-y-2">
            <Label htmlFor="ch-name">Channel name</Label>
            <Input
              id="ch-name"
              placeholder={`My ${platform.label} channel`}
              value={channelName}
              onChange={(e) => setChannelName(e.target.value)}
            />
          </div>

          {platform.configFields.map((f) => (
            <div key={f.key} className="space-y-2">
              <Label htmlFor={`ch-${f.key}`}>{f.label}</Label>
              <Input
                id={`ch-${f.key}`}
                placeholder={f.placeholder}
                type={f.key.includes("token") ? "password" : "text"}
                value={configValues[f.key] ?? ""}
                onChange={(e) =>
                  setConfigValues((prev) => ({ ...prev, [f.key]: e.target.value }))
                }
              />
            </div>
          ))}

          {error && <p className="text-xs text-red-400">{error}</p>}

          <Button onClick={handleCreate} disabled={creating || !channelName.trim()}>
            {creating ? "Creating..." : "Add channel"}
          </Button>
        </CardContent>
      </Card>
    </div>
  )
}
