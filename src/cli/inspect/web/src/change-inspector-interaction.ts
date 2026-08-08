/**
 * Interaction policy for the Change-first reader.  This module deliberately
 * keeps selection (a local reading cursor) separate from navigation (a routed,
 * exact request).  In particular, moving with j/k never chooses a Revision.
 */

import type { ChangeInspectorRenderActions } from "./change-inspector-render";
import {
  type ChangeInspectorRoute,
  formatChangeInspectorRoute,
  queryForExactNavigation,
} from "./change-inspector-router";
import type { ChangeInspectorSnapshot } from "./change-inspector-state";
import { scheduleChangeInspectorTimelineRemeasure } from "./change-inspector-timeline";
import {
  boundaryTimelineSelection,
  moveTimelineSelection,
  pageTimelineSelection,
  resolveTimelinePageSelection,
  type TimelineSelectionIntent,
  type TimelineWindow,
} from "./change-inspector-timeline-navigation";
import type { EventHistoryDocument } from "./change-protocol";
import {
  applyPrefs,
  applySplit,
  preferredSplit,
  setDensity,
  setThemeMode,
  watchColorScheme,
} from "./prefs";

let colorSchemeWatcherInstalled = false;

const HISTORY_ORIGIN_KEY = "__pointbreakChangeInspectorOrigin";
const MASTER_SURFACE_KEYS = new Set([
  "/",
  "j",
  "k",
  "ArrowDown",
  "ArrowUp",
  "g",
  "G",
  "f",
  "b",
  "d",
  "u",
  "F",
  "h",
  "l",
]);

function isTextControl(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target.matches(
      "input, textarea, select, [role='textbox'], [role='combobox']",
    )
  );
}

/** Native controls own Enter; the local Change cursor must not override them. */
function isNativeActionControl(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    target.closest(
      "button, summary, a[href], [role='button'], [role='link'], [role='separator']",
    ) !== null
  );
}

function isTimelineListTarget(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    (target.matches("#timeline") || target.closest("#timeline") !== null)
  );
}

function narrowDetailOwnsFocus(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    target.closest("#detail") !== null &&
    window.matchMedia("(max-width: 760px)").matches
  );
}

function refineDiffFocus(
  route: Extract<ChangeInspectorRoute, { kind: "diff" }>,
  patch: Partial<
    NonNullable<Extract<ChangeInspectorRoute, { kind: "diff" }>["focus"]>
  >,
): Extract<ChangeInspectorRoute, { kind: "diff" }> {
  const focus = { ...route.focus, ...patch };
  for (const key of Object.keys(focus) as Array<keyof typeof focus>) {
    if (!focus[key]) delete focus[key];
  }
  return { ...route, ...(Object.keys(focus).length ? { focus } : {}) };
}

function moveDiffTarget(
  items: HTMLElement[],
  current: string | undefined,
  delta: number,
  identity: (item: HTMLElement) => string | undefined,
): HTMLElement | null {
  if (!items.length) return null;
  const index = items.findIndex((item) => identity(item) === current);
  return (
    items[
      Math.max(0, Math.min(items.length - 1, (index < 0 ? -1 : index) + delta))
    ] ?? null
  );
}

function revisionRouteFromDiff(
  route: Extract<ChangeInspectorRoute, { kind: "diff" }>,
): Extract<ChangeInspectorRoute, { kind: "revision" }> {
  return {
    kind: "revision",
    changeId: route.changeId,
    revision: route.revision,
    query: route.query,
    ...(route.focus && (route.focus.factId || route.focus.filePath)
      ? {
          focus: {
            ...(route.focus.factId ? { factId: route.focus.factId } : {}),
            ...(route.focus.filePath ? { filePath: route.focus.filePath } : {}),
          },
        }
      : {}),
  };
}

type ValidRoute = Exclude<ChangeInspectorRoute, { kind: "invalid" }>;

/**
 * Timeline reader activity needs an idempotent park operation. The rendering
 * contract currently exposes only the historical toggle, so retain this
 * optional seam until the composition root wires `TimelineMonitor.park()`.
 */
type TimelineParkActions = ChangeInspectorRenderActions & {
  parkTimelineMonitoring?: () => void;
};

type TimelineRoute = Extract<ValidRoute, { kind: "timeline" }>;
type AdjacentTimelineIntent = Extract<
  TimelineSelectionIntent,
  { kind: "adjacent-page" }
>;
type PendingTimelineSelection = {
  intent: AdjacentTimelineIntent;
  route: string;
  restoreFocus: boolean;
};
type DiffIdentity = {
  changeId: string;
  revisionId: string;
  objectArtifactContentHash: string;
};

function diffIdentity(
  route: Extract<ValidRoute, { kind: "diff" }>,
): DiffIdentity {
  return {
    changeId: route.changeId,
    revisionId: route.revision.revisionId,
    objectArtifactContentHash: route.revision.objectArtifactContentHash,
  };
}

function sameDiffIdentity(
  left: DiffIdentity | null,
  right: DiffIdentity | null,
): boolean {
  return (
    left !== null &&
    right !== null &&
    left.changeId === right.changeId &&
    left.revisionId === right.revisionId &&
    left.objectArtifactContentHash === right.objectArtifactContentHash
  );
}

function companionTimelineRoute(
  route: ValidRoute | null,
): TimelineRoute | null {
  if (route?.kind === "timeline") return route;
  if (route?.kind === "event") {
    return { kind: "timeline", historyQuery: route.historyQuery };
  }
  return null;
}

function setSelected(changeId: string | null): void {
  document
    .querySelectorAll<HTMLElement>(".unit-card[data-change-id]")
    .forEach((card) => {
      const selected = card.dataset.changeId === changeId;
      card.classList.toggle("change-card-selected", selected);
      card.setAttribute("aria-current", selected ? "true" : "false");
    });
}

function moveSelection(
  selectedChangeId: string | null,
  delta: number,
): string | null {
  const cards = Array.from(
    document.querySelectorAll<HTMLElement>(".unit-card[data-change-id]"),
  );
  if (!cards.length) return selectedChangeId;
  const current = cards.findIndex(
    (card) => card.dataset.changeId === selectedChangeId,
  );
  const next = Math.max(
    0,
    Math.min(cards.length - 1, current < 0 ? 0 : current + delta),
  );
  const card = cards[next];
  const changeId = card.dataset.changeId ?? null;
  setSelected(changeId);
  card.scrollIntoView({ block: "nearest", behavior: "auto" });
  return changeId;
}

function moveSelectionToBoundary(
  selectedChangeId: string | null,
  boundary: "first" | "last",
): string | null {
  const cards = Array.from(
    document.querySelectorAll<HTMLElement>(".unit-card[data-change-id]"),
  );
  const card = boundary === "first" ? cards[0] : cards.at(-1);
  if (!card) return selectedChangeId;
  const changeId = card.dataset.changeId ?? null;
  setSelected(changeId);
  card.scrollIntoView({ block: "nearest", behavior: "auto" });
  return changeId;
}

function timelineRows(): HTMLElement[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>("#timeline [data-event-id]"),
  );
}

function timelineWindow(
  eventIds: readonly string[],
  selectedEventId: string | null,
): TimelineWindow {
  return {
    eventIds: [...eventIds],
    selectedEventId,
  };
}

function timelineRowId(eventId: string): string {
  return `timeline-event-${encodeURIComponent(eventId).replaceAll("%", "_")}`;
}

function setTimelineSelected(eventId: string | null): void {
  const list = document.querySelector<HTMLOListElement>("#timeline");
  const rows = timelineRows();
  let activeId: string | null = null;
  for (const row of rows) {
    const rowEventId = row.dataset.eventId ?? null;
    const selected = rowEventId === eventId;
    // A listbox owns one tab stop. Individual rows remain pointer targets and
    // active-descendant options, but do not add hundreds of Tab presses.
    row.tabIndex = -1;
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(selected));
    if (selected && rowEventId !== null) {
      row.id = timelineRowId(rowEventId);
      activeId = row.id;
    }
  }
  if (!list) return;
  list.setAttribute("role", "listbox");
  list.tabIndex = rows.length > 0 ? 0 : -1;
  list.toggleAttribute("aria-disabled", rows.length === 0);
  if (activeId) list.setAttribute("aria-activedescendant", activeId);
  else list.removeAttribute("aria-activedescendant");
}

function focusTimelineSelection(eventId: string): void {
  const row = timelineRows().find((item) => item.dataset.eventId === eventId);
  row?.scrollIntoView?.({ block: "nearest", behavior: "auto" });
  document
    .querySelector<HTMLElement>("#timeline")
    ?.focus({ preventScroll: true });
}

function visibleTimelineRows(): number {
  const list = document.querySelector<HTMLElement>("#timeline");
  const row = timelineRows()[0];
  const rowHeight = row?.getBoundingClientRect().height ?? 0;
  if (!list || rowHeight <= 0 || list.clientHeight <= 0) return 10;
  return Math.max(1, Math.floor(list.clientHeight / rowHeight));
}

function timelinePager(
  direction: "previous" | "next",
): HTMLButtonElement | null {
  const explicit = document.querySelector<HTMLButtonElement>(
    `#timeline [data-timeline-page="${direction}"], [data-timeline-page="${direction}"]`,
  );
  if (explicit) return explicit;
  return (
    Array.from(
      document.querySelectorAll<HTMLButtonElement>("#master button"),
    ).find(
      (button) =>
        button.textContent?.trim() ===
        `${direction === "next" ? "Next" : "Previous"} page`,
    ) ?? null
  );
}

function trapModalFocus(modal: HTMLElement, event: KeyboardEvent): void {
  if (event.key !== "Tab") return;
  const stops = Array.from(
    modal.querySelectorAll<HTMLElement>(
      "button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
    ),
  );
  if (!stops.length) return;
  const active =
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
  const first = stops[0];
  const last = stops.at(-1) ?? first;
  if (event.shiftKey && (active === first || !modal.contains(active))) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (active === last || !modal.contains(active))) {
    event.preventDefault();
    first.focus();
  }
}

/** Install once per active composition and return a sync hook for every paint. */
export function installChangeInspectorInteraction(
  actions: ChangeInspectorRenderActions,
): {
  sync(
    snapshot: ChangeInspectorSnapshot,
    timelinePage?: EventHistoryDocument | null,
  ): void;
  stop(): void;
} {
  let selectedChangeId: string | null = null;
  let selectedTimelineEventId: string | null = null;
  let currentTimelineEventIds: string[] = [];
  let pendingTimelineSelection: PendingTimelineSelection | null = null;
  let pendingGlobalTimelineSelection: {
    boundary: "first" | "last";
    route: string;
    restoreFocus: boolean;
  } | null = null;
  let modalReturnFocus: HTMLElement | null = null;
  let detailReturnFocus: HTMLElement | null = null;
  let detailWasOpen = false;
  let currentRoute: ValidRoute | null = null;
  let exactOriginLens: "timeline" | "changes" | "attention" | null = null;
  let timelineOriginRoute: Extract<ValidRoute, { kind: "timeline" }> | null =
    null;
  let detailDomIdentity: ChildNode | null = null;
  let pendingDiffEntryFocus: DiffIdentity | null = null;
  let pendingDiffExitFocus: string | null = null;

  const parkTimelineForReaderActivity = () => {
    (actions as TimelineParkActions).parkTimelineMonitoring?.();
  };

  const selectTimelineEvent = (eventId: string) => {
    selectedTimelineEventId = eventId;
    actions.revealTimelineEvent?.(eventId);
    setTimelineSelected(eventId);
    focusTimelineSelection(eventId);
  };

  const applyTimelineIntent = (intent: TimelineSelectionIntent | null) => {
    if (intent === null) return false;
    parkTimelineForReaderActivity();
    if (intent.kind === "select") {
      pendingTimelineSelection = null;
      selectTimelineEvent(intent.eventId);
      return true;
    }
    const pager = timelinePager(intent.direction);
    const targetRoute = pager?.dataset.timelineTargetRoute;
    if (!pager || !targetRoute) return false;
    pendingTimelineSelection = {
      intent,
      route: targetRoute,
      restoreFocus: document.activeElement?.id === "timeline",
    };
    pager.click();
    return true;
  };

  const syncTimelineDom = (route: ValidRoute | null = currentRoute) => {
    const window = timelineWindow(
      currentTimelineEventIds,
      selectedTimelineEventId,
    );
    // A route transition removes the old Timeline before the destination page
    // is published. Logical monitor rows can outlive that DOM briefly, but
    // consuming a pending boundary against them would strand the new page with
    // no active descendant. Pending selections resolve only after a real list
    // for the routed page has painted.
    const routedTimeline = companionTimelineRoute(route);
    const routedTimelineKey =
      routedTimeline === null
        ? null
        : formatChangeInspectorRoute(routedTimeline);
    const mountedTimeline =
      document.querySelector<HTMLOListElement>("#timeline");
    const mountedTimelineRoute = mountedTimeline?.dataset.timelineRoute ?? null;
    if (
      pendingTimelineSelection !== null &&
      (route?.kind !== "timeline" ||
        routedTimelineKey !== pendingTimelineSelection.route)
    ) {
      pendingTimelineSelection = null;
    }
    if (
      pendingGlobalTimelineSelection !== null &&
      (route?.kind !== "timeline" ||
        routedTimelineKey !== pendingGlobalTimelineSelection.route)
    ) {
      pendingGlobalTimelineSelection = null;
    }
    let restoreTimelineFocus = false;
    if (
      pendingGlobalTimelineSelection !== null &&
      route?.kind === "timeline" &&
      mountedTimelineRoute === formatChangeInspectorRoute(route) &&
      formatChangeInspectorRoute(route) ===
        pendingGlobalTimelineSelection.route &&
      window.eventIds.length > 0
    ) {
      selectedTimelineEventId =
        pendingGlobalTimelineSelection.boundary === "first"
          ? (window.eventIds[0] ?? null)
          : (window.eventIds.at(-1) ?? null);
      restoreTimelineFocus = pendingGlobalTimelineSelection.restoreFocus;
      pendingGlobalTimelineSelection = null;
      if (selectedTimelineEventId !== null) {
        actions.revealTimelineEvent?.(selectedTimelineEventId);
      }
    }
    if (
      pendingTimelineSelection !== null &&
      route?.kind === "timeline" &&
      mountedTimelineRoute === pendingTimelineSelection.route &&
      window.eventIds.length > 0
    ) {
      selectedTimelineEventId = resolveTimelinePageSelection(
        window.eventIds,
        pendingTimelineSelection.intent,
      );
      restoreTimelineFocus = pendingTimelineSelection.restoreFocus;
      pendingTimelineSelection = null;
      if (selectedTimelineEventId !== null) {
        actions.revealTimelineEvent?.(selectedTimelineEventId);
      }
    }
    if (
      selectedTimelineEventId !== null &&
      !window.eventIds.includes(selectedTimelineEventId)
    ) {
      // Virtual scrolling can temporarily unmount the active option. Keep the
      // cursor identity so it is restored if that row re-enters the window,
      // but do not advertise an off-DOM active descendant.
      setTimelineSelected(null);
      return;
    }
    setTimelineSelected(selectedTimelineEventId);
    if (restoreTimelineFocus && selectedTimelineEventId !== null) {
      focusTimelineSelection(selectedTimelineEventId);
    }
  };

  const applyGlobalTimelineBoundary = (
    boundary: "first" | "last",
    route: Extract<ValidRoute, { kind: "timeline" }>,
  ): boolean => {
    const navigateBoundary = actions.navigateTimelineBoundary;
    if (navigateBoundary === undefined) {
      return applyTimelineIntent(
        boundaryTimelineSelection(
          timelineWindow(currentTimelineEventIds, selectedTimelineEventId),
          boundary,
        ),
      );
    }
    parkTimelineForReaderActivity();
    pendingTimelineSelection = null;
    const restoreFocus = document.activeElement?.id === "timeline";
    void navigateBoundary(boundary, route)
      .then((target) => {
        if (target === null) return;
        const targetRoute = formatChangeInspectorRoute(target);
        pendingGlobalTimelineSelection = {
          boundary,
          route: targetRoute,
          restoreFocus,
        };
        // Navigation usually paints after the promise settles. Resolve eagerly
        // only when the exact destination page is already both current and
        // mounted; otherwise the next paint owns the pending selection. This
        // also prevents the source page from clearing a just-created intent.
        const mountedRoute =
          document.querySelector<HTMLOListElement>("#timeline")?.dataset
            .timelineRoute ?? null;
        if (
          currentRoute?.kind === "timeline" &&
          formatChangeInspectorRoute(currentRoute) === targetRoute &&
          mountedRoute === targetRoute
        ) {
          syncTimelineDom();
        }
      })
      .catch(() => {
        pendingGlobalTimelineSelection = null;
      });
    return true;
  };

  applyPrefs();
  if (!colorSchemeWatcherInstalled) {
    watchColorScheme();
    colorSchemeWatcherInstalled = true;
  }

  const historyOrigin = (
    route: ValidRoute,
  ): "timeline" | "changes" | "attention" | null => {
    if (route.kind === "lens" || route.kind === "timeline") return null;
    const state = history.state;
    if (state === null || typeof state !== "object" || Array.isArray(state))
      return null;
    const origin = (state as Record<string, unknown>)[HISTORY_ORIGIN_KEY];
    if (origin === null || typeof origin !== "object") return null;
    const record = origin as Record<string, unknown>;
    if (record.route !== formatChangeInspectorRoute(route)) return null;
    return record.lens === "timeline" ||
      record.lens === "changes" ||
      record.lens === "attention"
      ? record.lens
      : null;
  };

  const persistHistoryOrigin = (
    route: ValidRoute,
    lens: "timeline" | "changes" | "attention",
  ): void => {
    if (route.kind === "lens" || route.kind === "timeline") return;
    const state = history.state;
    const retained =
      state !== null && typeof state === "object" && !Array.isArray(state)
        ? (state as Record<string, unknown>)
        : {};
    history.replaceState(
      {
        ...retained,
        [HISTORY_ORIGIN_KEY]: {
          route: formatChangeInspectorRoute(route),
          lens,
        },
      },
      "",
      location.href,
    );
  };

  const listRoute = (route: ValidRoute): ValidRoute => {
    if (route.kind === "timeline" || route.kind === "lens") return route;
    const lens = historyOrigin(route) ?? exactOriginLens ?? "timeline";
    if (lens !== "timeline") return { kind: "lens", lens, query: route.query };
    // A direct event deep link has no preceding Timeline route to retain, but
    // its typed history query is itself the reader's exact Timeline context.
    // A captured Timeline origin still wins because it may include the signed
    // continuation page from which the event was opened.
    return (
      timelineOriginRoute ??
      (route.kind === "event"
        ? { kind: "timeline", historyQuery: route.historyQuery }
        : { kind: "timeline", historyQuery: {} })
    );
  };

  const focusFallback = (route: ValidRoute | null = currentRoute): void => {
    const target =
      route?.kind === "diff"
        ? document.querySelector<HTMLElement>("#diff-page-close")
        : route !== null && route.kind !== "lens" && route.kind !== "timeline"
          ? window.matchMedia("(max-width: 760px)").matches
            ? document.querySelector<HTMLElement>("#detail-back")
            : document.querySelector<HTMLElement>("#detail-close")
          : document.querySelector<HTMLElement>("#master");
    target?.focus({ preventScroll: true });
  };

  const setCoveredPageInert = (covered: boolean): void => {
    for (const selector of [
      "#topbar",
      "#toolbar",
      "#master-rail",
      "#master",
      ".divider",
    ]) {
      const element = document.querySelector<HTMLElement>(selector);
      if (element) element.inert = covered;
    }
  };

  const onViewportResize = (): void => {
    const covered =
      detailWasOpen && window.matchMedia("(max-width: 760px)").matches;
    setCoveredPageInert(covered);
    const detail = document.querySelector<HTMLElement>("#detail");
    const routeSurface =
      currentRoute?.kind === "diff"
        ? document.querySelector<HTMLElement>("#diff-page")
        : detail;
    const active =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    if (!covered) {
      // The Back affordance disappears at the wide breakpoint. Do not leave
      // browser focus parked on a now-hidden control after the sheet becomes
      // an ordinary split-pane detail.
      if (detailWasOpen && active?.id === "detail-back") {
        document
          .querySelector<HTMLButtonElement>("#detail-close")
          ?.focus({ preventScroll: true });
      }
      return;
    }
    if (
      currentRoute !== null &&
      currentRoute.kind !== "diff" &&
      currentRoute.kind !== "lens" &&
      currentRoute.kind !== "timeline" &&
      active?.id === "detail-close"
    ) {
      // The wide Close affordance disappears when retained detail becomes a
      // narrow sheet. Move focus to its visible Back counterpart instead of
      // accepting a hidden control merely because it remains inside detail.
      focusFallback();
      return;
    }
    if (
      routeSurface !== null &&
      (active === null || !routeSurface.contains(active)) &&
      (active === null || active.closest(".modal:not(.hidden)") === null)
    ) {
      focusFallback();
    }
  };
  window.addEventListener("resize", onViewportResize);

  const closeModal = (id: string): void => {
    const modal = document.querySelector<HTMLElement>(id);
    if (!modal || modal.classList.contains("hidden")) return;
    modal.classList.add("hidden");
    const focus = modalReturnFocus;
    modalReturnFocus = null;
    if (focus?.isConnected === true) focus.focus({ preventScroll: true });
    else focusFallback();
  };

  const openModal = (id: string, initial: HTMLElement | null): void => {
    const modal = document.querySelector<HTMLElement>(id);
    if (!modal) return;
    modalReturnFocus =
      document.activeElement instanceof HTMLElement &&
      document.activeElement !== document.body
        ? document.activeElement
        : null;
    modal.classList.remove("hidden");
    initial?.focus();
  };

  const changeTheme = (event: Event) => {
    const input = event.target as HTMLInputElement;
    if (!input.checked) return;
    setThemeMode(input.value);
  };
  const changeDensity = (event: Event) => {
    const input = event.target as HTMLInputElement;
    if (!input.checked) return;
    setDensity(input.value);
    scheduleChangeInspectorTimelineRemeasure();
  };
  document
    .querySelectorAll<HTMLInputElement>("input[name='theme-mode']")
    .forEach((input) => {
      input.addEventListener("change", changeTheme);
    });
  document
    .querySelectorAll<HTMLInputElement>("input[name='density-mode']")
    .forEach((input) => {
      input.addEventListener("change", changeDensity);
    });

  const paletteInput = document.querySelector<HTMLInputElement>("#cmd-input");
  const paletteResults = document.querySelector<HTMLElement>("#cmd-results");
  const paletteCommands = [
    ["Open Timeline", "timeline"],
    ["Open Changes", "changes"],
    ["Open Attention", "attention"],
  ] as const;
  const renderPaletteResults = () => {
    if (paletteResults) {
      paletteResults.replaceChildren();
      const query = paletteInput?.value.trim().toLocaleLowerCase() ?? "";
      const matching = paletteCommands.filter(([label]) =>
        label.toLocaleLowerCase().includes(query),
      );
      for (const [label, lens] of matching) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "ghost cmd-item";
        const commandLabel = document.createElement("span");
        commandLabel.className = "cmd-label";
        commandLabel.textContent = label;
        button.append(commandLabel);
        button.addEventListener("click", () => {
          closeModal("#cmd-palette");
          const route = currentRoute;
          if (route) {
            actions.navigate(
              lens === "timeline"
                ? { kind: "timeline", historyQuery: {} }
                : {
                    kind: "lens",
                    lens,
                    query:
                      route.kind === "timeline"
                        ? {}
                        : { ...route.query, after: undefined },
                  },
            );
          }
        });
        paletteResults.append(button);
      }
      if (matching.length === 0) {
        const empty = document.createElement("p");
        empty.className = "cmd-empty";
        empty.setAttribute("role", "status");
        empty.textContent = "No matching commands.";
        paletteResults.append(empty);
      }
    }
  };
  const openPalette = () => {
    if (paletteInput) paletteInput.value = "";
    renderPaletteResults();
    openModal("#cmd-palette", paletteInput);
  };
  paletteInput?.addEventListener("input", renderPaletteResults);
  const helpClose = () => closeModal("#key-help");
  const helpCloseButton = document.querySelector("#key-help-close");
  helpCloseButton?.addEventListener("click", helpClose);
  const readingButton =
    document.querySelector<HTMLButtonElement>("#detail-read");
  const masterRail = document.querySelector<HTMLButtonElement>("#master-rail");
  const setReading = (enabled: boolean) => {
    const split = document.querySelector<HTMLElement>(".split");
    const detailViewport = document.querySelector<HTMLElement>("#detail-body");
    const scrollTop = detailViewport?.scrollTop ?? 0;
    split?.classList.toggle("reading", enabled);
    scheduleChangeInspectorTimelineRemeasure();
    if (readingButton) {
      readingButton.textContent = enabled ? "⤡" : "⤢";
      readingButton.setAttribute("aria-pressed", String(enabled));
      readingButton.setAttribute(
        "aria-label",
        enabled ? "Exit reading mode" : "Enter reading mode",
      );
      readingButton.title = enabled ? "Exit reading mode" : "Reading mode";
    }
    if (detailViewport) detailViewport.scrollTop = scrollTop;
  };
  const toggleReading = () => {
    setReading(
      !document.querySelector(".split")?.classList.contains("reading"),
    );
  };
  readingButton?.addEventListener("click", toggleReading);
  const restoreMaster = () => {
    setReading(false);
    document
      .querySelector<HTMLElement>("#master")
      ?.focus({ preventScroll: true });
  };
  masterRail?.addEventListener("click", restoreMaster);

  const divider = document.querySelector<HTMLElement>(".divider");
  const updateSplit = (value: number | null) => {
    applySplit(value);
    scheduleChangeInspectorTimelineRemeasure();
    // `applySplit` clamps before persisting. Reflect that effective value so
    // assistive technology never hears an out-of-range width at either edge.
    divider?.setAttribute("aria-valuenow", String(preferredSplit() ?? 50));
  };
  updateSplit(preferredSplit());
  const onDividerKey = (event: KeyboardEvent) => {
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      updateSplit(null);
      return;
    }
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    event.stopPropagation();
    const value =
      (preferredSplit() ?? 50) + (event.key === "ArrowLeft" ? -5 : 5);
    updateSplit(value);
  };
  divider?.addEventListener("keydown", onDividerKey);
  let activeDividerPointerId: number | null = null;
  const finishDividerDrag = (event: PointerEvent) => {
    if (!divider || activeDividerPointerId !== event.pointerId) return;
    activeDividerPointerId = null;
    divider.classList.remove("dragging");
    if (divider.hasPointerCapture?.(event.pointerId)) {
      divider.releasePointerCapture?.(event.pointerId);
    }
  };
  const onDividerLostPointerCapture = (event: PointerEvent) => {
    if (!divider || activeDividerPointerId !== event.pointerId) return;
    activeDividerPointerId = null;
    divider.classList.remove("dragging");
  };
  const onDividerPointerDown = (event: PointerEvent) => {
    if (
      !divider ||
      activeDividerPointerId !== null ||
      (event.pointerType === "mouse" && event.button !== 0)
    )
      return;
    event.preventDefault();
    divider.focus();
    activeDividerPointerId = event.pointerId;
    divider.setPointerCapture?.(event.pointerId);
    divider.classList.add("dragging");
  };
  const onDividerPointerMove = (event: PointerEvent) => {
    if (
      !divider?.classList.contains("dragging") ||
      activeDividerPointerId !== event.pointerId
    )
      return;
    const split = document.querySelector<HTMLElement>(".split");
    const bounds = split?.getBoundingClientRect();
    if (!bounds || bounds.width <= 0) return;
    const value = ((event.clientX - bounds.left) / bounds.width) * 100;
    // Preserve the retained split's intentional snap into reading mode when a
    // pointer moves decisively past the minimum pane width. The last valid
    // persisted split remains available when the reader restores the rail.
    if (value < 15) {
      finishDividerDrag(event);
      setReading(true);
      return;
    }
    updateSplit(value);
  };
  const onDividerDoubleClick = (event: MouseEvent) => {
    event.preventDefault();
    updateSplit(null);
  };
  divider?.addEventListener("pointerdown", onDividerPointerDown);
  divider?.addEventListener("pointermove", onDividerPointerMove);
  divider?.addEventListener("pointerup", finishDividerDrag);
  divider?.addEventListener("pointercancel", finishDividerDrag);
  divider?.addEventListener("lostpointercapture", onDividerLostPointerCapture);
  divider?.addEventListener("dblclick", onDividerDoubleClick);

  const onClick = (event: MouseEvent) => {
    const target = event.target instanceof Element ? event.target : null;
    const card = target?.closest<HTMLElement>(".unit-card[data-change-id]");
    if (card && !target?.closest("button, a, input, select, textarea")) {
      selectedChangeId = card.dataset.changeId ?? null;
      setSelected(selectedChangeId);
      return;
    }
    const timelineEvent = target?.closest<HTMLElement>(
      "#timeline [data-event-id]",
    );
    if (
      timelineEvent?.dataset.eventId &&
      !target?.closest("button, a[href], input, select, textarea")
    ) {
      parkTimelineForReaderActivity();
      const eventId = timelineEvent.dataset.eventId;
      selectTimelineEvent(eventId);
      const historyQuery =
        currentRoute?.kind === "timeline" || currentRoute?.kind === "event"
          ? currentRoute.historyQuery
          : {};
      actions.navigate({
        kind: "event",
        eventId,
        historyQuery: { ...historyQuery, after: undefined },
        query: {},
      });
    }
  };
  document.addEventListener("click", onClick);
  const onFocusIn = (event: FocusEvent) => {
    const target = event.target instanceof Element ? event.target : null;
    const timelineEvent = target?.closest<HTMLElement>(
      "#timeline [data-event-id]",
    );
    if (!timelineEvent?.dataset.eventId) return;
    selectedTimelineEventId = timelineEvent.dataset.eventId;
    setTimelineSelected(selectedTimelineEventId);
  };
  document.addEventListener("focusin", onFocusIn);
  const onTimelineScroll = (event: Event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest("#timeline")) parkTimelineForReaderActivity();
  };
  // Scroll does not bubble. Capture makes this an integration hook for the
  // virtual list even though the renderer owns the list's own paint listener.
  document.addEventListener("scroll", onTimelineScroll, true);
  const timelineDomObserver = new MutationObserver(() => {
    if (companionTimelineRoute(currentRoute) !== null) syncTimelineDom();
  });
  const master = document.querySelector("#master");
  if (master) {
    timelineDomObserver.observe(master, {
      childList: true,
      subtree: true,
    });
  }
  const onKey = (event: KeyboardEvent) => {
    // Only the palette and key help belong to this interaction controller.
    // Authentication owns the reconnect dialog's settlement, focus trap, and
    // focus restoration; treating every `.modal` as ours could hide that
    // dialog without resolving the pending credential request.
    const open = document.querySelector<HTMLElement>(
      "#cmd-palette:not(.hidden), #key-help:not(.hidden)",
    );
    if (open) {
      if (event.key === "Escape") {
        event.preventDefault();
        closeModal(`#${open.id}`);
      } else {
        trapModalFocus(open, event);
      }
      return;
    }
    // An auth-owned dialog still blocks shortcuts from reaching the page
    // beneath it. Its own listener handles Tab and Escape settlement.
    if (document.querySelector("#reconnect-dialog:not(.hidden)")) return;
    const route = currentRoute;
    // Escape exits a routed diff even while its file-search control owns focus.
    // Other text-control keystrokes remain native and never trigger page keys.
    if (route?.kind === "diff" && event.key === "Escape") {
      event.preventDefault();
      actions.navigate(revisionRouteFromDiff(route));
      return;
    }
    if (isTextControl(event.target)) return;
    if (
      ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") ||
      (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "p")
    ) {
      event.preventDefault();
      openPalette();
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (!route) return;
    if (route.kind === "diff") {
      // The routed diff owns ]/[ and n/p. Controls inside the navigator/body
      // keep their native keyboard behavior, including button activation.
      if (isNativeActionControl(event.target)) return;
      if (event.key === "]" || event.key === "[") {
        const next = moveDiffTarget(
          Array.from(
            document.querySelectorAll<HTMLElement>("#diff-page-body .dfile"),
          ),
          route.focus?.filePath,
          event.key === "]" ? 1 : -1,
          (item) => item.dataset.filePath,
        );
        const filePath = next?.dataset.filePath;
        if (filePath) {
          event.preventDefault();
          actions.navigate(refineDiffFocus(route, { filePath }));
        }
        return;
      }
      if (event.key === "n" || event.key === "p") {
        const seen = new Set<string>();
        const facts = Array.from(
          document.querySelectorAll<HTMLElement>("#diff-page-body [data-anno]"),
        ).filter((item) => {
          const id = item.dataset.anno;
          if (!id || seen.has(id)) return false;
          seen.add(id);
          return true;
        });
        const next = moveDiffTarget(
          facts,
          route.focus?.factId,
          event.key === "n" ? 1 : -1,
          (item) => item.dataset.anno,
        );
        const factId = next?.dataset.anno;
        if (factId) {
          event.preventDefault();
          actions.navigate(refineDiffFocus(route, { factId }));
        }
        return;
      }
      return;
    }
    if (event.key === "?") {
      event.preventDefault();
      openModal("#key-help", document.querySelector("#key-help-close"));
      return;
    }
    // A narrow exact detail is a modal sheet over an inert master surface.
    // Master shortcuts must not operate or focus that covered surface until
    // the reader closes the sheet or widens the viewport.
    if (
      MASTER_SURFACE_KEYS.has(event.key) &&
      narrowDetailOwnsFocus(event.target)
    ) {
      return;
    }
    if (event.key === "/") {
      event.preventDefault();
      document.querySelector<HTMLInputElement>("#filter-text")?.focus();
      return;
    }
    const timelineRoute = companionTimelineRoute(route);
    if (timelineRoute !== null) {
      const timeline = timelineWindow(
        currentTimelineEventIds,
        selectedTimelineEventId,
      );
      if (
        (event.key === "ArrowDown" || event.key === "ArrowUp") &&
        !isTimelineListTarget(event.target)
      ) {
        return;
      }
      const applyScopedTimelineIntent = (
        intent: TimelineSelectionIntent | null,
      ): boolean => {
        // An exact event is a stable detail route. Its local cursor can move
        // across the loaded page, but only Enter deliberately replaces the
        // detail. Page-edge navigation remains a Timeline-lens operation.
        if (route.kind === "event" && intent?.kind === "adjacent-page") {
          return false;
        }
        return applyTimelineIntent(intent);
      };
      if (event.key === "j" || event.key === "ArrowDown") {
        if (applyScopedTimelineIntent(moveTimelineSelection(timeline, 1))) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "k" || event.key === "ArrowUp") {
        if (applyScopedTimelineIntent(moveTimelineSelection(timeline, -1))) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "g") {
        // g/G retain their global Timeline meaning. An exact-event detail does
        // not own the companion-page state needed to traverse globally without
        // replacing that detail, so it deliberately leaves them unhandled.
        if (
          route.kind === "timeline" &&
          applyGlobalTimelineBoundary("first", timelineRoute)
        ) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "G") {
        if (
          route.kind === "timeline" &&
          applyGlobalTimelineBoundary("last", timelineRoute)
        ) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "f") {
        if (
          applyScopedTimelineIntent(
            pageTimelineSelection(
              timeline,
              "forward",
              visibleTimelineRows(),
              "full",
            ),
          )
        ) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "b") {
        if (
          applyScopedTimelineIntent(
            pageTimelineSelection(
              timeline,
              "backward",
              visibleTimelineRows(),
              "full",
            ),
          )
        ) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "d" || event.key === "u") {
        if (
          applyScopedTimelineIntent(
            pageTimelineSelection(
              timeline,
              event.key === "d" ? "forward" : "backward",
              visibleTimelineRows(),
              "half",
            ),
          )
        ) {
          event.preventDefault();
        }
        return;
      }
      // Lowercase f retains its historical full-page meaning. Shift-F is the
      // non-conflicting, deliberate follow/park toggle; the visible Follow
      // button remains the discoverable primary control.
      if (event.key === "F") {
        if (route.kind === "timeline") {
          event.preventDefault();
          actions.toggleTimelineMonitoring?.();
        }
        return;
      }
      if (
        event.key === "Enter" &&
        selectedTimelineEventId !== null &&
        !isNativeActionControl(event.target)
      ) {
        event.preventDefault();
        actions.navigate({
          kind: "event",
          eventId: selectedTimelineEventId,
          historyQuery: timelineRoute.historyQuery,
          query: {},
        });
        return;
      }
    }
    if (event.key === "j" || event.key === "ArrowDown") {
      event.preventDefault();
      selectedChangeId = moveSelection(selectedChangeId, 1);
      return;
    }
    if (event.key === "k" || event.key === "ArrowUp") {
      event.preventDefault();
      selectedChangeId = moveSelection(selectedChangeId, -1);
      return;
    }
    if (event.key === "g") {
      event.preventDefault();
      selectedChangeId = moveSelectionToBoundary(selectedChangeId, "first");
      return;
    }
    if (event.key === "G") {
      event.preventDefault();
      selectedChangeId = moveSelectionToBoundary(selectedChangeId, "last");
      return;
    }
    if (event.key === "1") {
      event.preventDefault();
      actions.navigate({ kind: "timeline", historyQuery: {} });
      return;
    }
    if (event.key === "2") {
      event.preventDefault();
      actions.navigate({
        kind: "lens",
        lens: "changes",
        query:
          route.kind === "timeline" ? {} : { ...route.query, after: undefined },
      });
      return;
    }
    if (event.key === "3") {
      event.preventDefault();
      actions.navigate({
        kind: "lens",
        lens: "attention",
        query:
          route.kind === "timeline" ? {} : { ...route.query, after: undefined },
      });
      return;
    }
    if (
      event.key === "Enter" &&
      selectedChangeId &&
      !isNativeActionControl(event.target)
    ) {
      event.preventDefault();
      actions.navigate({
        kind: "change",
        changeId: selectedChangeId,
        query: queryForExactNavigation(route),
      });
      return;
    }
    if (event.key === "h") {
      event.preventDefault();
      updateSplit((preferredSplit() ?? 50) - 5);
      return;
    }
    if (event.key === "l") {
      event.preventDefault();
      updateSplit((preferredSplit() ?? 50) + 5);
      return;
    }
    if (
      event.key === "Escape" &&
      route.kind !== "lens" &&
      route.kind !== "timeline"
    ) {
      event.preventDefault();
      actions.navigate(listRoute(route));
    }
  };
  document.addEventListener("keydown", onKey);

  const onClose = () => {
    const route = currentRoute;
    if (route) actions.navigate(listRoute(route));
  };
  const closeButton =
    document.querySelector<HTMLButtonElement>("#detail-close");
  const backButton = document.querySelector<HTMLButtonElement>("#detail-back");
  if (closeButton) closeButton.onclick = onClose;
  // Replace the shell callback so an exact surface returns to its originating
  // lens with the same bounded query instead of a generic timeline route.
  if (backButton) backButton.onclick = onClose;

  const stop = () => {
    document.removeEventListener("click", onClick);
    document.removeEventListener("focusin", onFocusIn);
    document.removeEventListener("scroll", onTimelineScroll, true);
    document.removeEventListener("keydown", onKey);
    timelineDomObserver.disconnect();
    document
      .querySelectorAll<HTMLInputElement>("input[name='theme-mode']")
      .forEach((input) => {
        input.removeEventListener("change", changeTheme);
      });
    document
      .querySelectorAll<HTMLInputElement>("input[name='density-mode']")
      .forEach((input) => {
        input.removeEventListener("change", changeDensity);
      });
    helpCloseButton?.removeEventListener("click", helpClose);
    readingButton?.removeEventListener("click", toggleReading);
    masterRail?.removeEventListener("click", restoreMaster);
    divider?.removeEventListener("keydown", onDividerKey);
    divider?.removeEventListener("pointerdown", onDividerPointerDown);
    divider?.removeEventListener("pointermove", onDividerPointerMove);
    divider?.removeEventListener("pointerup", finishDividerDrag);
    divider?.removeEventListener("pointercancel", finishDividerDrag);
    divider?.removeEventListener(
      "lostpointercapture",
      onDividerLostPointerCapture,
    );
    divider?.removeEventListener("dblclick", onDividerDoubleClick);
    window.removeEventListener("resize", onViewportResize);
    if (
      divider &&
      activeDividerPointerId !== null &&
      divider.hasPointerCapture?.(activeDividerPointerId)
    ) {
      divider.releasePointerCapture?.(activeDividerPointerId);
    }
    activeDividerPointerId = null;
    divider?.classList.remove("dragging");
    paletteInput?.removeEventListener("input", renderPaletteResults);
    if (closeButton?.onclick === onClose) closeButton.onclick = null;
    if (backButton?.onclick === onClose) backButton.onclick = null;
    document.querySelector("#cmd-palette")?.classList.add("hidden");
    document.querySelector("#key-help")?.classList.add("hidden");
    paletteResults?.replaceChildren();
    selectedChangeId = null;
    selectedTimelineEventId = null;
    currentTimelineEventIds = [];
    pendingTimelineSelection = null;
    pendingGlobalTimelineSelection = null;
    setSelected(null);
    setTimelineSelected(null);
    modalReturnFocus = null;
    detailReturnFocus = null;
    detailWasOpen = false;
    currentRoute = null;
    exactOriginLens = null;
    timelineOriginRoute = null;
    detailDomIdentity = null;
    pendingDiffEntryFocus = null;
    pendingDiffExitFocus = null;
    setCoveredPageInert(false);
  };
  return {
    sync(snapshot, timelinePage = snapshot.generation?.history ?? null) {
      const nextRoute =
        snapshot.route.kind === "invalid" ? null : snapshot.route;
      currentTimelineEventIds =
        companionTimelineRoute(nextRoute) !== null && timelinePage !== null
          ? timelinePage.entries.map((entry) => entry.eventId)
          : [];
      if (
        nextRoute?.kind === "event" &&
        (currentRoute?.kind !== "event" ||
          formatChangeInspectorRoute(currentRoute) !==
            formatChangeInspectorRoute(nextRoute))
      ) {
        selectedTimelineEventId = nextRoute.eventId;
      }
      if (
        nextRoute !== null &&
        nextRoute.kind !== "lens" &&
        nextRoute.kind !== "timeline"
      ) {
        const persistedOrigin = historyOrigin(nextRoute);
        const origin =
          persistedOrigin ??
          (currentRoute?.kind === "lens"
            ? currentRoute.lens
            : currentRoute?.kind === "timeline"
              ? "timeline"
              : (exactOriginLens ?? "timeline"));
        if (currentRoute?.kind === "timeline") {
          // An exact Revision is deliberately a different route. Retain the
          // originating Timeline route locally so Escape, Back, and Close
          // preserve its server-owned filters and opaque continuation token.
          timelineOriginRoute = currentRoute;
        }
        exactOriginLens = origin;
        if (persistedOrigin === null) persistHistoryOrigin(nextRoute, origin);
      } else {
        exactOriginLens = null;
      }
      const cards = Array.from(
        document.querySelectorAll<HTMLElement>(".unit-card[data-change-id]"),
      );
      if (!cards.some((card) => card.dataset.changeId === selectedChangeId))
        selectedChangeId = null;
      setSelected(selectedChangeId);
      if (companionTimelineRoute(nextRoute) !== null) {
        syncTimelineDom(nextRoute);
      } else {
        selectedTimelineEventId = null;
        pendingTimelineSelection = null;
        pendingGlobalTimelineSelection = null;
      }
      // Route state, not viewport CSS, owns whether detail is interactive.
      // A lens route keeps the retained narrow sheet off-canvas for animation,
      // but `inert` and `aria-hidden` remove it from keyboard/AT navigation.
      // Entering an exact route captures the list opener once; same-route poll
      // paints do not churn focus. Returning to a lens restores that opener (or
      // the programmatic master fallback if a generation replaced it).
      const detailOpen =
        snapshot.route.kind !== "lens" &&
        snapshot.route.kind !== "timeline" &&
        snapshot.route.kind !== "invalid";
      const detail = document.querySelector<HTMLElement>("#detail");
      const viewportIsNarrow = window.matchMedia("(max-width: 760px)").matches;
      const coveredPage = detailOpen && viewportIsNarrow;
      if (!coveredPage) setCoveredPageInert(false);
      const nextDetailDomIdentity =
        document.querySelector<HTMLElement>("#detail-body")?.firstChild ?? null;
      const routeSurface =
        nextRoute?.kind === "diff"
          ? document.querySelector<HTMLElement>("#diff-page")
          : detail;
      const detailDomChanged = detailDomIdentity !== nextDetailDomIdentity;
      const active =
        document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null;
      const detailRouteChanged =
        currentRoute !== null &&
        currentRoute.kind !== "lens" &&
        currentRoute.kind !== "timeline" &&
        nextRoute !== null &&
        nextRoute.kind !== "lens" &&
        nextRoute.kind !== "timeline" &&
        formatChangeInspectorRoute(currentRoute) !==
          formatChangeInspectorRoute(nextRoute);
      const nextDiffIdentity =
        nextRoute?.kind === "diff" ? diffIdentity(nextRoute) : null;
      const currentDiffIdentity =
        currentRoute?.kind === "diff" ? diffIdentity(currentRoute) : null;
      if (nextDiffIdentity === null) {
        pendingDiffEntryFocus = null;
      } else if (!sameDiffIdentity(currentDiffIdentity, nextDiffIdentity)) {
        pendingDiffEntryFocus = nextDiffIdentity;
      }
      const entersVisibleDiff =
        nextRoute?.kind === "diff" &&
        sameDiffIdentity(pendingDiffEntryFocus, nextDiffIdentity) &&
        document.querySelector("#diff-page:not(.hidden)") !== null;
      const leavesDiffForExactSurface =
        currentRoute?.kind === "diff" &&
        nextRoute !== null &&
        nextRoute.kind !== "diff" &&
        nextRoute.kind !== "lens" &&
        nextRoute.kind !== "timeline";
      const leavesDiffForRevision =
        leavesDiffForExactSurface && nextRoute?.kind === "revision";
      const nextRevisionRoute =
        nextRoute?.kind === "revision"
          ? formatChangeInspectorRoute(nextRoute)
          : null;
      if (leavesDiffForRevision) {
        pendingDiffExitFocus = nextRevisionRoute;
      } else if (pendingDiffExitFocus !== nextRevisionRoute) {
        pendingDiffExitFocus = null;
      }
      const completesDiffExitFocus =
        pendingDiffExitFocus !== null &&
        pendingDiffExitFocus === nextRevisionRoute &&
        document
          .querySelector<HTMLElement>("#detail-body")
          ?.dataset.changeReadingKey?.startsWith(`${pendingDiffExitFocus}:`) ===
          true;
      document
        .querySelector(".split")
        ?.classList.toggle("split-closed", !detailOpen);
      if (detail) {
        detail.inert = !detailOpen;
        if (detailOpen) detail.removeAttribute("aria-hidden");
        else detail.setAttribute("aria-hidden", "true");
      }
      if (entersVisibleDiff) {
        pendingDiffEntryFocus = null;
        // The full-frame diff replaces the split reader. Put focus on its
        // visible Back control rather than leaving it on the now-hidden exact
        // Revision action that opened the diff.
        focusFallback(nextRoute);
      } else if (leavesDiffForExactSurface) {
        // The diff has just been hidden and its search/body nodes may still
        // be connected. Settle on retained exact-reader chrome immediately,
        // then keep that handoff pending if the exact reading is still loading.
        if (completesDiffExitFocus) pendingDiffExitFocus = null;
        focusFallback(nextRoute);
      } else if (completesDiffExitFocus) {
        // Exact hydration can deliberately focus a retained routed fact or
        // file after the loading paint. Complete the one-time diff-exit
        // handoff on stable exact-reader chrome, then return focus ownership
        // to ordinary same-route renderer refinements.
        pendingDiffExitFocus = null;
        focusFallback(nextRoute);
      } else if (detailOpen && !detailWasOpen) {
        detailReturnFocus = active && active !== document.body ? active : null;
        if (viewportIsNarrow && nextRoute?.kind !== "diff") {
          document
            .querySelector<HTMLButtonElement>("#detail-back")
            ?.focus({ preventScroll: true });
        }
      } else if (
        detailOpen &&
        detailWasOpen &&
        detailRouteChanged &&
        nextRoute?.kind !== "diff" &&
        viewportIsNarrow &&
        detail !== null &&
        (active === null || !detail.contains(active)) &&
        (active === null || active.closest(".modal:not(.hidden)") === null)
      ) {
        // A different exact route enters a new reading surface. At the narrow
        // breakpoint that surface is a fixed sheet, so connected focus left
        // on the covered master or toolbar must move into retained sheet
        // chrome. Same-route poll paints and visible modals retain focus.
        focusFallback(nextRoute);
      } else if (
        detailOpen &&
        detailWasOpen &&
        detailDomChanged &&
        (!(document.activeElement instanceof HTMLElement) ||
          document.activeElement === document.body ||
          !document.activeElement.isConnected)
      ) {
        // Exact detail paints may replace the body even when a generation
        // refresh keeps the same route. If the replaced body owned focus,
        // browsers move focus to body; return it to retained detail chrome
        // without overriding a newly focused target or an unchanged paint.
        focusFallback(nextRoute);
      } else if (!detailOpen && detailWasOpen) {
        // Reading mode hides the master pane. Clear it before making the exact
        // detail inert and restoring focus, otherwise an exact -> lens route
        // transition leaves both retained panes hidden.
        setReading(false);
        const candidate =
          detailReturnFocus?.isConnected === true
            ? detailReturnFocus
            : document.querySelector<HTMLElement>("#master");
        detailReturnFocus = null;
        candidate?.focus({ preventScroll: true });
      }
      if (coveredPage) {
        const coveredActive =
          document.activeElement instanceof HTMLElement
            ? document.activeElement
            : null;
        if (
          routeSurface !== null &&
          (coveredActive === null || !routeSurface.contains(coveredActive)) &&
          (coveredActive === null ||
            coveredActive.closest(".modal:not(.hidden)") === null)
        ) {
          focusFallback(nextRoute);
        }
        setCoveredPageInert(true);
      }
      detailWasOpen = detailOpen;
      currentRoute = nextRoute;
      detailDomIdentity = nextDetailDomIdentity;
    },
    stop,
  };
}
