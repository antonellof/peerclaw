import { memo, useMemo } from "react"
import { Streamdown } from "streamdown"

import { cn } from "@/lib/utils"

import { segmentToolActionBlocks } from "@/pages/chat/segmentToolActionTags"

export type ChatMessageMarkdownProps = {
  content: string
  /** True while tokens are still streaming into this message (Vercel AI / Streamdown-style incomplete MD). */
  isAnimating?: boolean
  className?: string
}

const shellClass = (className?: string) =>
  cn(
    "min-w-0 overflow-x-auto",
    "[&_.streamdown]:text-foreground",
    "[&_a]:break-words [&_a]:text-primary [&_a]:underline-offset-2 hover:[&_a]:underline",
    "[&_blockquote]:border-l-2 [&_blockquote]:border-primary/40 [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground",
    "[&_h1]:mb-2 [&_h1]:mt-4 [&_h1]:text-base [&_h1]:font-semibold [&_h1]:first:mt-0",
    "[&_h2]:mb-2 [&_h2]:mt-3 [&_h2]:text-[15px] [&_h2]:font-semibold [&_h2]:first:mt-0",
    "[&_h3]:mb-1 [&_h3]:mt-2 [&_h3]:text-sm [&_h3]:font-semibold",
    "[&_hr]:my-4 [&_hr]:border-border",
    "[&_li]:my-0.5 [&_ol]:my-2 [&_ol]:list-decimal [&_ol]:pl-5 [&_ul]:my-2 [&_ul]:list-disc [&_ul]:pl-5",
    "[&_p]:my-2 [&_p]:first:mt-0 [&_p]:last:mb-0",
    "[&_pre]:my-3 [&_pre]:max-h-[min(70vh,480px)] [&_pre]:overflow-auto [&_pre]:rounded-lg",
    "[&_table]:my-3 [&_table]:block [&_table]:max-w-full [&_table]:overflow-x-auto",
    "[&_thead]:bg-muted/50",
    "[&_th]:border [&_th]:border-border [&_th]:px-2 [&_th]:py-1.5 [&_th]:text-left [&_th]:text-xs [&_th]:font-medium",
    "[&_td]:border [&_td]:border-border/80 [&_td]:px-2 [&_td]:py-1.5 [&_td]:text-xs",
    className,
  )

function ToolActionBlock({ tag, raw }: { tag: string; raw: string }) {
  return (
    <div
      className="my-2 overflow-hidden rounded-lg border border-primary/20 bg-gradient-to-br from-primary/[0.07] to-muted/25 shadow-sm"
      role="group"
      aria-label={`Tool action ${tag}`}
    >
      <div className="flex items-center gap-2 border-b border-border/50 bg-muted/50 px-2.5 py-1.5">
        <span className="rounded-md bg-primary/15 px-2 py-0.5 font-mono text-[10px] font-semibold uppercase tracking-wide text-primary">
          {tag}
        </span>
        <span className="text-[10px] text-muted-foreground">Action</span>
      </div>
      <pre className="max-h-[min(40vh,320px)] overflow-auto whitespace-pre-wrap break-words p-2.5 font-mono text-[11px] leading-snug text-muted-foreground">
        {raw.trim()}
      </pre>
    </div>
  )
}

/**
 * Renders assistant markdown with Streamdown (GFM, Shiki code blocks, streaming-safe parsing).
 * Detects blocks like `<web_fetch>…</web_fetch>` and `<tool_call>…</tool_call>` and shows them as cards.
 * @see https://github.com/vercel/streamdown
 */
export const ChatMessageMarkdown = memo(function ChatMessageMarkdown({
  content,
  isAnimating = false,
  className,
}: ChatMessageMarkdownProps) {
  const streaming = Boolean(isAnimating)

  const segments = useMemo(() => segmentToolActionBlocks(content), [content])

  const streamClass = "streamdown-chat text-[13px] leading-relaxed"

  if (segments.length === 1 && segments[0].type === "markdown") {
    return (
      <div className={shellClass(className)}>
        <Streamdown
          mode={streaming ? "streaming" : "static"}
          isAnimating={streaming}
          parseIncompleteMarkdown={streaming}
          shikiTheme={["github-dark", "github-dark"]}
          lineNumbers={false}
          className={streamClass}
        >
          {segments[0].text || "\u00a0"}
        </Streamdown>
      </div>
    )
  }

  return (
    <div className={shellClass(className)}>
      <div className="space-y-1">
        {segments.map((seg, i) =>
          seg.type === "markdown" ? (
            seg.text.trim() === "" ? null : (
              <Streamdown
                key={`md-${i}`}
                mode={streaming ? "streaming" : "static"}
                isAnimating={streaming}
                parseIncompleteMarkdown={streaming}
                shikiTheme={["github-dark", "github-dark"]}
                lineNumbers={false}
                className={streamClass}
              >
                {seg.text}
              </Streamdown>
            )
          ) : (
            <ToolActionBlock key={`act-${i}-${seg.tag}`} tag={seg.tag} raw={seg.raw} />
          ),
        )}
      </div>
    </div>
  )
})
