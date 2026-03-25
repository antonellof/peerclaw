---
name: research
version: 0.1.0
description: Use tools for real data — web pages, timetables, venues — not placeholders
---

## When this skill applies

Chat agent **task type** is `research`, or the user wants facts, travel, comparisons, or anything that could change over time.

## Tool discipline

1. **Only** `<tool_call>` blocks run tools. Tags like `<json …>`, `<file_list>`, or `<wallet_balance>` are **ignored** — never use them as shortcuts.
2. For structured JSON work, call the **`json`** tool via `<tool_call>` with `name: json` and `args: { "action": "…", "input": "…" }`.
3. For web content, call **`web_fetch`** with a real `https://` URL (rail operator, official museum site, maps, etc.). Do not invent URLs, prices, or opening hours.
4. If the first fetch fails, try another **real** source or say what you could not verify — do not fabricate.

## Output

Cite or summarize what came from tools. Separate **verified** (from a fetched page) from **general knowledge** (no fetch).
