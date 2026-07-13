// Timeline follow is session-only stream-position state. This module is its sole
// writer: movement/engagement callers end follow here, while the explicit pill
// and control resume through the success-gated head reload.

import { historyQueryParams, loadHistoryHead } from "./data";
import { $ } from "./dom";
import { navigate } from "./router";
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

/** Whether the descending timeline is currently auto-advancing at its live edge. */
export function isFollowingTimeline(): boolean {
  return getState().followByLens.timeline === true;
}

/** End follow once and freeze the newest loaded entry as the count-since anchor. */
export function endTimelineFollow(): void {
  intentGeneration += 1;
  if (!isFollowingTimeline()) return;
  commit({
    followByLens: followState(false),
    timelineHeadAnchor: headAnchor(),
    timelineNewCount: 0,
  });
}

/** Reload the head, then explicitly leave the parked read and resume following. */
export async function resumeTimelineFollow(): Promise<void> {
  if (isFollowingTimeline()) return;
  const generation = timelineFollowGeneration();
  const queryKey = historyQueryParams(getState());
  if (
    !(await loadHistoryHead(
      () =>
        timelineFollowGeneration() === generation &&
        historyQueryParams(getState()) === queryKey,
    ))
  )
    return;

  // Selection is routed state: navigate first so Back can restore the parked read.
  navigate({ selected: { kind: null, id: null } });
  intentGeneration += 1;
  commit({
    followByLens: followState(true),
    timelineHeadAnchor: null,
    timelineNewCount: 0,
  });
  const timeline = $<HTMLElement>("#timeline");
  if (timeline) timeline.scrollTop = 0;
}

/** Reconcile follow after a query-driven page-one commit. */
export function resetTimelineFollowForQueryChange(): void {
  intentGeneration += 1;
  const engaged =
    getState().selected.kind === "event" && Boolean(getState().selected.id);
  commit({
    followByLens: followState(!engaged),
    timelineHeadAnchor: engaged ? headAnchor() : null,
    timelineNewCount: 0,
  });
}
