// The URL-fragment router: the hash grammar plus the navigate / apply choke point.
//
// `location.hash` is the single serialization of {lens|entity, selection, filters,
// diff overlay}. The fragment never reaches the GET-only server, so routing is
// entirely client-side. Theme and density are reader-local localStorage prefs and
// are deliberately NOT part of the fragment (not shareable view state).
//
//   #/<lens>                   lens-primary (lens ∈ timeline | list | attention)
//   #/<lens>?sel=<id>          a parked cursor within the lens (detail pane closed)
//   #/threads                  legacy alias: parses as #/list, query preserved
//                              verbatim; the address bar is replace-rewritten
//   #/revision/<revisionId>    entity-primary: the named revision is open in the detail pane
//   #/event/<eventId>          entity-primary: the named event is open in the detail pane
//   #/revision/<revisionId>/diff[?focus=<factId>][&file=<path>][&fq=<query>]
//                              the routed annotated-diff page (revision-primary;
//                              never carries the lens/selection/filter params);
//                              legacy `&nav=<filter>` links forward to the
//                              equivalent `fq=` clause (the address bar is
//                              replace-rewritten)
//   ?lens=<lens>               the master lens behind an entity-primary path
//   ?track= ?snapshot=         cross-lens scope (survive a lens switch)
//   ?order= ?types= ?q=        per-lens timeline controls
//   ?sort=<key>                the revision list's sort key (list lens only)
//   ?diff=<snapshotId> ?focus=<factId> ?diffHash=<snapshotContentHash>
//                              legacy diff entry: forwarded to the revision-primary
//                              page path when the snapshot maps to a loaded
//                              revision (the address bar is replace-rewritten);
//                              snapshot-only otherwise
//   ?v=1                       grammar version (reserved; parsed-but-ignored)
//   ?journal= ?asof=           reserved: reported as unsupported live-state input
//
// Ported from the served app.js router cluster. `parseHash`/`serializeState` are a
// pure round-trip — `parseHash` takes the present types, `serializeState` takes a
// state snapshot — so the grammar is unit-testable without a store. `navigate`/
// `applyHash` mutate through `store.commit` and `history` and never call render:
// the store subscriber is the only repaint path, so importing render here would be
// the router↔render cycle. This module imports neither render nor any lens.

import { fetchRevealPage, revealPatch } from "./data";
import { $ } from "./dom";
import { parkTimelineRead } from "./follow";
import {
  eventExists,
  presentTypes,
  revisionExists,
  revisionIdForSnapshot,
  revisionInAnyThread,
} from "./model";
import { refInfo, shortRef } from "./refs";
import { commit, getState, type Selection, type State } from "./store";

const LENSES = ["timeline", "list", "attention"];
const DEFAULT_LENS = "timeline";

/**
 * A parsed fragment: a full state patch plus the transient route seam. Absent
 * params resolve to their defaults so the fragment fully determines the
 * filter/selection state (Back/Forward to a barer fragment clears what it omits).
 * The seam fields (`unsupported*`/`unknownPath`) are read by `resolve`/
 * `liveStateDiagnostic` and are never committed — `resolve` returns a clean
 * `Partial<State>` that omits them.
 */
export interface RoutePatch {
  lens: string;
  // The cursor fields are OMITTED (not defaulted) by the diff-page path: the
  // page's identity is `diffRevision`, and an omitted field is never committed,
  // so applying a diff-page hash leaves the parked cursor untouched.
  selected?: Selection;
  // Openness rides the URL form: entity-primary paths parse as open, the
  // lens-primary `?sel=` cursor form (and no selection) as closed.
  open?: boolean;
  filterTrack: string;
  filterSnapshot: string;
  order: string;
  sortKey: "captured" | "activity";
  filterText: string;
  enabledTypes: Set<string>;
  diff: string | null;
  diffHash: string | null;
  focus: string | null;
  // The routed annotated-diff page. `parseHash` sets `diffPage` only from the
  // canonical path; `resolve()` additionally sets it for legacy `?diff=` links
  // so every diff intent renders as the page.
  diffPage: boolean;
  diffRevision: string | null;
  diffFile: string | null;
  // The diff page's file-search query (`?fq=`). OMITTED (like the cursor fields)
  // by every non-diff path arm, so a lens navigation never clobbers it; the
  // diff-page branch always assigns it (the URL is authoritative on that route).
  diffFileQuery?: string;
  // Reserved forward-compat seam: a reserved param surfaces a live-state notice.
  unsupportedAsOf: string | boolean | null;
  unsupportedJournal: string | boolean | null;
  // Set only when the path is unrecognized; resolve() surfaces a visible fallback.
  unknownPath: string | null;
  // Set when the path was a recognized legacy form that parsed into its current
  // equivalent; applyHash replace-rewrites the address bar so Back never bounces
  // through the stale form. A parse-seam field like `unknownPath` — never State.
  // "threads-alias" swaps only the path segment (query kept verbatim);
  // "legacy-diff" re-serializes to the canonical page form (the grammar changed
  // shape) and is set by resolve() only when the snapshot maps to a revision;
  // "legacy-diff-nav" re-serializes a diff-page `?nav=` filter into its `?fq=`
  // equivalent.
  migrated: "threads-alias" | "legacy-diff" | "legacy-diff-nav" | null;
}

/**
 * The state slice `serializeState` reads. `State` satisfies it structurally, so the
 * router passes `getState()`; tests pass a literal. Narrowing to this slice keeps
 * serialization a pure function of the route fields (it cannot read history/objects).
 */
export interface SerializeSnapshot {
  lens: string;
  selected: Selection;
  open: boolean;
  filterTrack: string;
  filterSnapshot: string;
  order: string;
  sortKey: "captured" | "activity";
  enabledTypes: Set<string>;
  filterText: string;
  diff: string | null;
  diffHash: string | null;
  focus: string | null;
  diffPage: boolean;
  diffRevision: string | null;
  diffFile: string | null;
  diffFileQuery: string;
}

/** Options for {@link navigate}: `replace` swaps the history entry for a refinement. */
export interface NavigateOptions {
  replace?: boolean;
}

// Classify a selection id as a revision or an event. A `rev:` id is a revision; the
// legacy `review-unit:` prefix is preserved as a revision too. Anything else is an
// event selection.
export function selectionKind(id: string): "event" | "revision" {
  const info = refInfo(id);
  if (info && (info.kind === "rev" || info.kind === "review-unit"))
    return "revision";
  return "event";
}

/** Decode an `&`-separated `key=value` query string; a bare key maps to "". */
export function parseQuery(
  queryString: string,
): Record<string, string | undefined> {
  const params: Record<string, string | undefined> = {};
  for (const pair of queryString.split("&")) {
    if (!pair) continue;
    const eq = pair.indexOf("=");
    const key = decodeURIComponent(eq < 0 ? pair : pair.slice(0, eq));
    params[key] = eq < 0 ? "" : decodeURIComponent(pair.slice(eq + 1));
  }
  return params;
}

/**
 * Parse a fragment into a complete route patch. `presentTypes` seeds the default
 * `enabledTypes` (all present) when no `types=` is given, so the function stays pure.
 */
export function parseHash(
  hash: string,
  presentTypes: readonly string[],
): RoutePatch {
  const raw = hash.replace(/^#/, "");
  const q = raw.indexOf("?");
  const path = q < 0 ? raw : raw.slice(0, q);
  const p = parseQuery(q < 0 ? "" : raw.slice(q + 1));

  const patch: RoutePatch = {
    lens: DEFAULT_LENS,
    filterTrack: p.track != null ? p.track : "",
    // The filter param is `snapshot`; legacy `object` is still parsed for old
    // bookmarks during the transition (#334).
    filterSnapshot:
      p.snapshot != null ? p.snapshot : p.object != null ? p.object : "",
    order: p.order === "asc" || p.order === "desc" ? p.order : "desc",
    sortKey: p.sort === "activity" ? "activity" : "captured",
    filterText: p.q != null ? p.q : "",
    enabledTypes:
      p.types != null
        ? new Set(p.types.split(",").filter(Boolean))
        : new Set(presentTypes),
    diff: p.diff || null,
    diffHash: p.diffHash || null,
    focus: p.focus ? p.focus : null,
    diffPage: false,
    diffRevision: null,
    diffFile: p.file ? p.file : null,
    unsupportedAsOf: p.asof != null ? p.asof || true : null,
    unsupportedJournal: p.journal != null ? p.journal || true : null,
    unknownPath: null,
    migrated: null,
  };

  const segs = path.split("/").filter(Boolean); // "/timeline" -> ["timeline"]
  const lensParam = p.lens ?? "";
  if (segs[0] === "revision" && segs[1] && segs[2] === "diff") {
    // The routed diff page. The page's identity is `diffRevision`; the patch
    // deliberately omits `selected`/`open` so the parked cursor of either kind
    // survives applying this hash (Back/forward, deep link). The file-search
    // query is assigned here ONLY (the URL is authoritative on this route); an
    // explicit `fq=` wins over any legacy `nav=`, and each of the three `nav=`
    // values that ever existed forwards to its `fq=` equivalent with the
    // canonical-URL rewrite — `nav=` itself is dead grammar.
    patch.diffPage = true;
    patch.diffRevision = decodeURIComponent(segs[1]);
    if (p.fq != null) {
      patch.diffFileQuery = p.fq;
    } else {
      switch (p.nav) {
        case "with-facts":
          patch.diffFileQuery = "has:facts";
          patch.migrated = "legacy-diff-nav";
          break;
        case "unanchored":
          patch.diffFileQuery = "is:unanchored";
          patch.migrated = "legacy-diff-nav";
          break;
        case "all":
          // The link's intent ("no filter") is already the grammar's default —
          // still worth canonicalizing the now-dead nav= param off the URL.
          patch.diffFileQuery = "";
          patch.migrated = "legacy-diff-nav";
          break;
        default:
          // No nav= at all, or a value that was never valid historically.
          patch.diffFileQuery = "";
      }
    }
    return patch;
  }
  patch.selected = { kind: null, id: null };
  patch.open = false;
  if (segs.length === 0) {
    patch.lens = DEFAULT_LENS;
  } else if (segs[0] === "revision" && segs[1]) {
    patch.selected = { kind: "revision", id: decodeURIComponent(segs[1]) };
    patch.open = true;
    patch.lens = LENSES.includes(lensParam) ? lensParam : DEFAULT_LENS;
  } else if (segs[0] === "event" && segs[1]) {
    patch.selected = { kind: "event", id: decodeURIComponent(segs[1]) };
    patch.open = true;
    patch.lens = LENSES.includes(lensParam) ? lensParam : DEFAULT_LENS;
  } else if (LENSES.includes(segs[0]) || segs[0] === "threads") {
    // `threads` is a retired lens: old links alias to the list lens, and the
    // query params (parsed independently of the path above) carry over verbatim.
    patch.lens = segs[0] === "threads" ? "list" : segs[0];
    if (segs[0] === "threads") patch.migrated = "threads-alias";
    if (p.sel) patch.selected = { kind: selectionKind(p.sel), id: p.sel };
  } else {
    patch.lens = DEFAULT_LENS;
    patch.unknownPath = path; // resolve() surfaces a visible fallback diagnostic
  }
  return patch;
}

/**
 * Serialize a state snapshot into a fragment, omitting defaults to keep the URL
 * short. An OPEN selection is entity-primary (durable identity, detail pane
 * showing); a parked cursor serializes lens-primary via `sel=` — the inverse of
 * the parser's `?sel=` handling. `presentTypes` decides whether a `types=` param
 * is needed (only when a present type is disabled).
 */
export function serializeState(
  snapshot: SerializeSnapshot,
  presentTypes: readonly string[],
): string {
  // The routed diff page is its own address: only the page params ride it —
  // never the lens/selection/filter params, and never the legacy diff=/diffHash=
  // pointers (the page derives snapshot identity from the revision). This branch
  // takes precedence so the page never rides another lens's path. A snapshot-only
  // page (diffPage with no diffRevision) falls through to the legacy `?diff=`
  // query form below, which remains parseable.
  if (snapshot.diffPage && snapshot.diffRevision) {
    const pageParams: string[] = [];
    if (snapshot.focus)
      pageParams.push(`focus=${encodeURIComponent(snapshot.focus)}`);
    if (snapshot.diffFile)
      pageParams.push(`file=${encodeURIComponent(snapshot.diffFile)}`);
    if (snapshot.diffFileQuery)
      pageParams.push(`fq=${encodeURIComponent(snapshot.diffFileQuery)}`);
    const pagePath = `#/revision/${encodeURIComponent(snapshot.diffRevision)}/diff`;
    return pageParams.length ? `${pagePath}?${pageParams.join("&")}` : pagePath;
  }
  const params: string[] = [];
  const sel = snapshot.selected ?? { kind: null, id: null };
  let path =
    snapshot.lens === DEFAULT_LENS ? "#/timeline" : `#/${snapshot.lens}`;
  if (
    sel.id &&
    snapshot.open &&
    (sel.kind === "revision" || sel.kind === "event")
  ) {
    path =
      sel.kind === "revision"
        ? `#/revision/${encodeURIComponent(sel.id)}`
        : `#/event/${encodeURIComponent(sel.id)}`;
    if (snapshot.lens && snapshot.lens !== DEFAULT_LENS)
      params.push(`lens=${encodeURIComponent(snapshot.lens)}`);
  } else if (sel.id) {
    params.push(`sel=${encodeURIComponent(sel.id)}`);
  }
  if (snapshot.filterTrack)
    params.push(`track=${encodeURIComponent(snapshot.filterTrack)}`);
  // Writes `snapshot`; the parser still accepts legacy `object` for old
  // bookmarks (#334 transition).
  if (snapshot.filterSnapshot)
    params.push(`snapshot=${encodeURIComponent(snapshot.filterSnapshot)}`);
  if (snapshot.order && snapshot.order !== "desc")
    params.push(`order=${encodeURIComponent(snapshot.order)}`);
  // The sort key is consumed only by the revision list, so only that lens
  // round-trips it — everywhere else the param is deliberately elided (the
  // serializer re-emits state on every lens switch and drops what it omits).
  if (snapshot.lens === "list" && snapshot.sortKey !== "captured")
    params.push(`sort=${encodeURIComponent(snapshot.sortKey)}`);
  if (presentTypes.some((id) => !snapshot.enabledTypes.has(id))) {
    params.push(
      `types=${encodeURIComponent(
        presentTypes.filter((id) => snapshot.enabledTypes.has(id)).join(","),
      )}`,
    );
  }
  if (snapshot.filterText)
    params.push(`q=${encodeURIComponent(snapshot.filterText)}`);
  if (snapshot.diff) params.push(`diff=${encodeURIComponent(snapshot.diff)}`);
  if (snapshot.diff && snapshot.diffHash)
    params.push(`diffHash=${encodeURIComponent(snapshot.diffHash)}`);
  if (snapshot.focus)
    params.push(`focus=${encodeURIComponent(snapshot.focus)}`);
  return params.length ? `${path}?${params.join("&")}` : path;
}

/**
 * The single mutation + history choke point. Commits the patch to the store (the
 * subscriber repaints), then pushes (or replaces, for a refinement) the serialized
 * state onto history. It never calls render — that is the router↔render cycle cut.
 */
export function navigate(
  patch: Partial<State>,
  opts: NavigateOptions = {},
): void {
  commit(patch);
  const hash = serializeState(getState(), presentTypes());
  if (opts.replace) history.replaceState({}, "", hash);
  else history.pushState({}, "", hash);
}

/**
 * Derive the whole view from the current fragment and commit it — the store
 * subscriber repaints. Called on boot and from the popstate / hashchange listeners
 * (Back/Forward + manual edits), which the composition root wires. An event
 * selection that is not in the loaded window is fetched-to-reveal asynchronously
 * (the history is server-paged, so the event may simply be off the loaded page).
 */
export function applyHash(): void {
  const parsed = parseHash(location.hash, presentTypes());
  const patch = resolve(parsed);
  // Applying any event selection parks the timeline before an off-window reveal
  // can replace its loaded window, preserving the pre-swap count anchor.
  if (patch.selected?.kind === "event" && patch.selected.id) parkTimelineRead();
  commit(patch);
  if (parsed.migrated === "threads-alias") {
    // Canonicalize the address bar: swap ONLY the path segment, keeping the
    // original query string byte-for-byte (serializeState would drop params it
    // does not know and normalize encoding). Replace, never push, so Back does
    // not bounce through the stale form.
    history.replaceState(
      {},
      "",
      location.hash.replace(/^#\/threads/, "#/list"),
    );
  } else if (
    parsed.migrated === "legacy-diff" ||
    parsed.migrated === "legacy-diff-nav"
  ) {
    // A forwarded `?diff=` or diff-page `?nav=` link intentionally changes query
    // grammar (the diff became a revision-primary page path; the nav filter
    // became the `?fq=` file query), so these modes re-serialize instead of
    // patching the original string. Replace, never push.
    history.replaceState({}, "", serializeState(getState(), presentTypes()));
  }
  const sel = getState().selected;
  if (sel.kind === "event" && sel.id && !eventExists(sel.id)) {
    void revealSelectedEvent(sel.id, patch.lens ?? DEFAULT_LENS);
  }
}

// Fetch-to-reveal an event a deep link named that is not in the loaded window:
// fetch the page containing it and commit the located window, or fall back with the
// existing "not in this store" diagnostic when it is genuinely absent from the set.
async function revealSelectedEvent(
  eventId: string,
  lens: string,
): Promise<void> {
  const page = await fetchRevealPage(eventId);
  if (!page) return;
  if (page.present) {
    commit(revealPatch(page, eventId));
    clearRouteDiagnostic();
    return;
  }
  commit({ selected: { kind: null, id: null } });
  showRouteDiagnostic(
    `fell back to the ${lens} lens — event ${shortRef(eventId)} is not in this store`,
  );
}

/**
 * Resolve a parsed patch against the loaded data, falling back (absent revision →
 * the lens, unknown route → timeline) with a visible diagnostic when a deep link
 * names an absent entity — never a 404, never a blank view. Returns a clean
 * `Partial<State>` that omits the transient route seam (the "cleaning" the
 * served code did by `delete`-ing fields off the patch).
 */
export function resolve(patch: RoutePatch): Partial<State> {
  const freshnessDiagnostic = liveStateDiagnostic(patch);
  const next = statePatchFrom(patch);
  if (patch.unknownPath != null) {
    showRouteDiagnostic(
      routeDiagnostic(
        `fell back to the timeline — unknown route ${patch.unknownPath}`,
        freshnessDiagnostic,
      ),
    );
    next.lens = DEFAULT_LENS;
    next.selected = { kind: null, id: null };
    return next;
  }
  // A legacy `?diff=<snapshotId>` link is a diff intent: it renders as the diff
  // page. When the snapshot maps to a loaded revision the link forwards to the
  // revision-primary form (and applyHash canonicalizes the address bar); an
  // unmappable snapshot-only link stays snapshot-addressed — the route must not
  // invent a revision. A `diffRevision` absent from the loaded list is left
  // alone: grouped-away ids resolve through the entity-primary composite fetch.
  if (patch.diff && !patch.diffPage) {
    next.diffPage = true;
    const mapped = revisionIdForSnapshot(patch.diff, patch.diffHash);
    if (mapped) {
      next.diffRevision = mapped;
      patch.migrated = "legacy-diff";
    }
  }
  const sel = patch.selected ?? { kind: null, id: null };
  if (sel.kind === "revision" && sel.id && !revisionExists(sel.id)) {
    if (revisionInAnyThread(sel.id)) {
      // Grouped away from the loaded list but known to the store: the detail
      // pane's entity-primary `/api/revisions/{id}` fetch resolves the exact
      // id, so the selection stands — and it always opens, because no list
      // card exists for the id and a parked cursor would be invisible state.
      next.open = true;
    } else {
      // Keep the requested lens (only the selection was absent); name it in the
      // diagnostic so the message matches the lens actually shown.
      const lens = patch.lens || DEFAULT_LENS;
      showRouteDiagnostic(
        routeDiagnostic(
          `fell back to the ${lens} lens — revision ${shortRef(sel.id)} is not in this store`,
          freshnessDiagnostic,
        ),
      );
      next.lens = lens;
      next.selected = { kind: null, id: null };
      return next;
    }
  }
  // An event selection is not resolved against the loaded window here (the history
  // is server-paged, so the event may be off the loaded page). `applyHash`
  // fetches-to-reveal it and applies the "not in this store" fallback only when the
  // server confirms it is genuinely absent.
  if (freshnessDiagnostic) {
    showRouteDiagnostic(freshnessDiagnostic);
    return next;
  }
  clearRouteDiagnostic();
  return next;
}

/** The committable State fields of a route patch — the transient seam is dropped here. */
function statePatchFrom(patch: RoutePatch): Partial<State> {
  const next: Partial<State> = {
    lens: patch.lens,
    filterTrack: patch.filterTrack,
    filterSnapshot: patch.filterSnapshot,
    order: patch.order,
    // sortKey, like order/filterText, is set in parseHash's base patch object
    // before any path-arm branches (including the diff-page early return), so it
    // is always present on a full parse — unlike selected/open, which the
    // diff-page branch deliberately omits. Unconditional copy is therefore correct.
    sortKey: patch.sortKey,
    filterText: patch.filterText,
    enabledTypes: patch.enabledTypes,
    diff: patch.diff,
    diffHash: patch.diffHash,
    focus: patch.focus,
    diffPage: patch.diffPage,
    diffRevision: patch.diffRevision,
    diffFile: patch.diffFile,
  };
  // The cursor fields are copied only when the patch carries them: the diff-page
  // path omits them, and committing `undefined` would clear the parked cursor.
  // `diffFileQuery` follows the same rule inverted: only the diff-page path
  // carries it, so a lens navigation never clobbers a parked query.
  if (patch.selected !== undefined) next.selected = patch.selected;
  if (patch.open !== undefined) next.open = patch.open;
  if (patch.diffFileQuery !== undefined)
    next.diffFileQuery = patch.diffFileQuery;
  return next;
}

/**
 * The live-state notice for reserved (unsupported) links. Pure: it reads the seam
 * fields and returns the message; the patch is cleaned by {@link statePatchFrom}
 * never copying them, not by deleting them here.
 */
export function liveStateDiagnostic(patch: RoutePatch): string {
  const unsupported: string[] = [];
  if (patch.unsupportedAsOf != null)
    unsupported.push("as-of links are not supported by this server");
  if (patch.unsupportedJournal != null)
    unsupported.push("journal links are not supported by this server");
  return unsupported.length
    ? `showing live state — ${unsupported.join("; ")}`
    : "";
}

/** Join a primary diagnostic with an optional secondary clause. */
export function routeDiagnostic(primary: string, secondary: string): string {
  return secondary ? `${primary} — ${secondary}` : primary;
}

/** Show a route diagnostic in the live region and reveal it. */
export function showRouteDiagnostic(message: string): void {
  const el = $("#route-diagnostic");
  if (!el) return;
  el.textContent = message;
  el.classList.remove("hidden");
}

/** Clear the route diagnostic and re-hide the live region. */
export function clearRouteDiagnostic(): void {
  const el = $("#route-diagnostic");
  if (!el) return;
  el.textContent = "";
  el.classList.add("hidden");
}
