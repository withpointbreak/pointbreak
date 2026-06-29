// Shared constants and type definitions for the inspector front-end.
// Ported from the served app.js constants/types cluster. This module is the
// shared type-definition home; later modules import from it.

/** An event type's display metadata: stable id, short label, and CSS palette colour. */
export interface EventType {
  id: string;
  label: string;
  color: string;
}

// Event types in canonical (timeline) order. Colours are CSS var() references so
// the palette themes from one place rather than hard-coded hex here.
export const TYPES: readonly EventType[] = [
  { id: "review_initialized", label: "init", color: "var(--evt-init)" },
  { id: "work_object_proposed", label: "capture", color: "var(--evt-capture)" },
  {
    id: "review_observation_recorded",
    label: "observation",
    color: "var(--evt-observation)",
  },
  {
    id: "review_assessment_recorded",
    label: "assessment",
    color: "var(--evt-assessment)",
  },
  { id: "input_request_opened", label: "request", color: "var(--evt-request)" },
  {
    id: "input_request_responded",
    label: "response",
    color: "var(--evt-response)",
  },
  { id: "review_note_imported", label: "note", color: "var(--evt-note)" },
  {
    id: "validation_check_recorded",
    label: "validation",
    color: "var(--evt-validation)",
  },
];

/** Event types indexed by id; unknown ids resolve to `undefined`. */
export const TYPE_MAP: Record<string, EventType | undefined> =
  Object.fromEntries(TYPES.map((type): [string, EventType] => [type.id, type]));

/** Resolve an event type's palette colour, falling back to the note colour. */
export function typeColor(id: string): string {
  return TYPE_MAP[id]?.color ?? "var(--evt-note)";
}

/** Resolve an event type's label, falling back to the raw id. */
export function typeLabel(id: string): string {
  return TYPE_MAP[id]?.label ?? id;
}

// Advisory signature/endorsement readback labels (reader-relative, never a gate).
export const VERIFICATION_LABELS: Record<string, string> = {
  valid: "signature valid",
  invalid: "signature invalid",
  untrusted_key: "untrusted key",
  unsigned: "unsigned",
};

export const ENDORSEMENT_LABELS: Record<string, string> = {
  "endorsement-trusted": "trusted endorsement",
  unknown_endorser: "unknown endorser",
  ambiguous_endorser: "ambiguous endorser",
};

export const ASSESSMENT_LABELS: Record<string, string> = {
  accepted: "accepted",
  accepted_with_follow_up: "accepted-with-follow-up",
  needs_changes: "needs-changes",
  needs_clarification: "needs-clarification",
};

// The master-pane projections, serialized into the URL fragment by the router.
export const LENSES: readonly string[] = ["timeline", "list", "threads"];
export const DEFAULT_LENS = "timeline";

// The structured-query field:value grammar fields.
export const QUERY_FIELDS: readonly string[] = [
  "type",
  "track",
  "revision",
  "object",
  "status",
  "attention",
];

// How many diff file bodies render eagerly; the rest stay collapsed until opened.
export const DEFAULT_OPEN_FILES = 10;
// A file body over this many rows is treated as large/generated and collapsed.
export const LARGE_FILE_ROWS = 500;

// Overlay name → root selector, for the single-overlay manager.
export const OVERLAY_SELECTORS: Record<string, string> = {
  diff: "#diff-modal",
  palette: "#cmd-palette",
  help: "#key-help",
};

// Review facts whose currency depends on the revision they target: a fact on a
// superseded revision is stale (named by all superseding successors).
export const SUPERSEDABLE_FACT_TYPES: ReadonlySet<string> = new Set([
  "review_observation_recorded",
  "review_assessment_recorded",
  "input_request_opened",
  "validation_check_recorded",
]);
