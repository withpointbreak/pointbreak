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
import {
  buildChangePageUrl,
  type ChangePageQuery,
  decodeChangePage,
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
  return parseChangeInspectorRoute(location.hash || "#/changes");
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
  const route = currentRoute();
  const committed = route.kind === "invalid" ? "" : (route.query.q ?? "");
  const focused = document.activeElement === filterInput;
  if (!focused && filterInput.value === committed) return null;
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
  let reading: ChangeInspectorReading | null = null;
  let readingRefusal: string | null = null;
  let visibleReading = "";
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
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      {
        reading,
        refusal: readingRefusal,
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
    interaction?.sync(state.snapshot());
  };
  let interaction: ReturnType<typeof installChangeInspectorInteraction> | null =
    null;
  const requestKey = (query: ChangePageQuery): string =>
    buildChangePageUrl("changes", query);
  let visibleRequest = "";
  let pendingReading: { key: string; token: symbol } | null = null;
  let releaseQueuedPoll: () => void = () => {};

  const readingKey = (
    route: Exclude<ChangeInspectorRoute, { kind: "lens" | "invalid" }>,
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
    if (route.kind === "lens") {
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
            error.code === "stale_projection")) &&
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
      const query = route.query;
      const activeLens = lensForRoute(route);
      // Continuations are signed to one exact lens/query/projection tuple.
      // Stage the active page beside the companion lens's first page; sending
      // one lens's opaque cursor to the other would make pagination refuse the
      // entire generation even though both pages share its projection stamp.
      const changesQuery =
        activeLens === "changes" ? query : firstPageQuery(query);
      const attentionQuery =
        activeLens === "attention" ? query : firstPageQuery(query);
      const [changes, attention] = await Promise.all([
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
      ]);
      const postflight = decodeReaderProfile(
        await fetchChangeInspectorJSON("/api/v2/profile"),
      );
      if (epoch !== requestEpoch) return;
      const staged = stageGeneration(profile, changes, attention, postflight);
      if (route.kind !== "lens") {
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
      visibleRequest = requestKey(query);
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
          error.code === "stale_projection") ||
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
    const route = currentRoute();
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
      request = requestKey(route.query);
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
  prepareChangeInspectorShell({ navigate });
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
  interaction = installChangeInspectorInteraction({ navigate });
  interactionStop = interaction.stop;
  filterInput = document.querySelector<HTMLInputElement>("#filter-text");
  filterInputListener = () => {
    const route = currentRoute();
    const base =
      route.kind === "invalid"
        ? { kind: "lens" as const, lens: "changes" as const, query: {} }
        : route;
    navigate({
      ...base,
      query: {
        ...base.query,
        after: undefined,
        q: filterInput?.value || undefined,
      },
    } as Exclude<ChangeInspectorRoute, { kind: "invalid" }>);
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
