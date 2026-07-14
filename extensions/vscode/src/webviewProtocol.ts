import type { DiffRenderData } from "./diffDataSource";

export interface ReviewPanelFocus {
  readonly kind: "attention";
  readonly id: string;
}

export interface SnapshotRangeTarget {
  readonly filePath: string;
  readonly side: "old" | "new";
  readonly startLine: number;
  readonly endLine: number;
}

export type HostToWebview =
  | {
      readonly type: "render";
      readonly data: DiffRenderData;
      readonly focus?: ReviewPanelFocus;
    }
  | { readonly type: "focus"; readonly focus?: ReviewPanelFocus }
  | { readonly type: "error"; readonly message: string }
  | { readonly type: "freshness"; readonly changed: boolean };

export type WebviewToHost =
  | { readonly type: "ready" }
  | { readonly type: "openSource"; readonly target: SnapshotRangeTarget }
  | { readonly type: "reload" };

export function isHostToWebview(message: unknown): message is HostToWebview {
  if (!isRecord(message) || typeof message.type !== "string") {
    return false;
  }
  switch (message.type) {
    case "render":
      return (
        hasOnlyKeys(message, ["type", "data", "focus"]) &&
        isDiffRenderData(message.data) &&
        isOptionalFocus(message.focus)
      );
    case "focus":
      return (
        hasOnlyKeys(message, ["type", "focus"]) &&
        isOptionalFocus(message.focus)
      );
    case "error":
      return (
        hasOnlyKeys(message, ["type", "message"]) &&
        typeof message.message === "string"
      );
    case "freshness":
      return (
        hasOnlyKeys(message, ["type", "changed"]) &&
        typeof message.changed === "boolean"
      );
    default:
      return false;
  }
}

export function isWebviewToHost(message: unknown): message is WebviewToHost {
  if (!isRecord(message) || typeof message.type !== "string") {
    return false;
  }
  switch (message.type) {
    case "ready":
    case "reload":
      return hasOnlyKeys(message, ["type"]);
    case "openSource":
      return (
        hasOnlyKeys(message, ["type", "target"]) &&
        isSnapshotRangeTarget(message.target)
      );
    default:
      return false;
  }
}

function isOptionalFocus(
  value: unknown,
): value is ReviewPanelFocus | undefined {
  return (
    value === undefined ||
    (isRecord(value) &&
      hasOnlyKeys(value, ["kind", "id"]) &&
      value.kind === "attention" &&
      typeof value.id === "string" &&
      value.id.length > 0)
  );
}

function isDiffRenderData(value: unknown): value is DiffRenderData {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, [
      "revisionId",
      "snapshotId",
      "artifact",
      "annotations",
    ]) ||
    typeof value.revisionId !== "string" ||
    !value.revisionId ||
    typeof value.snapshotId !== "string" ||
    !value.snapshotId ||
    !isSafeArtifact(value.artifact) ||
    !Array.isArray(value.annotations)
  ) {
    return false;
  }
  return value.annotations.every(isSafeAnnotation);
}

function isSafeArtifact(value: unknown): boolean {
  if (
    !isRecord(value) ||
    value.schema !== "pointbreak.review-snapshot" ||
    value.version !== 1 ||
    typeof value.contentHash !== "string" ||
    !isRecord(value.snapshot) ||
    !Array.isArray(value.snapshot.files)
  ) {
    return false;
  }
  return value.snapshot.files.every((file) => {
    if (!isRecord(file)) return false;
    return [file.old_path, file.new_path].every(
      (path) => path === undefined || path === null || isRelativePath(path),
    );
  });
}

function isSafeAnnotation(value: unknown): boolean {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, [
      "id",
      "kind",
      "title",
      "track",
      "body",
      "bodyContentType",
      "tags",
      "target",
    ]) ||
    typeof value.id !== "string" ||
    typeof value.kind !== "string" ||
    typeof value.title !== "string" ||
    typeof value.track !== "string"
  ) {
    return false;
  }
  if (
    (value.body !== undefined && typeof value.body !== "string") ||
    (value.bodyContentType !== undefined &&
      typeof value.bodyContentType !== "string")
  ) {
    return false;
  }
  if (value.tags !== undefined) {
    if (!Array.isArray(value.tags) || !value.tags.every(isString)) return false;
  }
  if (value.target !== undefined) {
    if (
      !isRecord(value.target) ||
      !hasOnlyKeys(value.target, [
        "kind",
        "revisionId",
        "filePath",
        "side",
        "startLine",
        "endLine",
      ])
    ) {
      return false;
    }
    if (
      value.target.filePath !== undefined &&
      !isRelativePath(value.target.filePath)
    ) {
      return false;
    }
    if (
      [value.target.kind, value.target.revisionId, value.target.side].some(
        (field) => field !== undefined && typeof field !== "string",
      ) ||
      [value.target.startLine, value.target.endLine].some(
        (line) => line !== undefined && !isPositiveInteger(line),
      )
    ) {
      return false;
    }
  }
  return true;
}

function isSnapshotRangeTarget(value: unknown): value is SnapshotRangeTarget {
  return (
    isRecord(value) &&
    hasOnlyKeys(value, ["filePath", "side", "startLine", "endLine"]) &&
    isRelativePath(value.filePath) &&
    (value.side === "old" || value.side === "new") &&
    isPositiveInteger(value.startLine) &&
    isPositiveInteger(value.endLine) &&
    value.endLine >= value.startLine
  );
}

function isRelativePath(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    !value.startsWith("/") &&
    !value.startsWith("\\") &&
    !/^[a-zA-Z]:/.test(value) &&
    !value.includes("\\") &&
    !value.split("/").includes("..")
  );
}

function isPositiveInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value > 0;
}

function isString(value: unknown): value is string {
  return typeof value === "string";
}

function hasOnlyKeys(
  value: Record<string, unknown>,
  keys: readonly string[],
): boolean {
  const allowed = new Set(keys);
  return Object.keys(value).every((key) => allowed.has(key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
