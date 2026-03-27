import { useEffect, useState, useCallback } from "react"

import {
  fetchVectorCollections,
  createVectorCollection,
  vectorSearch,
  vectorInsert,
  type VectorCollection,
  type VectorSearchResult,
} from "@/lib/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"

export function ConsoleVectorPage() {
  const [collections, setCollections] = useState<VectorCollection[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  // Create form
  const [newName, setNewName] = useState("")
  const [newDim, setNewDim] = useState("384")
  const [creating, setCreating] = useState(false)

  // Search
  const [searchQuery, setSearchQuery] = useState("")
  const [searchCollection, setSearchCollection] = useState("")
  const [searchResults, setSearchResults] = useState<VectorSearchResult[]>([])
  const [searching, setSearching] = useState(false)

  // Insert
  const [insertCollection, setInsertCollection] = useState("")
  const [insertText, setInsertText] = useState("")
  const [inserting, setInserting] = useState(false)
  const [insertMsg, setInsertMsg] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const cols = await fetchVectorCollections()
      setCollections(cols)
      setError(null)
      if (cols.length > 0 && !searchCollection) setSearchCollection(cols[0].name)
      if (cols.length > 0 && !insertCollection) setInsertCollection(cols[0].name)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load collections")
      setCollections([])
    } finally {
      setLoading(false)
    }
  }, [searchCollection, insertCollection])

  useEffect(() => {
    void load()
  }, [load])

  const handleCreate = async () => {
    if (!newName.trim()) return
    setCreating(true)
    try {
      const res = await createVectorCollection({
        name: newName.trim(),
        dimension: parseInt(newDim, 10) || 384,
      })
      if (res.ok) {
        setNewName("")
        setNewDim("384")
        await load()
      } else {
        setError(res.error ?? "Failed to create collection")
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Create failed")
    } finally {
      setCreating(false)
    }
  }

  const handleSearch = async () => {
    if (!searchQuery.trim() || !searchCollection) return
    setSearching(true)
    setSearchResults([])
    try {
      const results = await vectorSearch({
        collection: searchCollection,
        query: searchQuery.trim(),
        top_k: 10,
      })
      setSearchResults(results)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Search failed")
    } finally {
      setSearching(false)
    }
  }

  const handleInsert = async () => {
    if (!insertText.trim() || !insertCollection) return
    setInserting(true)
    setInsertMsg(null)
    try {
      const res = await vectorInsert({
        collection: insertCollection,
        text: insertText.trim(),
      })
      if (res.ok) {
        setInsertText("")
        setInsertMsg(`Inserted (id: ${res.id ?? "ok"})`)
        await load()
      } else {
        setInsertMsg(res.error ?? "Insert failed")
      }
    } catch (e) {
      setInsertMsg(e instanceof Error ? e.message : "Insert failed")
    } finally {
      setInserting(false)
    }
  }

  const collectionOptions = collections.map((c) => c.name)

  return (
    <div className="space-y-6">
      {/* Collections */}
      <div>
        <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Collections
        </h2>
        {loading ? (
          <p className="text-sm text-muted-foreground">Loading...</p>
        ) : error && collections.length === 0 ? (
          <p className="text-sm text-muted-foreground">{error}</p>
        ) : collections.length === 0 ? (
          <p className="text-sm text-muted-foreground">No collections yet.</p>
        ) : (
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {collections.map((c) => (
              <Card key={c.name}>
                <CardContent className="py-4">
                  <div className="flex items-center justify-between">
                    <span className="font-medium text-foreground">{c.name}</span>
                    <Badge variant="outline" className="text-[10px]">
                      dim {c.dimension}
                    </Badge>
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    {c.point_count.toLocaleString()} points
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        )}
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        {/* Create collection */}
        <Card>
          <CardHeader>
            <CardTitle>Create collection</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="vc-name">Name</Label>
              <Input
                id="vc-name"
                placeholder="my-collection"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="vc-dim">Dimension</Label>
              <Input
                id="vc-dim"
                type="number"
                placeholder="384"
                value={newDim}
                onChange={(e) => setNewDim(e.target.value)}
              />
            </div>
            <Button onClick={handleCreate} disabled={creating || !newName.trim()}>
              {creating ? "Creating..." : "Create"}
            </Button>
          </CardContent>
        </Card>

        {/* Insert document */}
        <Card>
          <CardHeader>
            <CardTitle>Insert document</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="vi-col">Collection</Label>
              <select
                id="vi-col"
                className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                value={insertCollection}
                onChange={(e) => setInsertCollection(e.target.value)}
              >
                {collectionOptions.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </div>
            <div className="space-y-2">
              <Label htmlFor="vi-text">Text</Label>
              <Textarea
                id="vi-text"
                placeholder="Document text to embed and store..."
                rows={3}
                value={insertText}
                onChange={(e) => setInsertText(e.target.value)}
              />
            </div>
            {insertMsg && (
              <p className={`text-xs ${insertMsg.startsWith("Inserted") ? "text-emerald-400" : "text-red-400"}`}>
                {insertMsg}
              </p>
            )}
            <Button onClick={handleInsert} disabled={inserting || !insertText.trim() || !insertCollection}>
              {inserting ? "Inserting..." : "Insert"}
            </Button>
          </CardContent>
        </Card>
      </div>

      {/* Search */}
      <Card>
        <CardHeader>
          <CardTitle>Semantic search</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col gap-3 sm:flex-row">
            <select
              className="flex h-10 rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring sm:w-48"
              value={searchCollection}
              onChange={(e) => setSearchCollection(e.target.value)}
            >
              {collectionOptions.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
            <Input
              placeholder="Search query..."
              className="flex-1"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSearch()}
            />
            <Button onClick={handleSearch} disabled={searching || !searchQuery.trim() || !searchCollection}>
              {searching ? "Searching..." : "Search"}
            </Button>
          </div>

          {searchResults.length > 0 && (
            <div className="space-y-2">
              {searchResults.map((r, i) => (
                <div
                  key={r.id ?? i}
                  className="rounded-lg border border-border p-3 text-sm"
                >
                  <div className="flex items-center justify-between gap-2">
                    <code className="text-xs text-primary">{r.id.slice(0, 16)}</code>
                    <Badge variant="outline" className="text-[10px]">
                      score {r.score.toFixed(4)}
                    </Badge>
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">{r.text}</p>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
