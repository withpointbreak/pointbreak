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
  formatChangeInspectorRoute,
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
import { createDisclosure } from "./disclosure";

export interface ChangeInspectorOptions {
  poll?: boolean;
  reload?: () => void;
}

let pollTimer: ReturnType<typeof setInterval> | null = null;
let routeListener: (() => void) | null = null;
let filterInput: HTMLInputElement | null = null;
let filterInputListener: (() => void) | null = null;
let connectionControlsInitialized = false;
let filterDisclosureInitialized = false;
let requestEpoch = 0;

function currentRoute(): ChangeInspectorRoute {
  return parseChangeInspectorRoute(location.hash || "#/changes");
}

/** Stop the active composition without touching the quarantined legacy reader. */
export function stopChangeInspector(): void {
  requestEpoch += 1;
  if (pollTimer !== null) clearInterval(pollTimer);
  pollTimer = null;
  if (routeListener !== null)
    window.removeEventListener("hashchange", routeListener);
  routeListener = null;
  if (filterInput !== null && filterInputListener !== null) {
    filterInput.removeEventListener("change", filterInputListener);
  }
  filterInput = null;
  filterInputListener = null;
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
  const paint = () =>
    renderChangeInspector(
      state.snapshot(),
      { navigate },
      {
        reading,
        refusal: readingRefusal,
      },
    );
  const requestKey = (query: ChangePageQuery): string =>
    buildChangePageUrl("changes", query);
  let visibleRequest = "";

  const clearReading = (): void => {
    reading = null;
    readingRefusal = null;
    visibleReading = "";
  };

  const loadReading = async (
    route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
    expectedProjectionStamp: string,
    epoch: number,
    restarted = false,
  ): Promise<void> => {
    if (route.kind === "lens") {
      clearReading();
      return;
    }
    const requested = formatChangeInspectorRoute(route);
    if (visibleReading === requested && reading !== null) return;
    reading = null;
    readingRefusal = null;
    visibleReading = requested;
    paint();
    try {
      const loaded = await loadChangeInspectorReading(
        route,
        expectedProjectionStamp,
      );
      const postflight = decodeReaderProfile(
        await fetchChangeInspectorJSON("/api/v2/profile"),
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
      paint();
    } catch (error) {
      if (epoch !== requestEpoch) return;
      if (
        !restarted &&
        (error instanceof ChangeInspectorGenerationChanged ||
          (error instanceof ChangeInspectorPageFailure &&
            error.code === "stale_projection"))
      ) {
        await loadGeneration(route, true);
        return;
      }
      reading = null;
      readingRefusal = error instanceof Error ? error.message : String(error);
      paint();
    }
  };

  const loadGeneration = async (
    route: Exclude<ChangeInspectorRoute, { kind: "invalid" }>,
    restarted = false,
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
      const [changes, attention] = await Promise.all([
        fetchChangeInspectorJSON(buildChangePageUrl("changes", query)).then(
          (value) =>
            decodeChangePage(value, { lens: "changes", bounded: true }),
        ),
        fetchChangeInspectorJSON(buildChangePageUrl("attention", query)).then(
          (value) =>
            decodeChangePage(value, { lens: "attention", bounded: true }),
        ),
      ]);
      const postflight = decodeReaderProfile(
        await fetchChangeInspectorJSON("/api/v2/profile"),
      );
      if (epoch !== requestEpoch) return;
      state.publish(stageGeneration(profile, changes, attention, postflight));
      visibleRequest = requestKey(query);
      paint();
      await loadReading(route, changes.projectionStamp, epoch);
    } catch (error) {
      if (epoch !== requestEpoch) return;
      if (
        !restarted &&
        ((error instanceof ChangeInspectorPageFailure &&
          error.code === "stale_projection") ||
          error instanceof ChangeInspectorGenerationChanged)
      ) {
        await loadGeneration(route, true);
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
        await loadGeneration(route);
      } else {
        await loadReading(
          route,
          generation.changes.projectionStamp,
          requestEpoch,
        );
        paint();
      }
    } else {
      visibleRequest = "";
      clearReading();
      state.clearGeneration();
      paint();
      await loadGeneration(route);
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
    await loadGeneration(route);
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
  if (!filterDisclosureInitialized) {
    createDisclosure({
      container: "#filter-controls",
      trigger: "#filters-toggle",
      panel: "#filters-panel",
    });
    filterDisclosureInitialized = true;
  }
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
  if (options.poll !== false)
    pollTimer = setInterval(() => {
      const route = currentRoute();
      if (route.kind !== "invalid") void loadGeneration(route);
    }, 3000);
}
