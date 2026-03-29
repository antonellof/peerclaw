import { SCENARIO_PRESETS } from "./scenarios"

/** Quick-fill bodies for chat input (same set as the former Agent panel). */
export const AGENT_TASK_TEMPLATES: Record<string, { taskType: string; text: string }> = {
  research: {
    taskType: "research",
    text: "Research [TOPIC] for someone new to the field. Output: (1) short overview, (2) key terms defined, (3) current debates, (4) 5 reputable sources or search queries to dig deeper.",
  },
  summarize: {
    taskType: "general",
    text: "Fetch and summarize this URL for a busy reader: [URL]. Include: main thesis, 5 takeaways, and anything that should be fact-checked.",
  },
  code: {
    taskType: "code",
    text: 'Review this code for correctness, edge cases, and performance. Suggest refactors and tests.\n\n```\n[PASTE CODE]\n```',
  },
  monitor: {
    taskType: "monitor",
    text: "Check this URL and report HTTP status, visible title, and whether the page looks like the expected content (describe what you see): [URL]",
  },
  analyze: {
    taskType: "analyze",
    text: "Analyze the following for decisions: trends, outliers, and one narrative paragraph for leadership.\n\n[PASTE DATA OR DESCRIPTION]",
  },
  automate: {
    taskType: "general",
    text: "Design automation for: [TASK]. Output: step-by-step shell or script outline, prerequisites, and how to verify it worked.",
  },
}

export { SCENARIO_PRESETS }
