import * as d3 from "d3"
import { useEffect, useRef } from "react"

import { cn } from "@/lib/utils"

export type FgNode = { id: string; label?: string; type?: string }
export type FgLink = { source: string; target: string }

type Props = {
  nodes: FgNode[]
  links: FgLink[]
  height: number
  className?: string
  variant?: "network" | "swarm"
  onNodeClick?: (id: string) => void
}

/** D3 force-directed graph (parity with embedded dashboard). */
export function ForceGraph({ nodes, links, height, className, variant = "network", onNodeClick }: Props) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = ref.current
    if (!el) return

    const width = el.clientWidth || 600
    el.innerHTML = ""

    if (!nodes.length) {
      const empty = document.createElement("div")
      empty.className = "flex h-full items-center justify-center text-sm text-muted-foreground"
      empty.textContent = "No nodes to display"
      el.appendChild(empty)
      return
    }

    const svg = d3.select(el).append("svg").attr("width", width).attr("height", height)

    const simNodes = nodes.map((n) => ({ ...n }))
    const simLinks = links.map((l) => ({ ...l }))

    const linkForce = d3
      .forceLink<FgNode & d3.SimulationNodeDatum, FgLink>(simLinks)
      .id((d) => d.id)
      .distance(variant === "swarm" ? 80 : 120)

    const simulation = d3
      .forceSimulation(simNodes as (FgNode & d3.SimulationNodeDatum)[])
      .force("link", linkForce)
      .force("charge", d3.forceManyBody().strength(variant === "swarm" ? -180 : -300))
      .force("center", d3.forceCenter(width / 2, height / 2))
      .force("collision", d3.forceCollide().radius(variant === "swarm" ? 30 : 45))

    const link = svg
      .append("g")
      .attr("stroke", "hsl(var(--border))")
      .attr("stroke-opacity", 0.85)
      .selectAll("line")
      .data(simLinks)
      .join("line")

    const drag = d3
      .drag<SVGGElement, FgNode & d3.SimulationNodeDatum>()
      .on("start", (event, d) => {
        if (!event.active) simulation.alphaTarget(0.3).restart()
        d.fx = d.x
        d.fy = d.y
      })
      .on("drag", (event, d) => {
        d.fx = event.x
        d.fy = event.y
      })
      .on("end", (event, d) => {
        if (!event.active) simulation.alphaTarget(0)
        d.fx = null
        d.fy = null
      })

    const node = svg
      .append("g")
      .selectAll<SVGGElement, FgNode & d3.SimulationNodeDatum>("g")
      .data(simNodes as (FgNode & d3.SimulationNodeDatum)[])
      .join("g")
      .style("cursor", onNodeClick ? "pointer" : "grab")
      .call(drag)

    const isLocal = (d: FgNode) => d.type === "local"
    const r = (d: FgNode) => (variant === "network" ? (isLocal(d) ? 22 : 16) : isLocal(d) ? 20 : 14)

    node
      .append("circle")
      .attr("r", (d) => r(d))
      .attr("fill", (d) => (isLocal(d) ? "hsl(var(--primary))" : "hsl(var(--accent))"))
      .attr("stroke", "hsl(var(--border))")
      .attr("stroke-width", 1.5)
      .on("click", (e, d) => {
        e.stopPropagation()
        onNodeClick?.(d.id)
      })

    node
      .append("text")
      .attr("text-anchor", "middle")
      .attr("dy", (d) => r(d) + 14)
      .attr("fill", "hsl(var(--muted-foreground))")
      .attr("font-size", 11)
      .text((d) => d.label ?? d.id.slice(0, 8))

    simulation.on("tick", () => {
      link
        .attr("x1", (d) => (d.source as d3.SimulationNodeDatum).x!)
        .attr("y1", (d) => (d.source as d3.SimulationNodeDatum).y!)
        .attr("x2", (d) => (d.target as d3.SimulationNodeDatum).x!)
        .attr("y2", (d) => (d.target as d3.SimulationNodeDatum).y!)

      node.attr("transform", (d) => `translate(${d.x},${d.y})`)
    })

    return () => {
      simulation.stop()
    }
  }, [nodes, links, height, variant, onNodeClick])

  return <div ref={ref} className={cn("w-full", className)} style={{ height }} />
}
