import { forwardRef } from "react"
import { Copy } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"

/** Mesh onboarding: serve commands to join the P2P network. */
export const JoinMeshSection = forwardRef<HTMLElement>(function JoinMeshSection(_, ref) {
  const cmd =
    "peerclaw serve --web 127.0.0.1:8080 --share-inference"

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
          Share inference capacity and earn PCLAW tokens. Your node advertises models to the network and peers can request inference.
        </p>
      </div>

      <Card className="border-primary/20 bg-gradient-to-br from-primary/5 to-transparent">
        <CardHeader>
          <CardTitle className="text-base">Start a contributing node</CardTitle>
          <CardDescription>
            Run this command to start sharing inference with the P2P network.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <pre className="overflow-x-auto rounded-md border border-border/80 bg-muted/40 p-3 text-[11px] leading-relaxed text-foreground">
            {cmd}
          </pre>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="gap-2"
            onClick={() => void navigator.clipboard.writeText(cmd)}
          >
            <Copy className="size-3.5" />
            Copy command
          </Button>
        </CardContent>
      </Card>
    </section>
  )
})
