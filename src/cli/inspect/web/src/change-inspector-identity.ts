/** Strict path-private repository identity used only by Inspector chrome. */

export interface InspectorIdentity {
  schema: "pointbreak.inspect-identity";
  storeIdentity: string;
  contextIdentity: string;
  repository: string;
  worktree?: string;
  placement: {
    tier: "clone" | "family" | "ephemeral";
    label: "clone store" | "family store" | "ephemeral store";
  };
  family?: { id: string };
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function hasOnlyKeys(
  value: Record<string, unknown>,
  allowed: readonly string[],
): boolean {
  const keys = new Set(allowed);
  return Object.keys(value).every((key) => keys.has(key));
}

function nonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function basename(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    !value.includes("/") &&
    !value.includes("\\")
  );
}

function familySlug(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length <= 64 &&
    /^[a-z0-9-]+$/.test(value)
  );
}

export function decodeInspectorIdentity(value: unknown): InspectorIdentity {
  const document = record(value);
  const placement = record(document?.placement);
  const family =
    document?.family === undefined ? undefined : record(document.family);
  const tier = placement?.tier;
  const expectedLabel =
    tier === "clone"
      ? "clone store"
      : tier === "family"
        ? "family store"
        : tier === "ephemeral"
          ? "ephemeral store"
          : null;
  if (
    document === null ||
    !hasOnlyKeys(document, [
      "schema",
      "storeIdentity",
      "contextIdentity",
      "repository",
      "worktree",
      "placement",
      "family",
    ]) ||
    document.schema !== "pointbreak.inspect-identity" ||
    !nonEmptyString(document.storeIdentity) ||
    !nonEmptyString(document.contextIdentity) ||
    !basename(document.repository) ||
    (document.worktree !== undefined && !basename(document.worktree)) ||
    placement === null ||
    !hasOnlyKeys(placement, ["tier", "label"]) ||
    expectedLabel === null ||
    placement.label !== expectedLabel ||
    (family !== undefined &&
      (family === null ||
        !hasOnlyKeys(family, ["id"]) ||
        !familySlug(family.id))) ||
    (tier === "family") !== (family !== undefined)
  ) {
    throw new Error("invalid Inspector identity DTO");
  }
  return {
    schema: "pointbreak.inspect-identity",
    storeIdentity: document.storeIdentity,
    contextIdentity: document.contextIdentity,
    repository: document.repository,
    ...(document.worktree === undefined ? {} : { worktree: document.worktree }),
    placement: {
      tier: tier as InspectorIdentity["placement"]["tier"],
      label: expectedLabel,
    },
    ...(family === undefined ? {} : { family: { id: family.id as string } }),
  };
}
