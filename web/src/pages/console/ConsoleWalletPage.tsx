import { useEffect, useState, useCallback } from "react"

import {
  fetchWallet,
  fetchWalletTransactions,
  type WalletResponse,
  type WalletTransaction,
} from "@/lib/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

export function ConsoleWalletPage() {
  const [wallet, setWallet] = useState<WalletResponse | null>(null)
  const [txns, setTxns] = useState<WalletTransaction[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const [w, t] = await Promise.all([fetchWallet(), fetchWalletTransactions()])
      setWallet(w)
      setTxns(t)
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load wallet")
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
    const i = setInterval(load, 30000)
    return () => clearInterval(i)
  }, [load])

  const txTypeColor = (t: string) => {
    switch (t.toLowerCase()) {
      case "credit":
      case "reward":
      case "received":
        return "text-emerald-400"
      case "debit":
      case "sent":
      case "escrow":
        return "text-amber-400"
      default:
        return "text-muted-foreground"
    }
  }

  return (
    <div className="space-y-6">
      {/* Balance card */}
      <Card>
        <CardContent className="py-8">
          {loading ? (
            <p className="text-sm text-muted-foreground">Loading...</p>
          ) : error && !wallet ? (
            <p className="text-sm text-muted-foreground">{error}</p>
          ) : wallet ? (
            <div className="flex flex-col items-center gap-4 sm:flex-row sm:items-end sm:justify-between">
              <div>
                <div className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                  PCLAW Balance
                </div>
                <div className="mt-2 text-5xl font-bold tracking-tight text-foreground">
                  {wallet.balance.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 4 })}
                </div>
                {wallet.escrowed > 0 && (
                  <div className="mt-1 text-sm text-amber-400">
                    {wallet.escrowed.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 4 })}{" "}
                    in escrow
                  </div>
                )}
              </div>
              <div className="text-right text-xs text-muted-foreground">
                <div className="font-medium text-foreground">Wallet address</div>
                <code className="mt-1 block break-all rounded bg-muted px-2 py-1 font-mono text-[11px]">
                  {wallet.address}
                </code>
              </div>
            </div>
          ) : null}
        </CardContent>
      </Card>

      {/* Transaction history */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Transaction history</CardTitle>
          <span className="text-xs text-muted-foreground">{txns.length} transactions</span>
        </CardHeader>
        <CardContent>
          {txns.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              {loading ? "Loading..." : "No transactions yet."}
            </p>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border text-xs text-muted-foreground">
                    <th className="pb-2 text-left font-medium">Type</th>
                    <th className="pb-2 text-right font-medium">Amount</th>
                    <th className="pb-2 text-left font-medium">Peer</th>
                    <th className="pb-2 text-left font-medium">Description</th>
                    <th className="pb-2 text-right font-medium">Time</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/50">
                  {txns.map((tx) => (
                    <tr key={tx.id} className="text-foreground">
                      <td className="py-2.5">
                        <Badge variant="outline" className="text-[10px]">
                          {tx.tx_type}
                        </Badge>
                      </td>
                      <td className={`py-2.5 text-right font-mono ${txTypeColor(tx.tx_type)}`}>
                        {tx.amount >= 0 ? "+" : ""}
                        {tx.amount.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 4 })}
                      </td>
                      <td className="py-2.5">
                        {tx.peer ? (
                          <code className="text-xs text-muted-foreground">{tx.peer.slice(0, 12)}...</code>
                        ) : (
                          <span className="text-xs text-muted-foreground/50">--</span>
                        )}
                      </td>
                      <td className="py-2.5 text-xs text-muted-foreground">
                        {tx.description ?? "--"}
                      </td>
                      <td className="py-2.5 text-right text-xs text-muted-foreground">
                        {new Date(tx.timestamp).toLocaleString()}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
