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
  sessionCredentialVersion,
} from "./auth";
import {
  ChangeInspectorPageFailure,
  fetchChangeInspectorJSON,
} from "./change-inspector-http";
import {
  decodeInspectorIdentity,
  type InspectorIdentity,
} from "./change-inspector-identity";
import { installChangeInspectorInteraction } from "./change-inspector-interaction";
import {
  type ChangeInspectorReading,
  loadChangeInspectorReading,
} from "./change-inspector-reading";
import {
  prepareChangeInspectorShell,
  renderChangeInspector,
  renderChangeInspectorIdentity,
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
  type ChangeInspectorSearchController,
  installChangeInspectorSearch,
} from "./change-inspector-search";
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
  type EventHistoryQuery,
  sameProfileGeneration,
} from "./change-protocol";
import {
  configureConnectionActions,
  initConnectionControls,
  setRefreshState,
} from "./connection";
import { createDisclosure, type DisclosureController } from "./disclosure";

export interface ChangeInspectorOptions {
  poll?: boolean;
  reload?: () => void;
}

interface ProjectionRetryBudget {
  remaining: number;
}

type GenerationLoadOrigin = "route" | "poll" | "recovery";

type ChangeInspectorExactRoute = Exclude<
  ChangeInspectorRoute,
  { kind: "lens" | "timeline" | "event" | "invalid" }
>;

interface FocusedFilterDraft {
  input: HTMLInputElement;
  restoreFocus: boolean;
  value: string;
  selectionStart: number | null;
  selectionEnd: number | null;
  selectionDirection: "forward" | "backward" | "none" | null;
}

interface CredentialedInspectorIdentity {
  identity: InspectorIdentity;
  credentialVersion: number;
}

const EXACT_READING_TIMEOUT_MS = 10_000;
const IDENTITY_TIMEOUT_MS = 3_000;
const POLL_CYCLE_TIMEOUT_MS = 15_000;

class ChangeInspectorTimeout extends Error {}
class ChangeInspectorSessionChanged extends Error {}

let pollTimer: ReturnType<typeof setInterval> | null = null;
let routeListener: (() => void) | null = null;
let filterInput: HTMLInputElement | null = null;
let searchController: ChangeInspectorSearchController | null = null;
let connectionControlsInitialized = false;
let filterDisclosure: DisclosureController | null = null;
let viewDisclosure: DisclosureController | null = null;
let interactionStop: (() => void) | null = null;
let timelineSearchFocusIntentStop: (() => void) | null = null;
let pollCoordinatorStop: (() => void) | null = null;
let refreshSettleTimer: ReturnType<typeof setTimeout> | null = null;
let requestEpoch = 0;
let compositionEpoch = 0;

function clearRefreshSettleTimer(): void {
  if (refreshSettleTimer !== null) clearTimeout(refreshSettleTimer);
  refreshSettleTimer = null;
}

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
  compositionEpoch += 1;
  requestEpoch += 1;
  if (pollTimer !== null) clearInterval(pollTimer);
  pollTimer = null;
  pollCoordinatorStop?.();
  pollCoordinatorStop = null;
  clearRefreshSettleTimer();
  if (routeListener !== null)
    window.removeEventListener("hashchange", routeListener);
  routeListener = null;
  searchController?.stop();
  searchController = null;
  filterInput = null;
  filterDisclosure?.dispose();
  filterDisclosure = null;
  viewDisclosure?.dispose();
  viewDisclosure = null;
  interactionStop?.();
  interactionStop = null;
  timelineSearchFocusIntentStop?.();
  timelineSearchFocusIntentStop = null;
}

/** Start the only active Inspector reader composition. */
export async function bootstrapChangeInspector(
  options: ChangeInspectorOptions = {},
): Promise<void> {
  stopChangeInspector();
  const composition = compositionEpoch;
  const isCurrentComposition = (): boolean => composition === compositionEpoch;
  const capability = bootstrapCapability();
  if (capability.token !== null) {
    (options.reload ?? (() => location.reload()))();
    return;
  }
  installDefaultAuthCoordinator();
  const refreshEnabled = options.poll !== false;
  const state = createChangeInspectorState(currentRoute());
  const credentialSessionChanged = (startedAt: number): boolean =>
    sessionCredentialVersion() !== startedAt;
  const showAcceptedPublication = (
    transition: "initial" | "unchanged" | "changed",
    origin: GenerationLoadOrigin,
  ): void => {
    if (!refreshEnabled) return;
    clearRefreshSettleTimer();
    if (transition === "changed" && origin !== "route") {
      setRefreshState("updated");
      refreshSettleTimer = setTimeout(() => {
        refreshSettleTimer = null;
        setRefreshState("watching");
      }, 1200);
      return;
    }
    setRefreshState("watching");
  };
  const showPollFailure = (): void => {
    if (!refreshEnabled) return;
    clearRefreshSettleTimer();
    setRefreshState("degraded");
  };
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
  let pendingTimelineSearchFocus = false;
  const cancelPendingTimelineSearchFocus = () => {
    pendingTimelineSearchFocus = false;
  };
  document.addEventListener("focusin", cancelPendingTimelineSearchFocus, true);
  document.addEventListener(
    "pointerdown",
    cancelPendingTimelineSearchFocus,
    true,
  );
  timelineSearchFocusIntentStop = () => {
    document.removeEventListener(
      "focusin",
      cancelPendingTimelineSearchFocus,
      true,
    );
    document.removeEventListener(
      "pointerdown",
      cancelPendingTimelineSearchFocus,
      true,
    );
  };
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
    searchController?.sync();
    if (pendingTimelineSearchFocus) {
      if (
        snapshot.route.kind !== "timeline" &&
        snapshot.route.kind !== "event"
      ) {
        pendingTimelineSearchFocus = false;
      } else {
        const timeline = document.querySelector<HTMLElement>("#timeline");
        if (timeline !== null) {
          pendingTimelineSearchFocus = false;
          timeline.focus({ preventScroll: true });
        }
      }
    }
  };
  let interaction: ReturnType<typeof installChangeInspectorInteraction> | null =
    null;
  // The filter identity of a history request: its query without either
  // positional anchor, so two pages of the same filtered stream compare equal
  // no matter where each one starts.
  const eventHistoryFilters = (query: EventHistoryQuery): string =>
    buildEventHistoryUrl({ ...query, after: undefined, at: undefined });
  // One function owns the history-page URL for every timeline and event route.
  // An exact event the loaded page already carries under the same filters
  // reads from that page; a deep link, an off-page event, or a filter change
  // re-centers honestly on the requested event.
  const historyPageUrl = (
    route: Extract<
      ChangeInspectorRoute,
      { kind: "timeline" } | { kind: "event" }
    >,
  ): string => {
    if (route.kind === "timeline") {
      return buildEventHistoryUrl(route.historyQuery);
    }
    const staged = state.snapshot().generation?.history ?? null;
    if (
      visibleRequest !== "" &&
      staged !== null &&
      visibleHistoryFilters === eventHistoryFilters(route.historyQuery) &&
      staged.entries.some((entry) => entry.eventId === route.eventId)
    ) {
      return visibleRequest;
    }
    return buildEventHistoryUrl({
      ...route.historyQuery,
      after: undefined,
      at: route.eventId,
    });
  };
  const requestKey = (
    route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
  ): string =>
    route.kind === "timeline" || route.kind === "event"
      ? historyPageUrl(route)
      : buildChangePageUrl("changes", route.query);
  let visibleRequest = "";
  /** The filter identity of the history page `visibleRequest` loaded. */
  let visibleHistoryFilters = "";
  const clearVisibleRequest = (): void => {
    visibleRequest = "";
    visibleHistoryFilters = "";
  };
  let pendingReading: { key: string; token: symbol } | null = null;
  let releaseQueuedPoll: () => void = () => {};
  let revalidateIdentityForCurrentSession: () => void = () => {};

  const readingKey = (
    route: ChangeInspectorExactRoute,
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
    origin: GenerationLoadOrigin = "route",
    credentialVersion = sessionCredentialVersion(),
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
      if (credentialSessionChanged(credentialVersion)) {
        throw new ChangeInspectorSessionChanged();
      }
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
      const sessionChanged =
        error instanceof ChangeInspectorSessionChanged && origin === "route";
      if (
        (error instanceof ChangeInspectorGenerationChanged ||
          sessionChanged ||
          (error instanceof ChangeInspectorPageFailure &&
            (error.code === "stale_projection" ||
              error.code === "moving_journal"))) &&
        consumeProjectionRetry(retryBudget)
      ) {
        if (sessionChanged) revalidateIdentityForCurrentSession();
        await loadGeneration(route, retryBudget, pollDraft, origin);
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
    origin: GenerationLoadOrigin = "route",
  ): Promise<void> => {
    const credentialVersion = sessionCredentialVersion();
    const epoch = ++requestEpoch;
    try {
      // The page this load will publish is decided once, before any staged
      // state can shift beneath it: the same URL is fetched and then recorded
      // as the visible request.
      const request = requestKey(route);
      const profile = decodeReaderProfile(
        await fetchChangeInspectorJSON("/api/v2/profile"),
      );
      if (epoch !== requestEpoch) return;
      if (profile.availability !== "ready") {
        if (origin !== "route" && state.snapshot().generation !== null) {
          showPollFailure();
          return;
        }
        pendingTimelineSearchFocus = false;
        clearVisibleRequest();
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
          ? fetchChangeInspectorJSON(request).then(decodeEventHistory)
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
      const refreshesExactReading =
        origin !== "route" &&
        route.kind !== "lens" &&
        route.kind !== "timeline" &&
        route.kind !== "event";
      let acceptedReading: ChangeInspectorReading | null = null;
      let acceptedReadingKey = "";
      if (refreshesExactReading) {
        acceptedReadingKey = readingKey(route, changes.projectionStamp);
        if (visibleReading === acceptedReadingKey && reading !== null) {
          acceptedReading = reading;
        } else {
          const result = await withinTimeout(
            (async () => {
              const loaded = await loadChangeInspectorReading(
                route,
                changes.projectionStamp,
              );
              const readingPostflight = decodeReaderProfile(
                await fetchChangeInspectorJSON("/api/v2/profile"),
              );
              return { loaded, readingPostflight };
            })(),
            EXACT_READING_TIMEOUT_MS,
            "exact Change reading timed out",
          );
          if (epoch !== requestEpoch) return;
          const browserRoute = currentRoute();
          if (
            browserRoute.kind === "invalid" ||
            formatChangeInspectorRoute(browserRoute) !==
              formatChangeInspectorRoute(route) ||
            !sameProfileGeneration(staged.profile, result.readingPostflight)
          ) {
            throw new ChangeInspectorGenerationChanged();
          }
          acceptedReading = result.loaded;
        }
      }
      if (credentialSessionChanged(credentialVersion)) {
        throw new ChangeInspectorSessionChanged();
      }
      if (
        origin === "route" &&
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
      } else if (refreshesExactReading) {
        reading = acceptedReading;
        readingRefusal = null;
        visibleReading = acceptedReadingKey;
      }
      const publication = state.publish(staged, credentialVersion);
      showAcceptedPublication(publication.transition, origin);
      if (route.kind === "timeline" && history !== null) {
        timelineMonitor.observe(route, history);
      }
      visibleRequest = request;
      visibleHistoryFilters =
        (route.kind === "timeline" || route.kind === "event") &&
        history !== null
          ? eventHistoryFilters(route.historyQuery)
          : "";
      paint(pollDraft);
      if (!refreshesExactReading) {
        await loadReading(
          route,
          changes.projectionStamp,
          epoch,
          retryBudget,
          pollDraft,
          origin,
          credentialVersion,
        );
      }
    } catch (error) {
      if (epoch !== requestEpoch) return;
      const sessionChanged =
        error instanceof ChangeInspectorSessionChanged && origin === "route";
      if (
        ((error instanceof ChangeInspectorPageFailure &&
          (error.code === "stale_projection" ||
            error.code === "moving_journal")) ||
          error instanceof ChangeInspectorGenerationChanged ||
          sessionChanged) &&
        consumeProjectionRetry(retryBudget)
      ) {
        if (sessionChanged) revalidateIdentityForCurrentSession();
        await loadGeneration(route, retryBudget, pollDraft, origin);
        return;
      }
      if (origin !== "route" && state.snapshot().generation !== null) {
        showPollFailure();
        return;
      }
      clearVisibleRequest();
      pendingTimelineSearchFocus = false;
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
      clearVisibleRequest();
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
      clearVisibleRequest();
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
  const readIdentity = async (
    reportConnection: boolean,
  ): Promise<CredentialedInspectorIdentity | null> => {
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const credentialVersion = sessionCredentialVersion();
      try {
        const identity = decodeInspectorIdentity(
          await withinTimeout(
            fetchChangeInspectorJSON("/api/identity", { reportConnection }),
            IDENTITY_TIMEOUT_MS,
            "Inspector identity timed out",
          ),
        );
        if (!isCurrentComposition()) return null;
        if (sessionCredentialVersion() === credentialVersion) {
          return { identity, credentialVersion };
        }
      } catch {
        if (
          !isCurrentComposition() ||
          sessionCredentialVersion() === credentialVersion
        ) {
          return null;
        }
      }
    }
    return null;
  };
  let verifiedIdentityCredentialVersion: number | null = null;
  let identityHydration: Promise<void> | null = null;
  const hydrateIdentity = (): Promise<void> => {
    if (identityHydration !== null) return identityHydration;
    const operation = (async () => {
      const verified = await readIdentity(false);
      if (verified === null || !isCurrentComposition()) return;
      verifiedIdentityCredentialVersion = verified.credentialVersion;
      const next = state.publishIdentity(
        verified.identity,
        verified.credentialVersion,
      );
      renderChangeInspectorIdentity(next.identity ?? null);
    })();
    identityHydration = operation.finally(() => {
      identityHydration = null;
    });
    return identityHydration;
  };
  revalidateIdentityForCurrentSession = () => {
    if (verifiedIdentityCredentialVersion === sessionCredentialVersion()) {
      return;
    }
    void hydrateIdentity();
  };
  const reloadCurrent = async (): Promise<void> => {
    const route = currentRoute();
    if (route.kind === "invalid") {
      await onRoute();
      return;
    }
    const verified = await readIdentity(true);
    if (verified === null || !isCurrentComposition()) {
      showPollFailure();
      return;
    }
    const identity = verified.identity;
    verifiedIdentityCredentialVersion = verified.credentialVersion;
    const prior = state.snapshot();
    const continuesSession =
      prior.identity !== null &&
      prior.identity !== undefined &&
      prior.identity.storeIdentity === identity.storeIdentity &&
      prior.identity.contextIdentity === identity.contextIdentity;
    const mustRetireGeneration = prior.generation !== null && !continuesSession;
    if (mustRetireGeneration) {
      clearVisibleRequest();
      pendingTimelineSearchFocus = false;
      clearReading();
      state.clearGeneration();
    }
    const next = state.publishIdentity(identity, verified.credentialVersion);
    if (mustRetireGeneration) paint();
    else renderChangeInspectorIdentity(next.identity ?? null);
    await loadGeneration(route, newProjectionRetryBudget(), null, "recovery");
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
        clearVisibleRequest();
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
    replace,
    navigateTimelineBoundary,
    revealTimelineEvent: revealChangeInspectorTimelineEvent,
    toggleTimelineMonitoring,
    parkTimelineMonitoring,
  });
  interactionStop = interaction.stop;
  filterInput = document.querySelector<HTMLInputElement>("#filter-text");
  const suggestionList = document.querySelector<HTMLElement>(
    "#filter-suggestions",
  );
  if (filterInput !== null && suggestionList !== null) {
    searchController = installChangeInspectorSearch({
      input: filterInput,
      list: suggestionList,
      readContext: () => {
        const snapshot = state.snapshot();
        const route = snapshot.route;
        const browserRoute = currentRoute();
        const routeKey =
          browserRoute.kind === "invalid"
            ? `invalid:${location.hash}`
            : formatChangeInspectorRoute(browserRoute);
        if (route.kind === "invalid") {
          return {
            surface: "change-timeline",
            completion: null,
            committedQuery: "",
            queryNotices: [],
            routeDiagnostic: snapshot.diagnostic,
            routeKey,
          };
        }
        const timelineRoute =
          route.kind === "timeline" || route.kind === "event";
        const history = timelineRoute ? snapshot.generation?.history : null;
        return {
          surface: timelineRoute ? "change-timeline" : "plain",
          completion: history?.completion ?? null,
          committedQuery: timelineRoute
            ? (route.historyQuery.q ?? "")
            : (route.query.q ?? ""),
          queryNotices: history?.queryNotices ?? [],
          routeDiagnostic: snapshot.diagnostic,
          routeKey,
        };
      },
      applyQuery: (query) => {
        const route = currentRoute();
        const base =
          route.kind === "invalid"
            ? { kind: "timeline" as const, historyQuery: {} }
            : route;
        if (base.kind === "timeline" || base.kind === "event") {
          replace({
            kind: "timeline",
            historyQuery: {
              ...base.historyQuery,
              after: undefined,
              at: undefined,
              q: query,
            },
          });
          return;
        }
        replace({
          ...base,
          query: {
            ...base.query,
            after: undefined,
            q: query,
          },
        } as Exclude<ChangeInspectorRoute, { kind: "invalid" }>);
      },
      focusTimeline: () => {
        const timeline = document.querySelector<HTMLElement>("#timeline");
        if (timeline !== null) {
          pendingTimelineSearchFocus = false;
          timeline.focus({ preventScroll: true });
          return;
        }
        const route = currentRoute();
        pendingTimelineSearchFocus =
          route.kind === "timeline" || route.kind === "event";
      },
    });
  }
  const initialRouteLoad = onRoute();
  void hydrateIdentity();
  await initialRouteLoad;
  if (!isCurrentComposition()) return;
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
        "poll",
      );
      const pollEpoch = requestEpoch;
      void withinTimeout(
        operation,
        POLL_CYCLE_TIMEOUT_MS,
        "Change generation poll timed out",
      )
        .catch((error) => {
          if (
            error instanceof ChangeInspectorTimeout &&
            isCurrentComposition() &&
            requestEpoch === pollEpoch
          ) {
            // The fetch cannot be forcibly cancelled at this layer. Advancing
            // the epoch makes its eventual completion observationally inert
            // before the coalesced successor poll is allowed to publish.
            requestEpoch += 1;
            showPollFailure();
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
