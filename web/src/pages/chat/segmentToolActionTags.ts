/**
 * Split assistant message text into markdown segments and pseudo-tool XML blocks
 * (`<tool_call>`, `<web_fetch>`, `<job_status>`, etc.) for richer UI rendering.
 */

export type ToolActionSegment =
  | { type: "markdown"; text: string }
  | { type: "action"; tag: string; raw: string }

/** Alternation order: more specific / longer patterns first. */
const ACTION_BLOCK_RE =
  /<tool_call>[\s\S]*?<\/tool_call>|<web_fetch>[\s\S]*?<\/web_fetch>|<web_search>[\s\S]*?<\/web_search>|<job_status\s[^>]*(?:\/>|>[\s\S]*?<\/job_status>)>/gi

export function tagFromActionRaw(raw: string): string {
  const m = raw.match(/^<\s*([a-zA-Z0-9_:-]+)/)
  return m?.[1] ?? "action"
}

/**
 * Split `source` into alternating markdown and tool/action blocks.
 * If nothing matches, returns a single markdown segment.
 */
export function segmentToolActionBlocks(source: string): ToolActionSegment[] {
  if (!source) return [{ type: "markdown", text: "" }]

  const segments: ToolActionSegment[] = []
  let last = 0
  const re = new RegExp(ACTION_BLOCK_RE.source, ACTION_BLOCK_RE.flags)
  let m: RegExpExecArray | null
  while ((m = re.exec(source)) !== null) {
    if (m.index > last) {
      segments.push({ type: "markdown", text: source.slice(last, m.index) })
    }
    const raw = m[0]
    segments.push({ type: "action", tag: tagFromActionRaw(raw), raw })
    last = m.index + raw.length
  }
  if (last < source.length) {
    segments.push({ type: "markdown", text: source.slice(last) })
  }

  return segments.length > 0 ? segments : [{ type: "markdown", text: source }]
}
