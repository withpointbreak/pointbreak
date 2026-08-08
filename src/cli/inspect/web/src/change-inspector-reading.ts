/**
 * Exact Inspector reading controller.
 *
 * Each reading surface keeps its own identity: a captured resource is bound to
 * one RevisionRef, an association is a Git comparison about that RevisionRef,
 * and an interdiff is an ordered pair of RevisionRefs. These documents cannot
 * be exchanged or refreshed into one another. The caller must additionally
 * compare the returned projection stamp with its staged Change generation and
 * run the profile postflight before it paints a returned document.
 */

import { fetchChangeInspectorJSON } from "./change-inspector-http";
import type { ChangeInspectorRoute } from "./change-inspector-router";
import {
  type ChangeDetail,
  type ChangeRevisionDetail,
  decodeChangeDetail,
  decodeChangeRevisionDetail,
  decodeRevisionInterdiff,
  decodeRevisionResource,
  type RevisionInterdiff,
  type RevisionRef,
  type RevisionResource,
} from "./change-protocol";

export type ChangeInspectorReading =
  | { kind: "change"; document: ChangeDetail }
  | { kind: "revision"; document: ChangeRevisionDetail }
  | { kind: "association"; document: ChangeRevisionDetail }
  | { kind: "resource"; document: RevisionResource }
  | { kind: "interdiff"; document: RevisionInterdiff };

export function sameExactRevision(
  left: RevisionRef,
  right: RevisionRef,
): boolean {
  return (
    left.revisionId === right.revisionId &&
    left.objectArtifactContentHash === right.objectArtifactContentHash
  );
}

function encoded(value: string): string {
  return encodeURIComponent(value);
}

function revisionPath(changeId: string, revision: RevisionRef): string {
  return `/api/v2/changes/${encoded(changeId)}/revisions/${encoded(revision.revisionId)}?artifactHash=${encoded(revision.objectArtifactContentHash)}`;
}

function resourcePath(changeId: string, revision: RevisionRef): string {
  return `/api/v2/changes/${encoded(changeId)}/revisions/${encoded(revision.revisionId)}/resource?artifactHash=${encoded(revision.objectArtifactContentHash)}`;
}

function assertStamp(stamp: string, expected: string, surface: string): void {
  if (stamp !== expected) {
    throw new Error(
      `${surface} projection stamp does not match the staged Change generation`,
    );
  }
}

function assertRevisionDetail(
  document: ChangeRevisionDetail,
  route: Extract<ChangeInspectorRoute, { kind: "revision" | "association" }>,
  stamp: string,
): void {
  if (document.changeId !== route.changeId) {
    throw new Error(
      "contextual Revision detail Change ID does not match its exact route",
    );
  }
  if (!sameExactRevision(document.revision, route.revision)) {
    throw new Error(
      "contextual Revision detail does not match its exact route",
    );
  }
  if (
    !sameExactRevision(
      document.exactRevisionDocument.resource.revision,
      route.revision,
    )
  ) {
    throw new Error(
      "embedded captured resource does not match its exact Revision route",
    );
  }
  if (
    document.factPresentations.some(
      (fact) =>
        fact.contextChangeId !== route.changeId ||
        (fact.presentedInRevision !== undefined &&
          !sameExactRevision(fact.presentedInRevision, route.revision)),
    )
  ) {
    throw new Error(
      "fact presentation does not match its Change and exact Revision context",
    );
  }
  if (
    document.factPorts.some(
      (port) => !sameExactRevision(port.targetRevision, route.revision),
    )
  ) {
    throw new Error("fact port does not target the selected exact Revision");
  }
  if (
    document.associations.some(
      (association) =>
        !sameExactRevision(association.comparison.revision, route.revision),
    )
  ) {
    throw new Error(
      "association comparison does not target the selected exact Revision",
    );
  }
  if (
    document.exactRevisionDocument.projectionStamp !== document.projectionStamp
  ) {
    throw new Error(
      "embedded captured resource is from another projection stamp",
    );
  }
  assertStamp(document.projectionStamp, stamp, "contextual Revision detail");
}

/** Read one exact route, then reject any mismatch before a presenter sees it. */
export async function loadChangeInspectorReading(
  route: Exclude<ChangeInspectorRoute, { kind: "lens" | "invalid" }>,
  expectedProjectionStamp: string,
): Promise<ChangeInspectorReading> {
  if (route.kind === "change") {
    const document = decodeChangeDetail(
      await fetchChangeInspectorJSON(
        `/api/v2/changes/${encoded(route.changeId)}`,
      ),
    );
    if (document.summary.changeId !== route.changeId) {
      throw new Error("Change detail does not match its route");
    }
    assertStamp(
      document.projectionStamp,
      expectedProjectionStamp,
      "Change detail",
    );
    return { kind: "change", document };
  }
  if (route.kind === "revision" || route.kind === "association") {
    const document = decodeChangeRevisionDetail(
      await fetchChangeInspectorJSON(
        revisionPath(route.changeId, route.revision),
      ),
    );
    assertRevisionDetail(document, route, expectedProjectionStamp);
    return { kind: route.kind, document };
  }
  if (route.kind === "resource") {
    const document = decodeRevisionResource(
      await fetchChangeInspectorJSON(
        resourcePath(route.changeId, route.revision),
      ),
    );
    if (!sameExactRevision(document.resource.revision, route.revision)) {
      throw new Error(
        "captured resource does not match its exact Revision route",
      );
    }
    assertStamp(
      document.projectionStamp,
      expectedProjectionStamp,
      "captured resource",
    );
    return { kind: "resource", document };
  }
  const params = new URLSearchParams({
    fromArtifactHash: route.from.objectArtifactContentHash,
    toArtifactHash: route.to.objectArtifactContentHash,
  });
  const document = decodeRevisionInterdiff(
    await fetchChangeInspectorJSON(
      `/api/v2/changes/${encoded(route.changeId)}/interdiff/${encoded(route.from.revisionId)}/${encoded(route.to.revisionId)}?${params}`,
    ),
  );
  if (
    !sameExactRevision(document.interdiff.from, route.from) ||
    !sameExactRevision(document.interdiff.to, route.to)
  ) {
    throw new Error(
      "ordered Revision interdiff does not match its exact route",
    );
  }
  assertStamp(
    document.projectionStamp,
    expectedProjectionStamp,
    "Revision interdiff",
  );
  return { kind: "interdiff", document };
}
