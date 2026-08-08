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
      "button, a[href], [role='button'], [role='link'], [role='separator']",
    ) !== null
  );
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
  let pendingTimelineSelection: Extract<
    TimelineSelectionIntent,
    { kind: "adjacent-page" }
  > | null = null;
  let pendingGlobalTimelineSelection: {
    boundary: "first" | "last";
    route: string;
  } | null = null;
  let modalReturnFocus: HTMLElement | null = null;
  let detailReturnFocus: HTMLElement | null = null;
  let detailWasOpen = false;
  let currentRoute: ValidRoute | null = null;
  let exactOriginLens: "timeline" | "changes" | "attention" | null = null;
  let timelineOriginRoute: Extract<ValidRoute, { kind: "timeline" }> | null =
    null;
  let detailDomIdentity: ChildNode | null = null;

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
    if (!pager) return false;
    pendingTimelineSelection = intent;
    pager.click();
    return true;
  };

  const syncTimelineDom = (route: ValidRoute | null = currentRoute) => {
    const window = timelineWindow(
      currentTimelineEventIds,
      selectedTimelineEventId,
    );
    if (
      pendingGlobalTimelineSelection !== null &&
      route?.kind === "timeline" &&
      formatChangeInspectorRoute(route) ===
        pendingGlobalTimelineSelection.route &&
      window.eventIds.length > 0
    ) {
      selectedTimelineEventId =
        pendingGlobalTimelineSelection.boundary === "first"
          ? (window.eventIds[0] ?? null)
          : (window.eventIds.at(-1) ?? null);
      pendingGlobalTimelineSelection = null;
      if (selectedTimelineEventId !== null) {
        actions.revealTimelineEvent?.(selectedTimelineEventId);
      }
    }
    if (pendingTimelineSelection !== null && window.eventIds.length > 0) {
      selectedTimelineEventId = resolveTimelinePageSelection(
        window.eventIds,
        pendingTimelineSelection,
      );
      pendingTimelineSelection = null;
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
    void navigateBoundary(boundary, route)
      .then((target) => {
        if (target === null) return;
        pendingGlobalTimelineSelection = {
          boundary,
          route: formatChangeInspectorRoute(target),
        };
        // Navigation usually paints after the promise settles. If a test host
        // or cached same-route navigation painted synchronously, resolve the
        // landing against that already-published page now.
        syncTimelineDom();
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
      route !== null && route.kind !== "lens" && route.kind !== "timeline"
        ? window.matchMedia("(max-width: 760px)").matches
          ? document.querySelector<HTMLElement>("#detail-back")
          : document.querySelector<HTMLElement>("#detail-close")
        : document.querySelector<HTMLElement>("#master");
    target?.focus({ preventScroll: true });
  };

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
    if (currentRoute?.kind === "timeline") syncTimelineDom();
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
    const route = currentRoute;
    if (!route) return;
    if (event.key === "?") {
      event.preventDefault();
      openModal("#key-help", document.querySelector("#key-help-close"));
      return;
    }
    if (event.key === "/") {
      event.preventDefault();
      document.querySelector<HTMLInputElement>("#filter-text")?.focus();
      return;
    }
    if (route.kind === "timeline") {
      const timeline = timelineWindow(
        currentTimelineEventIds,
        selectedTimelineEventId,
      );
      if (event.key === "j" || event.key === "ArrowDown") {
        if (applyTimelineIntent(moveTimelineSelection(timeline, 1))) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "k" || event.key === "ArrowUp") {
        if (applyTimelineIntent(moveTimelineSelection(timeline, -1))) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "g") {
        if (applyGlobalTimelineBoundary("first", route)) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "G") {
        if (applyGlobalTimelineBoundary("last", route)) {
          event.preventDefault();
        }
        return;
      }
      if (event.key === "f") {
        if (
          applyTimelineIntent(
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
          applyTimelineIntent(
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
          applyTimelineIntent(
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
        event.preventDefault();
        actions.toggleTimelineMonitoring?.();
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
          historyQuery: route.historyQuery,
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
  };
  return {
    sync(snapshot, timelinePage = snapshot.generation?.history ?? null) {
      const nextRoute =
        snapshot.route.kind === "invalid" ? null : snapshot.route;
      currentTimelineEventIds =
        nextRoute?.kind === "timeline" && timelinePage !== null
          ? timelinePage.entries.map((entry) => entry.eventId)
          : [];
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
      if (nextRoute?.kind === "timeline") {
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
      const nextDetailDomIdentity =
        document.querySelector<HTMLElement>("#detail-body")?.firstChild ?? null;
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
      document
        .querySelector(".split")
        ?.classList.toggle("split-closed", !detailOpen);
      if (detail) {
        detail.inert = !detailOpen;
        if (detailOpen) detail.removeAttribute("aria-hidden");
        else detail.setAttribute("aria-hidden", "true");
      }
      if (detailOpen && !detailWasOpen) {
        detailReturnFocus = active && active !== document.body ? active : null;
        if (viewportIsNarrow) {
          document
            .querySelector<HTMLButtonElement>("#detail-back")
            ?.focus({ preventScroll: true });
        }
      } else if (
        detailOpen &&
        detailWasOpen &&
        detailRouteChanged &&
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
      detailWasOpen = detailOpen;
      currentRoute = nextRoute;
      detailDomIdentity = nextDetailDomIdentity;
    },
    stop,
  };
}
