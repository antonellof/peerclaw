import { memo } from "react"
import { Streamdown } from "streamdown"

import { cn } from "@/lib/utils"

export type ChatMessageMarkdownProps = {
  content: string
  /** True while tokens are still streaming into this message (Vercel AI / Streamdown-style incomplete MD). */
  isAnimating?: boolean
  className?: string
}

/**
 * Renders assistant markdown with Streamdown (GFM, Shiki code blocks, streaming-safe parsing).
 * @see https://github.com/vercel/streamdown
 */
export const ChatMessageMarkdown = memo(function ChatMessageMarkdown({
  content,
  isAnimating = false,
  className,
}: ChatMessageMarkdownProps) {
  const streaming = Boolean(isAnimating)

  return (
    <div
      className={cn(
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
      )}
    >
      <Streamdown
        mode={streaming ? "streaming" : "static"}
        isAnimating={streaming}
        parseIncompleteMarkdown={streaming}
        shikiTheme={["github-dark", "github-dark"]}
        lineNumbers={false}
        className="streamdown-chat text-[13px] leading-relaxed"
      >
        {content || "\u00a0"}
      </Streamdown>
    </div>
  )
})
