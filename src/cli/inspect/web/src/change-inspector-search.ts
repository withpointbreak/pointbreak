/**
 * Active Inspector search interaction. The controller owns only an input
 * draft, its suggestion popover, and a bounded replace callback. Timeline
 * completion values come from the accepted Event History document; this
 * module never reads a store, fetches, or constructs semantic detail routes.
 */

import {
  type EventHistoryDocument,
  normalizeChangePageQueryText,
  normalizeEventHistoryQueryText,
} from "./change-protocol";
import { parseSearchQueryFor, type QuerySurface } from "./query";
import { shortRef } from "./refs";
import {
  CHANGE_TIMELINE_QUERY_FIELDS,
  EVENT_QUERY_FIELDS,
  REVISION_QUERY_FIELDS,
} from "./types";

const SEARCH_DEBOUNCE_MS = 150;

type SearchCompletion = EventHistoryDocument["completion"];
type SearchSurface = QuerySurface | "plain";

export interface ChangeInspectorSearchContext {
  surface: SearchSurface;
  completion: SearchCompletion | null;
  committedQuery: string;
  queryNotices: readonly string[];
  routeDiagnostic: string | null;
  routeKey: string;
}

export interface ChangeInspectorSearchOptions {
  input: HTMLInputElement;
  list: HTMLElement;
  readContext(): ChangeInspectorSearchContext;
  applyQuery(query: string | undefined): void;
  focusTimeline(): void;
}

export interface ChangeInspectorSearchController {
  /** Reconcile controller-owned draft chrome after the composition repaints. */
  sync(): void;
  stop(): void;
}

interface SearchSuggestion {
  insertText: string;
  label: string;
  title?: string;
  accessibleName?: string;
}

function fieldsFor(surface: SearchSurface): readonly string[] {
  if (surface === "plain") return [];
  if (surface === "revision") return REVISION_QUERY_FIELDS;
  if (surface === "change-timeline") return CHANGE_TIMELINE_QUERY_FIELDS;
  return EVENT_QUERY_FIELDS;
}

function activeToken(filterText: string): string {
  if (/\s$/.test(filterText)) return "";
  return filterText.match(/\S+$/)?.[0] ?? "";
}

function canonicalSuggestionField(field: string): string {
  return field === "rev" ? "revision" : field;
}

function unique(values: readonly string[]): string[] {
  return [...new Set(values)];
}

function completionValues(
  field: string,
  completion: SearchCompletion | null,
): string[] {
  switch (field) {
    case "type":
      return completion?.eventTypes ?? [];
    case "track":
      return completion?.trackIds ?? [];
    case "change":
      return completion?.changeIds ?? [];
    case "revision":
      // `revision:` is a Revision-ID set query. Multiple exact refs with the
      // same ID intentionally produce one clause candidate; artifact hashes
      // remain available only to the separate exact outer filter.
      return unique([
        ...(completion?.revisionRefs.map((reference) =>
          String(reference.revisionId),
        ) ?? []),
        ...(completion?.unresolvedRevisionIds ?? []),
      ]);
    default:
      // The Event History completion contract does not provide actor or tag
      // values. Their keys remain discoverable without inventing candidates.
      return [];
  }
}

function suggestionForValue(
  typedField: string,
  canonicalField: string,
  value: string,
  negate: boolean,
): SearchSuggestion {
  const prefix = negate ? "-" : "";
  const insertText = `${prefix}${typedField}:${value}`;
  if (canonicalField !== "change" && canonicalField !== "revision") {
    return { insertText, label: insertText };
  }
  const visible = `${prefix}${typedField}:${shortRef(value)}`;
  const kind = canonicalField === "change" ? "Change ID" : "Revision ID";
  return {
    insertText,
    label: visible,
    title: `${typedField}:${value}`,
    accessibleName: `Use ${typedField} filter for ${kind} ${value}`,
  };
}

function suggestionsFor(
  filterText: string,
  context: ChangeInspectorSearchContext,
): SearchSuggestion[] {
  let token = activeToken(filterText);
  if (!token) return [];
  const negate = token.startsWith("-") && token.length > 1;
  if (negate) token = token.slice(1);
  const colon = token.indexOf(":");
  if (colon < 0) {
    const prefix = token.toLowerCase();
    return fieldsFor(context.surface)
      .filter((field) => field.startsWith(prefix))
      .map((field) => {
        const insertText = `${negate ? "-" : ""}${field}:`;
        return { insertText, label: insertText };
      });
  }

  const typedField = token.slice(0, colon).toLowerCase();
  const canonicalField = canonicalSuggestionField(typedField);
  if (!fieldsFor(context.surface).includes(canonicalField)) return [];
  const valuePrefix = token
    .slice(colon + 1)
    .replace(/^"/, "")
    .toLowerCase();
  return completionValues(canonicalField, context.completion)
    .filter((value) => value.toLowerCase().includes(valuePrefix))
    .map((value) =>
      suggestionForValue(typedField, canonicalField, value, negate),
    );
}

function acceptSuggestion(filterText: string, insertText: string): string {
  const token = activeToken(filterText);
  const trailing = insertText.endsWith(":") ? "" : " ";
  if (!token) return `${filterText}${insertText}${trailing}`;
  return `${filterText.slice(0, filterText.length - token.length)}${insertText}${trailing}`;
}

function incompleteFieldKey(
  filterText: string,
  surface: SearchSurface,
): string | null {
  let token = activeToken(filterText);
  if (token.startsWith("-") && token.length > 1) token = token.slice(1);
  const match = token.match(/^([a-z]+):"?$/i);
  if (!match) return null;
  const typedField = match[1]?.toLowerCase() ?? "";
  const canonicalField = canonicalSuggestionField(typedField);
  return fieldsFor(surface).includes(canonicalField) ? typedField : null;
}

function incompleteIdentityKey(
  filterText: string,
  surface: SearchSurface,
): string | null {
  const typedField = incompleteFieldKey(filterText, surface);
  if (typedField === null) return null;
  const canonicalField = canonicalSuggestionField(typedField);
  return canonicalField === "change" || canonicalField === "revision"
    ? typedField
    : null;
}

function localDiagnostics(
  filterText: string,
  context: ChangeInspectorSearchContext,
): { messages: string[]; fatal: boolean } {
  const parsed =
    context.surface === "plain"
      ? { diagnostics: [] }
      : parseSearchQueryFor(filterText, context.surface);
  const incompleteKey = incompleteIdentityKey(filterText, context.surface);
  const diagnostics = parsed.diagnostics.filter(
    (diagnostic) =>
      !(
        diagnostic.code === "unsupported-value" &&
        incompleteKey !== null &&
        diagnostic.key === incompleteKey
      ),
  );
  const messages = diagnostics.map((diagnostic) => diagnostic.message);
  let fatal = diagnostics.some(
    (diagnostic) => diagnostic.code !== "deprecated-qualifier",
  );
  if (normalizedQuery(filterText)) {
    try {
      if (context.surface === "change-timeline") {
        normalizeEventHistoryQueryText(filterText);
      } else {
        normalizeChangePageQueryText(filterText);
      }
    } catch (error) {
      messages.push(error instanceof Error ? error.message : String(error));
      fatal = true;
    }
  }
  return { messages, fatal };
}

function normalizedQuery(value: string): string {
  return value.trim();
}

/** Install the active search controller over the static Inspector shell. */
export function installChangeInspectorSearch(
  options: ChangeInspectorSearchOptions,
): ChangeInspectorSearchController {
  const { input, list } = options;
  let activeIndex = -1;
  let paintedSuggestions: SearchSuggestion[] = [];
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let draftValue: string | null = null;
  let draftBaseRouteKey: string | null = null;
  let retainedTimelineCompletion: SearchCompletion | null = null;
  let suggestionsRequested = false;

  input.setAttribute("role", "combobox");
  input.setAttribute("aria-autocomplete", "list");
  input.setAttribute("aria-controls", list.id);
  input.setAttribute("aria-expanded", "false");
  if (!input.hasAttribute("aria-label")) {
    input.setAttribute("aria-label", "Search Inspector");
  }
  list.setAttribute("role", "listbox");
  if (!list.hasAttribute("aria-label")) {
    list.setAttribute("aria-label", "Search suggestions");
  }

  const cancelDebounce = () => {
    if (debounceTimer !== null) clearTimeout(debounceTimer);
    debounceTimer = null;
  };

  const readContext = (): ChangeInspectorSearchContext => {
    const context = options.readContext();
    if (context.surface !== "change-timeline") {
      retainedTimelineCompletion = null;
      return context;
    }
    if (context.completion !== null) {
      retainedTimelineCompletion = context.completion;
      return context;
    }
    // A query replace deliberately clears the mounted generation before its
    // bounded successor read completes. Retain only the last accepted
    // Timeline vocabulary across that gap so consecutive terms stay
    // completable; a stale candidate can at worst form an honest zero-match
    // set query and can never become an exact route.
    return {
      ...context,
      completion: retainedTimelineCompletion,
    };
  };

  const clearSuggestions = () => {
    activeIndex = -1;
    paintedSuggestions = [];
    list.replaceChildren();
    list.classList.add("hidden");
    input.setAttribute("aria-expanded", "false");
    input.removeAttribute("aria-activedescendant");
  };

  const dismiss = () => {
    suggestionsRequested = false;
    clearSuggestions();
  };

  const updateActive = () => {
    const options = list.querySelectorAll<HTMLElement>("[role='option']");
    options.forEach((option, index) => {
      const active = index === activeIndex;
      option.classList.toggle("suggestion-active", active);
      option.setAttribute("aria-selected", String(active));
    });
    const active = options[activeIndex];
    if (active) input.setAttribute("aria-activedescendant", active.id);
    else input.removeAttribute("aria-activedescendant");
  };

  const paintSuggestions = (
    context: ChangeInspectorSearchContext,
    preserveActive = false,
  ) => {
    const activeInsertText = preserveActive
      ? paintedSuggestions[activeIndex]?.insertText
      : undefined;
    const suggestions = suggestionsFor(input.value, context);
    if (suggestions.length === 0) {
      clearSuggestions();
      return;
    }
    activeIndex =
      activeInsertText === undefined
        ? -1
        : suggestions.findIndex(
            (suggestion) => suggestion.insertText === activeInsertText,
          );
    paintedSuggestions = suggestions;
    list.replaceChildren(
      ...suggestions.map((suggestion, index) => {
        const option = document.createElement("li");
        option.id = `filter-suggestion-${index}`;
        option.className = "suggestion";
        option.dataset.index = String(index);
        option.setAttribute("role", "option");
        option.setAttribute("aria-selected", "false");
        option.textContent = suggestion.label;
        if (suggestion.title) option.title = suggestion.title;
        if (suggestion.accessibleName) {
          option.setAttribute("aria-label", suggestion.accessibleName);
        }
        return option;
      }),
    );
    list.classList.remove("hidden");
    input.setAttribute("aria-expanded", "true");
    updateActive();
  };

  const paintDiagnostics = (
    context: ChangeInspectorSearchContext,
    localMessages: readonly string[],
    fatal: boolean,
  ) => {
    if (context.routeDiagnostic !== null) return;
    const routeDiagnostic =
      document.querySelector<HTMLElement>("#route-diagnostic");
    if (!routeDiagnostic) return;
    const draftMatchesCommitted =
      normalizedQuery(input.value) === normalizedQuery(context.committedQuery);
    const messages = [
      ...localMessages,
      ...(draftMatchesCommitted ? context.queryNotices : []),
    ].filter((message, index, all) => all.indexOf(message) === index);
    routeDiagnostic.textContent = messages.join(" · ");
    routeDiagnostic.classList.toggle("hidden", messages.length === 0);
    if (messages.length > 0) {
      input.setAttribute("aria-describedby", "route-diagnostic");
    } else {
      input.removeAttribute("aria-describedby");
    }
    if (fatal) input.setAttribute("aria-invalid", "true");
    else input.removeAttribute("aria-invalid");
  };

  const applyNow = (value: string) => {
    cancelDebounce();
    const context = readContext();
    const local = localDiagnostics(value, context);
    paintDiagnostics(context, local.messages, local.fatal);
    if (local.fatal || incompleteFieldKey(value, context.surface) !== null) {
      return;
    }
    options.applyQuery(normalizedQuery(value) || undefined);
  };

  const scheduleApply = (
    context: ChangeInspectorSearchContext,
    value: string,
  ) => {
    cancelDebounce();
    const scheduledRouteKey = context.routeKey;
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      if (readContext().routeKey !== scheduledRouteKey) return;
      applyNow(value);
    }, SEARCH_DEBOUNCE_MS);
  };

  const accept = (suggestion: SearchSuggestion) => {
    const next = acceptSuggestion(input.value, suggestion.insertText);
    draftValue = next;
    draftBaseRouteKey = readContext().routeKey;
    input.value = next;
    if (suggestion.insertText.endsWith(":")) {
      suggestionsRequested = true;
      const context = readContext();
      const local = localDiagnostics(next, context);
      paintSuggestions(context);
      paintDiagnostics(context, local.messages, local.fatal);
      input.focus({ preventScroll: true });
      input.setSelectionRange(next.length, next.length);
      return;
    }
    dismiss();
    applyNow(next);
    // A replace navigation may synchronously repaint the shell. The accepted
    // trailing space is still a useful local draft for the reader's next term.
    input.value = next;
    input.focus({ preventScroll: true });
    input.setSelectionRange(next.length, next.length);
  };

  const onInput = () => {
    cancelDebounce();
    const context = readContext();
    draftValue = input.value;
    draftBaseRouteKey = context.routeKey;
    suggestionsRequested = true;
    const local = localDiagnostics(input.value, context);
    paintSuggestions(context);
    paintDiagnostics(context, local.messages, local.fatal);
    if (
      local.fatal ||
      incompleteFieldKey(input.value, context.surface) !== null
    ) {
      return;
    }
    scheduleApply(context, input.value);
  };

  const onChange = () => {
    const context = readContext();
    draftValue = input.value;
    draftBaseRouteKey = context.routeKey;
    const local = localDiagnostics(input.value, context);
    paintDiagnostics(context, local.messages, local.fatal);
    if (
      local.fatal ||
      incompleteFieldKey(input.value, context.surface) !== null ||
      normalizedQuery(input.value) ===
        normalizedQuery(context.committedQuery) ||
      debounceTimer !== null
    ) {
      return;
    }
    scheduleApply(context, input.value);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const open = !list.classList.contains("hidden");
    if (open && event.key === "ArrowDown") {
      event.preventDefault();
      activeIndex = Math.min(paintedSuggestions.length - 1, activeIndex + 1);
      updateActive();
      return;
    }
    if (open && event.key === "ArrowUp") {
      event.preventDefault();
      activeIndex = Math.max(0, activeIndex - 1);
      updateActive();
      return;
    }
    if (
      open &&
      (event.key === "Enter" || event.key === "Tab") &&
      activeIndex >= 0
    ) {
      event.preventDefault();
      event.stopPropagation();
      const suggestion = paintedSuggestions[activeIndex];
      if (suggestion) accept(suggestion);
      return;
    }
    if (open && event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      applyNow(input.value);
      dismiss();
      options.focusTimeline();
    }
  };

  const onBlur = (event: FocusEvent) => {
    if (
      event.relatedTarget instanceof Node &&
      list.contains(event.relatedTarget)
    ) {
      return;
    }
    dismiss();
  };

  const onListMouseDown = (event: MouseEvent) => {
    event.preventDefault();
  };

  const onListClick = (event: MouseEvent) => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const option = target.closest<HTMLElement>("[data-index]");
    const index = Number(option?.dataset.index);
    if (!Number.isInteger(index)) return;
    const suggestion = paintedSuggestions[index];
    if (suggestion) accept(suggestion);
  };

  const onDocumentClick = (event: MouseEvent) => {
    if (list.classList.contains("hidden")) return;
    if (
      event.target instanceof Node &&
      (input.contains(event.target) || list.contains(event.target))
    ) {
      return;
    }
    dismiss();
  };

  input.addEventListener("input", onInput);
  input.addEventListener("change", onChange);
  input.addEventListener("keydown", onKeyDown);
  input.addEventListener("blur", onBlur);
  list.addEventListener("mousedown", onListMouseDown);
  list.addEventListener("click", onListClick);
  document.addEventListener("click", onDocumentClick, true);

  return {
    sync() {
      const context = readContext();
      if (draftValue !== null) {
        const draftStillBelongs =
          normalizedQuery(draftValue) ===
            normalizedQuery(context.committedQuery) ||
          draftBaseRouteKey === context.routeKey;
        if (draftStillBelongs) input.value = draftValue;
        else {
          cancelDebounce();
          draftValue = null;
          draftBaseRouteKey = null;
          dismiss();
        }
      }
      const local = localDiagnostics(input.value, context);
      paintDiagnostics(context, local.messages, local.fatal);
      if (draftValue !== null && suggestionsRequested) {
        paintSuggestions(context, true);
      }
    },
    stop() {
      cancelDebounce();
      dismiss();
      input.removeEventListener("input", onInput);
      input.removeEventListener("change", onChange);
      input.removeEventListener("keydown", onKeyDown);
      input.removeEventListener("blur", onBlur);
      list.removeEventListener("mousedown", onListMouseDown);
      list.removeEventListener("click", onListClick);
      document.removeEventListener("click", onDocumentClick, true);
    },
  };
}
