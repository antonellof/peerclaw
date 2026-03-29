/** IDs match `src/flow/interpreter.rs` guardrails arm. */
export const GUARD_OPTIONS: { id: string; label: string }[] = [
  { id: "pii", label: "Personally identifiable information (PII)" },
  { id: "moderation", label: "Moderation (policy)" },
  { id: "jailbreak", label: "Jailbreak / injection patterns" },
  { id: "hallucination", label: "Hallucination (policy block)" },
  { id: "nsfw", label: "NSFW / content policy" },
  { id: "url", label: "URL filter (SSRF-style host checks)" },
  { id: "injection", label: "Prompt injection detection" },
  { id: "policy", label: "Custom policy rules" },
  { id: "custom", label: "Custom substring block (see field below)" },
]

export function guardrailSetFromStr(s: string | undefined): Set<string> {
  return new Set(
    (s ?? "")
      .split(",")
      .map((x) => x.trim().toLowerCase())
      .filter(Boolean),
  )
}

export function stringifyGuardSet(set: Set<string>): string {
  return GUARD_OPTIONS.map((o) => o.id).filter((id) => set.has(id)).join(", ")
}
