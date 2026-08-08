/**
 * Active Change-first composition root. It validates the reader profile before
 * semantic requests, stages one stamped generation, resolves the URL against
 * that generation, and then paints a DOM projection. Browser route/query state
 * never becomes Change semantic state; it only determines the bounded request.
 */

import {
  bootstrapCapability,
  installDefaultAuthCoordinator,
  requestReconnect,
} from "./auth";
import {
  ChangeInspectorPageFailure,
  fetchChangeInspectorJSON,
} from "./change-inspector-http";
import { installChangeInspectorInteraction } from "./change-inspector-interaction";
import {
  type ChangeInspectorReading,
  loadChangeInspectorReading,
} from "./change-inspector-reading";
import {
  prepareChangeInspectorShell,
  renderChangeInspector,
  renderChangeInspectorRefusal,
  renderChangeInspectorUnavailable,
} from "./change-inspector-render";
import {
  type ChangeInspectorRoute,
  firstPageQuery,
  formatChangeInspectorRoute,
  lensForRoute,
  parseChangeInspectorRoute,
} from "./change-inspector-router";
import {
  ChangeInspectorGenerationChanged,
  createChangeInspectorState,
  stageGeneration,
} from "./change-inspector-state";
import { revealChangeInspectorTimelineEvent } from "./change-inspector-timeline";
import {
  firstTimelineRoute,
  traverseTimelineTail,
} from "./change-inspector-timeline-boundary";
import { createTimelineMonitor } from "./change-inspector-timeline-monitor";
import {
  buildChangePageUrl,
  buildEventHistoryUrl,
  decodeChangePage,
  decodeEventHistory,
  decodeReaderProfile,
  sameProfileGeneration,
} from "./change-protocol";
import {
  configureConnectionActions,
  initConnectionControls,
} from "./connection";
import { createDisclosure, type DisclosureController } from "./disclosure";

export interface ChangeInspectorOptions {
  poll?: boolean;
  reload?: () => void;
}

interface ProjectionRetryBudget {
  remaining: number;
}

interface FocusedFilterDraft {
  input: HTMLInputElement;
  restoreFocus: boolean;
  value: string;
  selectionStart: number | null;
  selectionEnd: number | null;
  selectionDirection: "forward" | "backward" | "none" | null;
}

const EXACT_READING_TIMEOUT_MS = 10_000;
const POLL_CYCLE_TIMEOUT_MS = 15_000;

class ChangeInspectorTimeout extends Error {}

let pollTimer: ReturnType<typeof setInterval> | null = null;
let routeListener: (() => void) | null = null;
let filterInput: HTMLInputElement | null = null;
let filterInputListener: (() => void) | null = null;
let connectionControlsInitialized = false;
let filterDisclosure: DisclosureController | null = null;
let viewDisclosure: DisclosureController | null = null;
let interactionStop: (() => void) | null = null;
let pollCoordinatorStop: (() => void) | null = null;
let requestEpoch = 0;

function currentRoute(): ChangeInspectorRoute {
  return parseChangeInspectorRoute(location.hash || "#/timeline");
}

function newProjectionRetryBudget(): ProjectionRetryBudget {
  return { remaining: 1 };
}

function consumeProjectionRetry(budget: ProjectionRetryBudget): boolean {
  if (budget.remaining === 0) return false;
  budget.remaining -= 1;
  return true;
}

function snapshotFilterDraft(
  input: HTMLInputElement,
  restoreFocus: boolean,
): FocusedFilterDraft {
  return {
    input,
    restoreFocus,
    value: input.value,
    selectionStart: input.selectionStart,
    selectionEnd: input.selectionEnd,
    selectionDirection: input.selectionDirection,
  };
}

function capturePollFilterDraft(): FocusedFilterDraft | null {
  if (filterInput === null) return null;
  const focused = document.activeElement === filterInput;
  // Always retain the input identity for a poll, even when it is clean and
  // unfocused at request start. The reader may focus and edit it while that
  // asynchronous generation read is in flight; paint snapshots the live value
  // immediately before render so the late response cannot erase that draft.
  return snapshotFilterDraft(filterInput, focused);
}

async function withinTimeout<T>(
  operation: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  try {
    return await Promise.race([
      operation,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () => reject(new ChangeInspectorTimeout(message)),
          timeoutMs,
        );
      }),
    ]);
  } finally {
    if (timer !== null) clearTimeout(timer);
  }
}

/** Stop the active composition without touching the quarantined legacy reader. */
export function stopChangeInspector(): void {
  requestEpoch += 1;
  if (pollTimer !== null) clearInterval(pollTimer);
  pollTimer = null;
  pollCoordinatorStop?.();
  pollCoordinatorStop = null;
  if (routeListener !== null)
    window.removeEventListener("hashchange", routeListener);
  routeListener = null;
  if (filterInput !== null && filterInputListener !== null) {
    filterInput.removeEventListener("change", filterInputListener);
  }
  filterInput = null;
  filterInputListener = null;
  filterDisclosure?.dispose();
  filterDisclosure = null;
  viewDisclosure?.dispose();
  viewDisclosure = null;
  interactionStop?.();
  interactionStop = null;
}

/** Start the only active Inspector reader composition. */
export async function bootstrapChangeInspector(
  options: ChangeInspectorOptions = {},
): Promise<void> {
  stopChangeInspector();
  const capability = bootstrapCapability();
  if (capability.token !== null) {
    (options.reload ?? (() => location.reload()))();
    return;
  }
  installDefaultAuthCoordinator();
  const state = createChangeInspectorState(currentRoute());
  const navigate = (
    route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
  ) => {
    const hash = formatChangeInspectorRoute(route);
    if (location.hash !== hash) location.hash = hash;
    else void onRoute();
  };
  // File navigation search is route state so reload/copy/Back remain honest,
  // but typing it must not create a history entry per character. It refines
  // the same exact document and still goes through the normal route epoch.
  const replace = (
    route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
  ) => {
    const hash = formatChangeInspectorRoute(route);
    if (location.hash === hash) return;
    history.replaceState(history.state, "", hash);
    void onRoute();
  };
  let reading: ChangeInspectorReading | null = null;
  let readingRefusal: string | null = null;
  let visibleReading = "";
  const timelineMonitor = createTimelineMonitor();
  // Reader activity is a presentation-only reason to hold a live head page.
  // The monitor's `park` operation is idempotent and never changes the route
  // or server authority; repainting merely keeps the held window visible.
  const parkTimelineMonitoring = () => {
    if (timelineMonitor.park() !== null) paint();
  };
  const paint = (pollDraft: FocusedFilterDraft | null = null) => {
    // A poll snapshots an uncommitted search before its asynchronous read
    // begins. Prefer the live draft if the user kept typing. Value preservation
    // does not steal focus from another control; focus is restored only when
    // the input still owns it or refresh transiently left focus on body.
    const draft =
      pollDraft !== null && filterInput === pollDraft.input
        ? snapshotFilterDraft(
            filterInput,
            document.activeElement === filterInput ||
              (document.activeElement === document.body &&
                pollDraft.restoreFocus),
          )
        : null;
    const snapshot = state.snapshot();
    const monitor = timelineMonitor.snapshot();
    renderChangeInspector(
      snapshot,
      { navigate, replace, parkTimelineMonitoring },
      {
        reading,
        refusal: readingRefusal,
        timeline: monitor,
      },
    );
    if (draft !== null && filterInput !== null) {
      filterInput.value = draft.value;
      if (draft.restoreFocus) filterInput.focus({ preventScroll: true });
      if (draft.selectionStart !== null && draft.selectionEnd !== null) {
        filterInput.setSelectionRange(
          draft.selectionStart,
          draft.selectionEnd,
          draft.selectionDirection ?? undefined,
        );
      }
    }
    // The parked monitor can still retain the prior head while a routed page
    // is loading. Do not publish those stale logical rows to the interaction
    // owner when no Timeline generation is actually mounted; pending page and
    // global-boundary selections must land against the destination page.
    const interactiveTimeline =
      (snapshot.route.kind === "timeline" || snapshot.route.kind === "event") &&
      snapshot.generation !== null
        ? snapshot.route.kind === "timeline"
          ? (monitor?.display ?? snapshot.generation.history)
          : snapshot.generation.history
        : null;
    interaction?.sync(snapshot, interactiveTimeline);
  };
  let interaction: ReturnType<typeof installChangeInspectorInteraction> | null =
    null;
  const requestKey = (
    route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
  ): string =>
    route.kind === "timeline" || route.kind === "event"
      ? buildEventHistoryUrl(
          route.kind === "event"
            ? { ...route.historyQuery, after: undefined, at: route.eventId }
            : route.historyQuery,
        )
      : buildChangePageUrl("changes", route.query);
  let visibleRequest = "";
  let pendingReading: { key: string; token: symbol } | null = null;
  let releaseQueuedPoll: () => void = () => {};

  const readingKey = (
    route: Exclude<
      ChangeInspectorRoute,
      { kind: "lens" | "timeline" | "event" | "invalid" }
    >,
    projectionStamp: string,
  ): string => `${formatChangeInspectorRoute(route)}\u0000${projectionStamp}`;

  const clearReading = (): void => {
    reading = null;
    readingRefusal = null;
    visibleReading = "";
  };

  const loadReading = async (
    route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
    expectedProjectionStamp: string,
    epoch: number,
    retryBudget: ProjectionRetryBudget,
    pollDraft: FocusedFilterDraft | null = null,
  ): Promise<void> => {
    if (
      route.kind === "lens" ||
      route.kind === "timeline" ||
      route.kind === "event"
    ) {
      clearReading();
      return;
    }
    const requested = formatChangeInspectorRoute(route);
    // Exact contextual documents are projections, not route-owned cache
    // entries. The same deep link must be hydrated again when a poll stages a
    // newer projection generation or it can retain stale facts indefinitely.
    const requestedReading = readingKey(route, expectedProjectionStamp);
    if (visibleReading === requestedReading && reading !== null) return;
    reading = null;
    readingRefusal = null;
    visibleReading = requestedReading;
    paint(pollDraft);
    const pendingToken = Symbol("exact-reading");
    pendingReading = { key: requestedReading, token: pendingToken };
    try {
      const { loaded, postflight } = await withinTimeout(
        (async () => {
          const loaded = await loadChangeInspectorReading(
            route,
            expectedProjectionStamp,
          );
          const postflight = decodeReaderProfile(
            await fetchChangeInspectorJSON("/api/v2/profile"),
          );
          return { loaded, postflight };
        })(),
        EXACT_READING_TIMEOUT_MS,
        "exact Change reading timed out",
      );
      if (epoch !== requestEpoch || currentRoute().kind === "invalid") return;
      const staged = state.snapshot().generation;
      if (
        staged === null ||
        formatChangeInspectorRoute(
          currentRoute() as Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
        ) !== requested ||
        !sameProfileGeneration(staged.profile, postflight)
      ) {
        throw new ChangeInspectorGenerationChanged();
      }
      reading = loaded;
      readingRefusal = null;
      paint(pollDraft);
    } catch (error) {
      if (epoch !== requestEpoch) return;
      if (
        (error instanceof ChangeInspectorGenerationChanged ||
          (error instanceof ChangeInspectorPageFailure &&
            (error.code === "stale_projection" ||
              error.code === "moving_journal"))) &&
        consumeProjectionRetry(retryBudget)
      ) {
        await loadGeneration(route, retryBudget, pollDraft);
        return;
      }
      reading = null;
      readingRefusal = error instanceof Error ? error.message : String(error);
      paint(pollDraft);
    } finally {
      if (pendingReading?.token === pendingToken) pendingReading = null;
      releaseQueuedPoll();
    }
  };

  const loadGeneration = async (
    route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
    retryBudget: ProjectionRetryBudget,
    pollDraft: FocusedFilterDraft | null = null,
  ): Promise<void> => {
    const epoch = ++requestEpoch;
    try {
      const profile = decodeReaderProfile(
        await fetchChangeInspectorJSON("/api/v2/profile"),
      );
      if (epoch !== requestEpoch) return;
      if (profile.availability !== "ready") {
        visibleRequest = "";
        clearReading();
        state.clearGeneration();
        renderChangeInspectorUnavailable(profile.availability);
        return;
      }
      const query =
        route.kind === "timeline" || route.kind === "event" ? {} : route.query;
      const activeLens = lensForRoute(route);
      // Continuations are signed to one exact lens/query/projection tuple.
      // Stage the active page beside the companion lens's first page; sending
      // one lens's opaque cursor to the other would make pagination refuse the
      // entire generation even though both pages share its projection stamp.
      const changesQuery =
        activeLens === "changes" ? query : firstPageQuery(query);
      const attentionQuery =
        activeLens === "attention" ? query : firstPageQuery(query);
      const historyRequest =
        route.kind === "timeline" || route.kind === "event"
          ? fetchChangeInspectorJSON(
              buildEventHistoryUrl(
                route.kind === "event"
                  ? {
                      ...route.historyQuery,
                      after: undefined,
                      at: route.eventId,
                    }
                  : route.historyQuery,
              ),
            ).then(decodeEventHistory)
          : Promise.resolve(null);
      const [changes, attention, history] = await Promise.all([
        fetchChangeInspectorJSON(
          buildChangePageUrl("changes", changesQuery),
        ).then((value) =>
          decodeChangePage(value, { lens: "changes", bounded: true }),
        ),
        fetchChangeInspectorJSON(
          buildChangePageUrl("attention", attentionQuery),
        ).then((value) =>
          decodeChangePage(value, { lens: "attention", bounded: true }),
        ),
        historyRequest,
      ]);
      const postflight = decodeReaderProfile(
        await fetchChangeInspectorJSON("/api/v2/profile"),
      );
      if (epoch !== requestEpoch) return;
      const staged = stageGeneration(
        profile,
        changes,
        attention,
        postflight,
        history,
      );
      if (
        route.kind !== "lens" &&
        route.kind !== "timeline" &&
        route.kind !== "event"
      ) {
        const requestedReading = readingKey(route, changes.projectionStamp);
        if (visibleReading !== requestedReading) {
          // Never paint a detail from the prior generation beside a newly
          // published list. loadReading will replace this loading state only
          // after its exact document passes the same-generation postflight.
          reading = null;
          readingRefusal = null;
        }
      }
      state.publish(staged);
      if (route.kind === "timeline" && history !== null) {
        timelineMonitor.observe(route, history);
      }
      visibleRequest = requestKey(route);
      paint(pollDraft);
      await loadReading(
        route,
        changes.projectionStamp,
        epoch,
        retryBudget,
        pollDraft,
      );
    } catch (error) {
      if (epoch !== requestEpoch) return;
      if (
        ((error instanceof ChangeInspectorPageFailure &&
          (error.code === "stale_projection" ||
            error.code === "moving_journal")) ||
          error instanceof ChangeInspectorGenerationChanged) &&
        consumeProjectionRetry(retryBudget)
      ) {
        await loadGeneration(route, retryBudget, pollDraft);
        return;
      }
      visibleRequest = "";
      clearReading();
      state.clearGeneration();
      renderChangeInspectorRefusal(error);
    }
  };

  const onRoute = async (): Promise<void> => {
    // Lightweight panels are transient chrome for the route being inspected.
    // Dismiss them without restoring focus before route focus management runs;
    // otherwise a hash-only navigation can leave a high-z-index panel covering
    // the newly selected Change or exact Revision.
    filterDisclosure?.close();
    viewDisclosure?.close();
    // Capability credentials are startup/transport state, never part of the
    // strict Change route grammar. A pasted same-origin capability can be a
    // hash-only navigation, so consume and scrub it on every route transition
    // before parsing semantic URL state.
    const capability = bootstrapCapability();
    const route = parseChangeInspectorRoute(
      capability.cleanedHash || "#/timeline",
    );
    // Every URL intent, including same-query detail/focus navigation, owns a
    // new read epoch. An older detail request may still finish, but it may not
    // restart a generation for the route the user has already left.
    requestEpoch += 1;
    state.setRoute(route);
    if (route.kind === "invalid") {
      visibleRequest = "";
      clearReading();
      state.clearGeneration();
      paint();
      return;
    }
    let request: string;
    try {
      request = requestKey(route);
    } catch (error) {
      state.clearGeneration();
      renderChangeInspectorRefusal(error);
      return;
    }
    // A query change cannot display the prior semantic generation beneath its
    // new URL. Lens/detail routes with the same query reuse the already-staged
    // pair because both pages were atomically published together.
    if (request === visibleRequest) {
      const generation = state.snapshot().generation;
      if (generation === null) {
        await loadGeneration(route, newProjectionRetryBudget());
      } else {
        await loadReading(
          route,
          generation.changes.projectionStamp,
          requestEpoch,
          newProjectionRetryBudget(),
        );
        paint();
      }
    } else {
      visibleRequest = "";
      clearReading();
      state.clearGeneration();
      paint();
      await loadGeneration(route, newProjectionRetryBudget());
    }
  };
  routeListener = () => {
    void onRoute();
  };
  window.addEventListener("hashchange", routeListener);
  const reloadCurrent = async (): Promise<void> => {
    const route = currentRoute();
    if (route.kind === "invalid") {
      await onRoute();
      return;
    }
    await loadGeneration(route, newProjectionRetryBudget());
  };
  configureConnectionActions({
    retry: reloadCurrent,
    reconnect: async () => {
      if (await requestReconnect()) await reloadCurrent();
    },
  });
  if (!connectionControlsInitialized) {
    initConnectionControls();
    connectionControlsInitialized = true;
  }
  const toggleTimelineMonitoring = () => {
    if (currentRoute().kind !== "timeline") return;
    if (timelineMonitor.toggle() !== null) paint();
  };
  const navigateTimelineBoundary = async (
    boundary: "first" | "last",
    route: Extract<ChangeInspectorRoute, { kind: "timeline" }>,
  ): Promise<Extract<ChangeInspectorRoute, { kind: "timeline" }> | null> => {
    const first = firstTimelineRoute(route);
    if (boundary === "first") {
      navigate(first);
      return first;
    }

    const retryBudget = newProjectionRetryBudget();
    const requestedRoute = formatChangeInspectorRoute(route);
    for (;;) {
      const generation = state.snapshot().generation;
      // A parked monitor intentionally retains an older presentation window.
      // Global traversal, however, is an authority read and must be anchored
      // to the current staged generation or its first fetched page will appear
      // to cross generations and refuse forever while the monitor is parked.
      const anchor = generation?.history;
      if (generation === null || anchor === null || anchor === undefined) {
        return null;
      }
      const epoch = ++requestEpoch;
      try {
        const preflight = decodeReaderProfile(
          await fetchChangeInspectorJSON("/api/v2/profile"),
        );
        if (
          epoch !== requestEpoch ||
          currentRoute().kind === "invalid" ||
          formatChangeInspectorRoute(
            currentRoute() as Exclude<
              ChangeInspectorRoute,
              { kind: "invalid" }
            >,
          ) !== requestedRoute
        ) {
          return null;
        }
        if (!sameProfileGeneration(generation.profile, preflight)) {
          throw new ChangeInspectorGenerationChanged();
        }
        const tail = await traverseTimelineTail(
          route,
          anchor,
          async (query) => {
            const page = decodeEventHistory(
              await fetchChangeInspectorJSON(buildEventHistoryUrl(query)),
            );
            if (epoch !== requestEpoch) {
              throw new ChangeInspectorGenerationChanged();
            }
            return page;
          },
        );
        const postflight = decodeReaderProfile(
          await fetchChangeInspectorJSON("/api/v2/profile"),
        );
        if (
          epoch !== requestEpoch ||
          !sameProfileGeneration(generation.profile, postflight)
        ) {
          throw new ChangeInspectorGenerationChanged();
        }
        navigate(tail.route);
        return tail.route;
      } catch (error) {
        if (epoch !== requestEpoch) return null;
        if (
          (error instanceof ChangeInspectorGenerationChanged ||
            (error instanceof ChangeInspectorPageFailure &&
              (error.code === "stale_projection" ||
                error.code === "moving_journal"))) &&
          consumeProjectionRetry(retryBudget)
        ) {
          await loadGeneration(route, retryBudget);
          if (
            currentRoute().kind === "invalid" ||
            formatChangeInspectorRoute(
              currentRoute() as Exclude<
                ChangeInspectorRoute,
                { kind: "invalid" }
              >,
            ) !== requestedRoute
          ) {
            return null;
          }
          continue;
        }
        visibleRequest = "";
        clearReading();
        state.clearGeneration();
        renderChangeInspectorRefusal(error);
        return null;
      }
    }
  };
  prepareChangeInspectorShell({ navigate, toggleTimelineMonitoring });
  filterDisclosure = createDisclosure({
    container: "#filter-controls",
    trigger: "#filters-toggle",
    panel: "#filters-panel",
  });
  viewDisclosure = createDisclosure({
    container: "#view-controls",
    trigger: "#view-toggle",
    panel: "#view-panel",
  });
  interaction = installChangeInspectorInteraction({
    navigate,
    navigateTimelineBoundary,
    revealTimelineEvent: revealChangeInspectorTimelineEvent,
    toggleTimelineMonitoring,
    parkTimelineMonitoring,
  });
  interactionStop = interaction.stop;
  filterInput = document.querySelector<HTMLInputElement>("#filter-text");
  filterInputListener = () => {
    const route = currentRoute();
    const base =
      route.kind === "invalid"
        ? { kind: "timeline" as const, historyQuery: {} }
        : route;
    if (base.kind === "timeline" || base.kind === "event") {
      navigate({
        kind: "timeline",
        historyQuery: {
          ...base.historyQuery,
          after: undefined,
          at: undefined,
          q: filterInput?.value || undefined,
        },
      });
    } else {
      navigate({
        ...base,
        query: {
          ...base.query,
          after: undefined,
          q: filterInput?.value || undefined,
        },
      } as Exclude<ChangeInspectorRoute, { kind: "invalid" }>);
    }
  };
  filterInput?.addEventListener("change", filterInputListener);
  await onRoute();
  if (options.poll !== false) {
    let pollRequested = false;
    let pollRunning = false;
    let pollActive = true;
    const drainPoll = (): void => {
      if (!pollActive || pollRunning || !pollRequested) return;
      const route = currentRoute();
      if (route.kind === "invalid") {
        pollRequested = false;
        return;
      }
      const generation = state.snapshot().generation;
      if (
        route.kind !== "lens" &&
        route.kind !== "timeline" &&
        route.kind !== "event" &&
        generation !== null &&
        pendingReading?.key ===
          readingKey(route, generation.changes.projectionStamp)
      ) {
        return;
      }
      pollRequested = false;
      pollRunning = true;
      const operation = loadGeneration(
        route,
        newProjectionRetryBudget(),
        capturePollFilterDraft(),
      );
      void withinTimeout(
        operation,
        POLL_CYCLE_TIMEOUT_MS,
        "Change generation poll timed out",
      )
        .catch((error) => {
          if (error instanceof ChangeInspectorTimeout) {
            // The fetch cannot be forcibly cancelled at this layer. Advancing
            // the epoch makes its eventual completion observationally inert
            // before the coalesced successor poll is allowed to publish.
            requestEpoch += 1;
          }
        })
        .finally(() => {
          pollRunning = false;
          drainPoll();
        });
    };
    const requestPoll = (): void => {
      pollRequested = true;
      drainPoll();
    };
    releaseQueuedPoll = drainPoll;
    pollCoordinatorStop = () => {
      pollActive = false;
      pollRequested = false;
    };
    pollTimer = setInterval(requestPoll, 3000);
  }
}
