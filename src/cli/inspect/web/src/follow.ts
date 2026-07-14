// Timeline streaming has three independent session-only concerns: explicit
// follow intent, whether the reader is parked away from the live edge, and the
// unseen count accumulated while parked. This module is their sole writer.

import { historyQueryParams, loadHistoryHead, probeNewCount } from "./data";
import { $ } from "./dom";
import { commit, getState, type State } from "./store";

function followState(timeline: boolean): Record<string, boolean> {
  return { ...getState().followByLens, timeline };
}

// Includes intent that produces no store commit (for example, moving again
// while already parked), so an older async catch-up/poll response cannot win.
let intentGeneration = 0;

/** Token for async work whose result is relevant only to the current read intent. */
export function timelineFollowGeneration(): number {
  return intentGeneration;
}

function headAnchor(): State["timelineHeadAnchor"] {
  const head = getState().history?.entries?.[0];
  const occurredAt = head?.occurredAt;
  const eventId = head?.eventId;
  return occurredAt && eventId ? { occurredAt, eventId } : null;
}

/** Whether timeline ingestion is enabled by explicit user intent. */
export function isFollowingTimeline(): boolean {
  return getState().followByLens.timeline === true;
}

/** Freeze the displayed head for unseen counting without changing follow intent. */
export function parkTimelineRead(): void {
  const state = getState();
  if (state.timelineHeadAnchor) return;
  const anchor = headAnchor();
  if (!anchor) return;
  intentGeneration += 1;
  commit({ timelineHeadAnchor: anchor, timelineNewCount: 0 });
}

/** Reload the head and expose queued arrivals without changing selection or follow. */
export async function catchUpTimeline(): Promise<void> {
  const state = getState();
  if (
    !state.followByLens.timeline ||
    state.order !== "desc" ||
    !state.timelineHeadAnchor
  )
    return;
  const generation = timelineFollowGeneration();
  const queryKey = historyQueryParams(state);
  if (
    !(await loadHistoryHead(() => {
      const current = getState();
      return (
        timelineFollowGeneration() === generation &&
        current.followByLens.timeline &&
        current.order === "desc" &&
        current.timelineHeadAnchor != null &&
        historyQueryParams(current) === queryKey
      );
    }))
  )
    return;

  intentGeneration += 1;
  commit({ timelineHeadAnchor: null, timelineNewCount: 0 });
  const timeline = $<HTMLElement>("#timeline");
  if (timeline) timeline.scrollTop = 0;
}

/** Toggle timeline ingestion explicitly, preserving the current read and selection. */
export async function toggleTimelineFollow(): Promise<void> {
  intentGeneration += 1;
  if (isFollowingTimeline()) {
    commit({
      followByLens: followState(false),
      timelineNewCount: 0,
    });
    return;
  }

  commit({ followByLens: followState(true) });
  const state = getState();
  if (state.order !== "desc") return;
  if (state.timelineHeadAnchor) {
    await probeNewCount();
    return;
  }

  const generation = timelineFollowGeneration();
  const queryKey = historyQueryParams(state);
  await loadHistoryHead(() => {
    const current = getState();
    return (
      timelineFollowGeneration() === generation &&
      current.followByLens.timeline &&
      current.order === "desc" &&
      current.timelineHeadAnchor == null &&
      historyQueryParams(current) === queryKey
    );
  });
}

/** Re-anchor a newly loaded query without changing explicit follow intent. */
export function resetTimelineReadForQueryChange(): void {
  intentGeneration += 1;
  const state = getState();
  const engaged = state.selected.kind === "event" && Boolean(state.selected.id);
  const atDescendingHead = state.order === "desc" && !engaged;
  commit({
    timelineHeadAnchor: atDescendingHead ? null : headAnchor(),
    timelineNewCount: 0,
  });
}
