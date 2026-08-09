/**
 * The Inspector's publication state machine is profile -> staged generation ->
 * route resolution -> paint. Nothing is painted from the staged documents:
 * profile postflight and shared projection stamps must agree before a generation
 * becomes visible. A Change selection stays stable across refreshes; a Revision
 * selection additionally requires the exact (Revision ID, artifact hash) pair.
 */

import type { InspectorIdentity } from "./change-inspector-identity";
import type { ChangeInspectorRoute } from "./change-inspector-router";
import {
  type AttentionPage,
  type ChangeSummary,
  type ChangesPage,
  type EventHistoryDocument,
  type ReaderProfile,
  type RevisionRef,
  requireCoherentGeneration,
  sameAuthorityCursor,
  sameProfileGeneration,
} from "./change-protocol";

export interface ChangeInspectorGeneration {
  profile: ReaderProfile;
  changes: ChangesPage;
  attention: AttentionPage;
  history: EventHistoryDocument | null;
}

export interface ChangeInspectorSnapshot {
  generation: ChangeInspectorGeneration | null;
  /** Session chrome state; older argument-built snapshots may omit it. */
  identity?: InspectorIdentity | null;
  route: ChangeInspectorRoute;
  selected: ChangeSummary | null;
  diagnostic: string | null;
}

export type PublicationTransition = "initial" | "unchanged" | "changed";

export interface ChangeInspectorPublication {
  snapshot: ChangeInspectorSnapshot;
  transition: PublicationTransition;
}

export class ChangeInspectorGenerationChanged extends Error {
  constructor() {
    super("Change generation changed during staging");
  }
}

function sameRevision(left: RevisionRef, right: RevisionRef): boolean {
  return (
    left.revisionId === right.revisionId &&
    left.objectArtifactContentHash === right.objectArtifactContentHash
  );
}

function selectedChange(
  generation: ChangeInspectorGeneration,
  route: ChangeInspectorRoute,
): ChangeSummary | null {
  if (route.kind !== "change" && route.kind !== "revision") return null;
  const all = [...generation.changes.changes, ...generation.attention.changes];
  const change =
    all.find((candidate) => candidate.changeId === route.changeId) ?? null;
  if (change === null || route.kind !== "revision") return change;
  return change.currentRevisionRefs.some((candidate) =>
    sameRevision(candidate, route.revision),
  )
    ? change
    : null;
}

function selectionDiagnostic(route: ChangeInspectorRoute): string | null {
  if (route.kind === "invalid") return route.message;
  // A bounded page and `currentRevisionRefs` cannot disprove contextual
  // membership: a selected Change may be off-page and an exact Revision may be
  // stale but still readable. The contextual exact-detail document is the
  // authoritative membership source.
  return null;
}

export function stageGeneration(
  profile: ReaderProfile,
  changes: ChangesPage,
  attention: AttentionPage,
  postflight: ReaderProfile,
  history: EventHistoryDocument | null = null,
): ChangeInspectorGeneration {
  requireCoherentGeneration(changes, attention);
  if (!sameProfileGeneration(profile, postflight)) {
    throw new ChangeInspectorGenerationChanged();
  }
  if (
    history !== null &&
    (history.sourceChangeProjectionStamp !== changes.projectionStamp ||
      !sameAuthorityCursor(history.authorityCursor, profile.authorityCursor) ||
      !sameAuthorityCursor(history.authorityCursor, postflight.authorityCursor))
  ) {
    throw new ChangeInspectorGenerationChanged();
  }
  return { profile, changes, attention, history };
}

export function createChangeInspectorState(initialRoute: ChangeInspectorRoute) {
  let generation: ChangeInspectorGeneration | null = null;
  let generationCredentialVersion: number | null = null;
  let identity: {
    value: InspectorIdentity;
    credentialVersion: number;
  } | null = null;
  let pendingIdentity: {
    value: InspectorIdentity;
    credentialVersion: number;
  } | null = null;
  let route = initialRoute;

  const snapshot = (): ChangeInspectorSnapshot => {
    const selected =
      generation === null ? null : selectedChange(generation, route);
    return {
      generation,
      identity: identity?.value ?? null,
      route,
      selected,
      diagnostic: selectionDiagnostic(route),
    };
  };

  return {
    snapshot,
    publish(
      next: ChangeInspectorGeneration,
      credentialVersion = 0,
    ): ChangeInspectorPublication {
      const transition: PublicationTransition =
        generation === null
          ? "initial"
          : sameProfileGeneration(generation.profile, next.profile) &&
              generation.changes.projectionStamp ===
                next.changes.projectionStamp &&
              generation.history?.timelineProjectionStamp ===
                next.history?.timelineProjectionStamp
            ? "unchanged"
            : "changed";
      generation = next;
      generationCredentialVersion = credentialVersion;
      if (identity?.credentialVersion !== credentialVersion) {
        identity =
          pendingIdentity?.credentialVersion === credentialVersion
            ? pendingIdentity
            : null;
      }
      if (pendingIdentity?.credentialVersion === credentialVersion) {
        pendingIdentity = null;
      }
      return { snapshot: snapshot(), transition };
    },
    publishIdentity(
      next: InspectorIdentity,
      credentialVersion = 0,
    ): ChangeInspectorSnapshot {
      const candidate = { value: next, credentialVersion };
      if (
        generation === null ||
        generationCredentialVersion === credentialVersion
      ) {
        identity = candidate;
      } else {
        pendingIdentity = candidate;
      }
      return snapshot();
    },
    setRoute(next: ChangeInspectorRoute): ChangeInspectorSnapshot {
      route = next;
      return snapshot();
    },
    clearGeneration(): ChangeInspectorSnapshot {
      generation = null;
      generationCredentialVersion = null;
      if (pendingIdentity !== null) {
        identity = pendingIdentity;
        pendingIdentity = null;
      }
      return snapshot();
    },
  };
}
