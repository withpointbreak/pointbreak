// Search-bar autocomplete: pure suggestion logic over the surface key sets and
// per-key value vocabularies, plus the DOM wiring for the suggestion popover
// (the shape of overlay.ts: module-local ephemeral state, an `initControls()`
// the composition root calls once). The per-surface key sets are the single
// key-list authority (types.ts); the value vocabularies are the closed enums
// where one exists and the store-wide distinct values otherwise.

import { CLASS, suggestionClass } from "./classNames";
import { $ } from "./dom";
import { escapeHtml } from "./escape";
import { presentTypes } from "./model";
import type { QuerySurface } from "./query";
import { navigate } from "./router";
import type { DistinctValues } from "./store";
import { getState } from "./store";
import {
  ASSESSMENT_LABELS,
  CHANGE_TIMELINE_QUERY_FIELDS,
  EVENT_QUERY_FIELDS,
  REVISION_ATTENTION_VALUES,
  REVISION_QUERY_FIELDS,
  TYPES,
} from "./types";

export interface Suggestion {
  insertText: string;
  label: string;
}

const CHECK_VALUES = ["passed", "failed", "errored", "skipped"];
// The is: value sets differ per surface — event has only the request-lifecycle
// pair; revision has the full rollup set. They mirror the surface parser's own
// closed-set validation (query.ts), which keeps them module-private.
const EVENT_IS_VALUES = ["open", "answered"];
const REVISION_IS_VALUES = [
  "open",
  "answered",
  "unassessed",
  "stale",
  "follow-up",
  "contested",
  "superseded",
];

function keysFor(surface: QuerySurface): readonly string[] {
  return surface === "revision"
    ? REVISION_QUERY_FIELDS
    : surface === "change-timeline"
      ? CHANGE_TIMELINE_QUERY_FIELDS
      : EVENT_QUERY_FIELDS;
}

function valuesForKey(
  field: string,
  surface: QuerySurface,
  distinct: DistinctValues,
  presentTypeIds?: ReadonlySet<string>,
): readonly string[] {
  switch (field) {
    case "type": {
      // Scoped to the types actually present when the caller knows them (the
      // facet menu's own authority) — offering `type:init` against a store
      // with no init events completes a clause that can only match nothing.
      // An unknown present id is offered raw: the grammar accepts wire ids.
      if (!presentTypeIds) return TYPES.map((t) => t.label);
      const labels = TYPES.filter((t) => presentTypeIds.has(t.id)).map(
        (t) => t.label,
      );
      for (const id of presentTypeIds) {
        if (!TYPES.some((t) => t.id === id)) labels.push(id);
      }
      return labels;
    }
    case "track":
      return distinct.track;
    case "actor":
      return distinct.actor;
    case "tag":
      return distinct.tag;
    case "check":
      return CHECK_VALUES;
    case "assessment":
      return Object.keys(ASSESSMENT_LABELS);
    case "is":
      return surface === "revision" ? REVISION_IS_VALUES : EVENT_IS_VALUES;
    case "attention":
      // Reached only on the revision surface: `attention` is absent from
      // EVENT_QUERY_FIELDS, so `keysFor("event")` already rejects it before
      // this switch ever runs — no surface check needed here.
      return REVISION_ATTENTION_VALUES;
    default:
      return [];
  }
}

/**
 * Suggestions for the token currently being typed at the END of `filterText`
 * (the common case — typing a new qualifier last; a mid-string cursor edit is
 * out of scope). Returns `[]` when the trailing token is empty or names a key
 * `parseSearchQueryFor` wouldn't recognize for `surface`.
 */
export function suggestionsFor(
  filterText: string,
  surface: QuerySurface,
  distinct: DistinctValues,
  presentTypeIds?: ReadonlySet<string>,
): Suggestion[] {
  const tokens = filterText.split(/\s+/);
  const active = tokens[tokens.length - 1] ?? "";
  if (!active) return [];
  const colon = active.indexOf(":");
  if (colon < 0) {
    const prefix = active.toLowerCase();
    return keysFor(surface)
      .filter((k) => k.startsWith(prefix))
      .map((k) => ({ insertText: `${k}:`, label: `${k}:` }));
  }
  const field = active.slice(0, colon).toLowerCase();
  // Strip a leading quote so a partially-typed quoted value (`actor:"git-n`)
  // still completes against the unquoted candidate strings.
  const valuePrefix = active
    .slice(colon + 1)
    .toLowerCase()
    .replace(/^"/, "");
  if (!keysFor(surface).includes(field)) return [];
  return valuesForKey(field, surface, distinct, presentTypeIds).flatMap(
    (full) => {
      // Values match by SUBSTRING, not prefix — ids read most naturally from
      // their tail segments (`track:cod` completes `agent:codex`), and an empty
      // typed value offers everything.
      //
      // `distinctValues.actor` carries FULL ids; the parser canonicalizes the
      // short (prefix-less) spelling back to them, and the UI mints the short
      // form (chips, actor-ref clicks) — so suggestions insert and label the
      // short spelling, and a partially-typed value completes from EITHER
      // spelling (the substring test over the full id covers both).
      const value = field === "actor" ? full.replace(/^actor:/, "") : full;
      const matches =
        value.toLowerCase().includes(valuePrefix) ||
        (field === "actor" && full.toLowerCase().includes(valuePrefix));
      if (!matches) return [];
      // A whitespace-bearing value (a Git-name actor id) is quoted so the
      // inserted clause survives tokenization as ONE field clause.
      const clause = /\s/.test(value)
        ? `${field}:"${value}"`
        : `${field}:${value}`;
      return [{ insertText: clause, label: clause }];
    },
  );
}

/** Replace the trailing token of `filterText` with `insertText`, appending a
 * trailing space so the next keystroke starts a fresh token. */
export function acceptSuggestion(
  filterText: string,
  insertText: string,
): string {
  const tokens = filterText.split(/\s+/);
  tokens[tokens.length - 1] = insertText;
  return `${tokens.join(" ")} `;
}

// ---------------------------------------------------------------------------
// DOM wiring (module-local ephemeral view state, installed once)
// ---------------------------------------------------------------------------

function currentSurface(): QuerySurface {
  return getState().lens === "list" ? "revision" : "event";
}

// Client-side distinct values for the revisions lens, intentionally limited to
// loaded pages (the lens labels that partial scope). Read the STRUCTURED overview
// lists (the same fact-meta
// aggregation `revisionSearchIndex` encodes), never the space-wrapped index
// fields — the set encoding cannot carry a space-bearing actor id losslessly,
// and splitting it would fragment such ids into junk completions. Tags reduce
// to their first-colon keys (the whole string when colon-less), matching the
// server's `tag_completion_key`.
function distinctValuesFromRevisions(): DistinctValues {
  const track = new Set<string>();
  const actor = new Set<string>();
  const tag = new Set<string>();
  for (const r of getState().revisions?.entries ?? []) {
    const overview = r.overview ?? {};
    for (const id of overview.tracks ?? []) if (id) track.add(id.toLowerCase());
    for (const id of overview.actors ?? []) if (id) actor.add(id.toLowerCase());
    for (const full of overview.tags ?? []) {
      const key = full.split(":")[0] ?? full;
      if (key) tag.add(key.toLowerCase());
    }
  }
  return { track: [...track], actor: [...actor], tag: [...tag] };
}

function activeDistinctValues(): DistinctValues {
  if (currentSurface() === "event") {
    return (
      getState().history?.distinctValues ?? { track: [], actor: [], tag: [] }
    );
  }
  return distinctValuesFromRevisions();
}

// The union of type ids ever seen present this session — the `type:`
// completion vocabulary. `presentTypes()` narrows with the live query (its
// facet authority honors `q`), and a partially-typed `type:` clause matches
// nothing, so reading it live would empty the vocabulary mid-keystroke; the
// monotone union keeps the store's types offerable while the reader types.
// (Seeded by the unfiltered first load; a deep link that arrives narrowed
// self-heals as soon as a broader response lands.)
const seenPresentTypeIds = new Set<string>();

function presentTypeVocabulary(): ReadonlySet<string> {
  for (const id of presentTypes()) seenPresentTypeIds.add(id);
  return seenPresentTypeIds;
}

function currentSuggestions(input: HTMLInputElement): Suggestion[] {
  return suggestionsFor(
    input.value,
    currentSurface(),
    activeDistinctValues(),
    presentTypeVocabulary(),
  );
}

// The highlighted row, -1 when none. Transient view state (like the facet
// menu's open flag) — never on the store.
let activeIndex = -1;

function suggestionListEl(): HTMLElement | null {
  return $("#filter-suggestions");
}

function dismiss(): void {
  const list = suggestionListEl();
  if (list) {
    list.classList.add("hidden");
    list.innerHTML = "";
  }
  activeIndex = -1;
}

function paint(input: HTMLInputElement): Suggestion[] {
  const list = suggestionListEl();
  const suggestions = currentSuggestions(input);
  if (!list) return suggestions;
  activeIndex = -1;
  if (!suggestions.length) {
    dismiss();
    return suggestions;
  }
  list.classList.remove("hidden");
  list.innerHTML = suggestions
    .map(
      (s, i) =>
        `<li class="${suggestionClass(false)}" data-index="${i}">${escapeHtml(s.label)}</li>`,
    )
    .join("");
  return suggestions;
}

function updateActive(items: NodeListOf<HTMLElement>): void {
  items.forEach((el, i) => {
    el.classList.toggle(CLASS.suggestionActive, i === activeIndex);
  });
}

function accept(input: HTMLInputElement, suggestion: Suggestion): void {
  const next = acceptSuggestion(input.value, suggestion.insertText);
  input.value = next;
  navigate({ filterText: next }, { replace: true });
  dismiss();
  input.focus();
}

// Installed CAPTURE-phase (see initControls): an in-list row click commits via
// navigate, whose synchronous repaint can replace rows before the click
// finishes propagating — checked at bubble time, the clicked row could already
// be detached and containment would misread it as an outside click. At capture
// time the tree is intact.
function onDocumentClickForSuggestions(ev: MouseEvent): void {
  const list = suggestionListEl();
  if (!list || list.classList.contains("hidden")) return;
  if (
    ev.target instanceof Node &&
    (list.contains(ev.target) || $("#filter-text")?.contains(ev.target))
  ) {
    return;
  }
  dismiss();
}

function onSuggestionListClick(input: HTMLInputElement, ev: Event): void {
  const t = ev.target;
  if (!(t instanceof Element)) return;
  const row = t.closest<HTMLElement>("[data-index]");
  const indexAttr = row?.dataset.index;
  if (indexAttr == null) return;
  const chosen = currentSuggestions(input)[Number(indexAttr)];
  if (chosen) accept(input, chosen);
}

/** Wire `#filter-text`'s autocomplete listeners — installed once, independent
 * of main.ts's own "input" listener on the same element (which commits
 * filterText); multiple listeners on one element is the existing pattern
 * (every module's `initControls` wires its own fixed-id delegates). */
export function initControls(): void {
  const input = $<HTMLInputElement>("#filter-text");
  if (!input) return;
  input.addEventListener("input", () => paint(input));
  input.addEventListener("keydown", (ev) => {
    const list = suggestionListEl();
    if (!list || list.classList.contains("hidden")) {
      // Escape with nothing open falls through to the global ladder unchanged
      // (e.g. blurring the field) — do not stopPropagation here.
      return;
    }
    const items = list.querySelectorAll<HTMLElement>(`.${CLASS.suggestion}`);
    if (ev.key === "ArrowDown") {
      ev.preventDefault();
      activeIndex = Math.min(items.length - 1, activeIndex + 1);
      updateActive(items);
    } else if (ev.key === "ArrowUp") {
      ev.preventDefault();
      activeIndex = Math.max(0, activeIndex - 1);
      updateActive(items);
    } else if (ev.key === "Enter" || ev.key === "Tab") {
      if (activeIndex < 0) return; // no highlighted row — Enter's existing behavior runs
      ev.preventDefault();
      ev.stopPropagation(); // consumed: don't also run onKey's "focus timeline" Enter case
      const chosen = currentSuggestions(input)[activeIndex];
      if (chosen) accept(input, chosen);
    } else if (ev.key === "Escape") {
      // input[type=search] has a NATIVE Escape default — clear the field —
      // which stopPropagation cannot suppress; cancel it so dismissing the
      // popover never also wipes the query.
      ev.preventDefault();
      ev.stopPropagation(); // consumed: don't also run keyboard.ts's global Escape ladder
      dismiss();
    }
  });
  // Focus leaving the input closes the popover — Enter's "done searching"
  // handoff to the list, Tab, or any other focus move must not leave
  // suggestions floating over the master pane. The mousedown guard below
  // keeps a row click from blurring (and dismissing) its own target first.
  input.addEventListener("blur", (ev) => {
    const list = suggestionListEl();
    if (
      list &&
      ev.relatedTarget instanceof Node &&
      list.contains(ev.relatedTarget)
    ) {
      return;
    }
    dismiss();
  });
  const list = suggestionListEl();
  list?.addEventListener("mousedown", (ev) => {
    ev.preventDefault();
  });
  list?.addEventListener("click", (ev) => onSuggestionListClick(input, ev));
  document.addEventListener("click", onDocumentClickForSuggestions, true);
}
