export type ChangeInspectorAccess = "derived" | "authoritative";
const AVAILABILITIES = [
  "absent",
  "bootstrapping",
  "current",
  "catching_up",
  "rebuild_required",
  "quarantined",
  "unavailable",
] as const;
const NAMESPACES = ["absent", "stable", "legacy", "conflict"] as const;
const PHASES = [
  "cursor_population",
  "projection_population",
  "strict_verification",
  "finalizing",
] as const;
const ACTION_VALUES = [
  "wait",
  "authoritative_fallback",
  "cancel",
  "retry",
] as const;
export type ChangeRecoveryAction = (typeof ACTION_VALUES)[number];

export interface ChangeRecoveryStatus {
  schema: "pointbreak.inspect-derived-access-status";
  version: 1;
  active: boolean;
  availability: (typeof AVAILABILITIES)[number];
  namespace: (typeof NAMESPACES)[number];
  generationId?: string;
  phase?: (typeof PHASES)[number];
  completedEvents?: number;
  totalEvents?: number;
  completedBytes?: number;
  elapsedMilliseconds?: number;
  etaMilliseconds?: number;
  detail?: string;
  rebuildInFlight: boolean;
  rebuildPaused: boolean;
  servingCurrent: boolean;
  fallbackInFlight: boolean;
  actions: ChangeRecoveryAction[];
}

export interface ChangeRecoveryFailure {
  kind: "projection" | "page" | "selection" | "authority" | "capability";
  schema: string;
  code: string;
  message: string;
  retryable: boolean;
  status: number;
}

const STATUS_VALUES = {
  availability: new Set<string>(AVAILABILITIES),
  namespace: new Set<string>(NAMESPACES),
  phase: new Set<string>(PHASES),
};
const ACTIONS = new Set<string>(ACTION_VALUES);
const OPTIONAL_COUNTS = [
  "completedEvents",
  "totalEvents",
  "completedBytes",
  "elapsedMilliseconds",
  "etaMilliseconds",
] as const;

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : null;
}

export function decodeChangeRecoveryStatus(
  value: unknown,
): ChangeRecoveryStatus | null {
  const doc = record(value);
  if (
    doc === null ||
    doc.schema !== "pointbreak.inspect-derived-access-status" ||
    doc.version !== 1 ||
    typeof doc.active !== "boolean" ||
    typeof doc.availability !== "string" ||
    !STATUS_VALUES.availability.has(doc.availability) ||
    typeof doc.namespace !== "string" ||
    !STATUS_VALUES.namespace.has(doc.namespace) ||
    (doc.phase !== undefined &&
      (typeof doc.phase !== "string" || !STATUS_VALUES.phase.has(doc.phase))) ||
    (doc.generationId !== undefined && typeof doc.generationId !== "string") ||
    (doc.detail !== undefined && typeof doc.detail !== "string") ||
    [
      "rebuildInFlight",
      "rebuildPaused",
      "servingCurrent",
      "fallbackInFlight",
    ].some((key) => typeof doc[key] !== "boolean") ||
    OPTIONAL_COUNTS.some(
      (key) =>
        doc[key] !== undefined &&
        (typeof doc[key] !== "number" ||
          !Number.isFinite(doc[key]) ||
          doc[key] < 0),
    ) ||
    !Array.isArray(doc.actions)
  )
    return null;
  const decoded = {
    ...doc,
    actions: doc.actions.filter(
      (action): action is ChangeRecoveryAction =>
        typeof action === "string" && ACTIONS.has(action),
    ),
  } as unknown as ChangeRecoveryStatus;
  for (const key of ["phase", "generationId", "detail", ...OPTIONAL_COUNTS])
    if (decoded[key as keyof ChangeRecoveryStatus] === undefined)
      delete (decoded as unknown as Record<string, unknown>)[key];
  return decoded;
}

type FailureRule = readonly [
  kind: ChangeRecoveryFailure["kind"],
  status: number,
  retryable: boolean,
  codes: ReadonlySet<string>,
];
const PROJECTION_CODES = new Set([
  "projection_absent",
  "projection_rebuild_required",
  "projection_stale",
  "projection_invalid",
  "projection_unstable",
]);
const PAGE_CODES = new Set(["invalid_query", "stale_projection"]);
const HISTORY_CODES = new Set([...PAGE_CODES, "moving_journal"]);
function failureRule(schema: string): FailureRule | null {
  if (schema === "pointbreak.inspect-change-projection-error")
    return ["projection", 503, true, PROJECTION_CODES];
  if (schema === "pointbreak.inspect-change-page-error")
    return ["page", 0, true, PAGE_CODES];
  if (schema === "pointbreak.inspect-event-history-error")
    return ["page", 0, true, HISTORY_CODES];
  if (schema === "pointbreak.inspect-change-selection-error")
    return ["selection", 400, false, new Set(["invalid_exact_selection"])];
  if (schema === "pointbreak.inspect-change-authority-error")
    return [
      "authority",
      409,
      false,
      new Set(["authority_conflicted", "authority_invalid"]),
    ];
  if (schema === "pointbreak.reader-upgrade-required")
    return ["capability", 426, false, new Set(["reader_upgrade_required"])];
  return null;
}
const PAGE_STATUSES = new Map([
  ["invalid_query", 400],
  ["stale_projection", 409],
  ["moving_journal", 503],
]);

export function decodeChangeRecoveryFailure(
  value: unknown,
  status: number,
): ChangeRecoveryFailure | null {
  const doc = record(value);
  if (doc === null || doc.version !== 1 || typeof doc.schema !== "string")
    return null;
  const migrationState =
    doc.schema === "pointbreak.store-migration-required"
      ? "migration_required"
      : doc.schema === "pointbreak.store-migration-in-progress"
        ? "migration_in_progress"
        : null;
  if (status === 409 && migrationState !== null && doc.state === migrationState)
    return {
      kind: "capability",
      schema: doc.schema,
      code: migrationState,
      message: `Store migration ${doc.state === "migration_in_progress" ? "is in progress" : "is required"}`,
      retryable: false,
      status,
    };
  const rule = failureRule(doc.schema);
  if (rule === null || typeof doc.code !== "string") return null;
  const [kind, requiredStatus, needsRetryable, codes] = rule;
  if (
    (requiredStatus !== 0 && requiredStatus !== status) ||
    !codes.has(doc.code) ||
    typeof doc.message !== "string" ||
    (needsRetryable && typeof doc.retryable !== "boolean")
  )
    return null;
  if (kind === "page" && PAGE_STATUSES.get(doc.code) !== status) return null;
  return {
    kind,
    schema: doc.schema,
    code: doc.code,
    message: doc.message,
    retryable: doc.retryable === true,
    status,
  };
}
