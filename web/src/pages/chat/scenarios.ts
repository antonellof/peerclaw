/** Pre-fill Agent goal in chat (Home starters). */
export const SCENARIO_PRESETS: Record<string, { type: string; text: string }> = {
  trip: {
    type: "research",
    text: "Plan a 3-day weekend trip for 2 adults to [DESTINATION] with a [MODERATE/LOW/HIGH] budget. Include: (1) how to get there, (2) day-by-day itinerary with one backup indoor option, (3) three dining picks per day style (casual / nice / quick), (4) a short packing and booking checklist.",
  },
  email: {
    type: "general",
    text: "Draft a [formal / friendly] email to [RECIPIENT ROLE OR NAME] about [SUBJECT]. Include these points: [BULLET 1], [BULLET 2]. End with a clear ask and a subject line suggestion.",
  },
  research: {
    type: "research",
    text: "Research [TOPIC] for a work decision. Deliver: (1) 5-bullet executive summary, (2) pros/cons table, (3) what still needs verification with sources to check, (4) three recommended next actions with owners.",
  },
  data: {
    type: "analyze",
    text: "I will paste data below (table or metrics). Analyze it for a stakeholder update: key trends, anomalies, one chart you would draw (describe axes), and risks if we act vs wait. Data:\n\n[PASTE HERE]",
  },
  bugfix: {
    type: "code",
    text: "I have a bug: [SYMPTOMS]. Environment: [LANGUAGE / FRAMEWORK / OS]. Relevant code or stack trace:\n\n[PASTE HERE]\n\nPropose root-cause hypotheses, minimal fix, and a test to prevent regression.",
  },
  meeting: {
    type: "general",
    text: "Prepare me for a [DURATION] meeting about [TOPIC] with [ATTENDEES / ROLES]. Output: agenda with timeboxes, my talking points, likely objections with responses, and a follow-up email outline.",
  },
}
