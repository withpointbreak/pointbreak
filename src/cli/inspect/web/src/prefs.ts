// Reader-local display preferences (theme / density / split width). Ported from
// the served app.js prefs cluster. These are reader-local choices, persisted in
// localStorage and never encoded in the URL/hash — they are not shareable view
// state, so they live off the store, applied straight to the document root as
// app.js does. The split width is an integer master-pane percent (25–75) applied
// as the `--split-master` custom property the `.split` grid reads; unset means
// the 50/50 default.
//
// Unlike app.js, the apply-on-load is NOT a top-level import side-effect: `main`
// calls `applyPrefs()` explicitly before the first paint.

import { $ } from "./dom";

const THEME_KEY = "shore-inspect-theme";
const DENSITY_KEY = "shore-inspect-density";
const SPLIT_KEY = "shore-inspect-split";

const SPLIT_MIN = 25;
const SPLIT_MAX = 75;

// Strong references to the MediaQueryLists we've attached `change` listeners to.
// WebKit/Safari garbage-collects a MediaQueryList that is reachable only through
// its own listener, which silently stops the listener from ever firing; holding a
// reference here keeps the query alive (Chromium/Firefox retain it regardless).
const liveMediaQueries: MediaQueryList[] = [];

/** True when the reader has pinned an explicit theme via the toggle (so the OS is ignored). */
function hasPinnedTheme(): boolean {
  const stored = localStorage.getItem(THEME_KEY);
  return stored === "light" || stored === "dark";
}

/** The stored theme if it is `light`/`dark`, else the OS color-scheme preference. */
export function preferredTheme(): string {
  if (hasPinnedTheme()) return localStorage.getItem(THEME_KEY) as string;
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

/** Apply a theme by setting `data-theme` on the document root. */
export function applyTheme(theme: string): void {
  document.documentElement.setAttribute("data-theme", theme);
}

/** Flip the theme (only `light` is checked, so any other value goes to `light`), persist, apply. */
export function toggleTheme(): void {
  const next =
    document.documentElement.getAttribute("data-theme") === "light"
      ? "dark"
      : "light";
  localStorage.setItem(THEME_KEY, next);
  applyTheme(next);
}

/** The stored density, defaulting to `comfortable` when unset. */
function preferredDensity(): string {
  return localStorage.getItem(DENSITY_KEY) || "comfortable";
}

/** Apply a density by toggling the `compact` class on the document root. */
export function applyDensity(mode: string): void {
  document.documentElement.classList.toggle("compact", mode === "compact");
}

/** Flip the density between `compact` and `comfortable`, persist, apply. */
export function toggleDensity(): void {
  const next = document.documentElement.classList.contains("compact")
    ? "comfortable"
    : "compact";
  localStorage.setItem(DENSITY_KEY, next);
  applyDensity(next);
}

/** The stored split width (integer master percent, 25–75), or null for the 50/50 default. */
export function preferredSplit(): number | null {
  const raw = localStorage.getItem(SPLIT_KEY);
  const n = raw === null ? Number.NaN : Number.parseInt(raw, 10);
  return Number.isInteger(n) && n >= SPLIT_MIN && n <= SPLIT_MAX ? n : null;
}

/**
 * Apply (and persist) a split width, clamped to the valid range so the stored
 * pref can never hold junk from our own writers; null clears back to the 50/50
 * default (property and key both removed). The divider controller is the only
 * post-paint caller — every width write goes through here.
 */
export function applySplit(pct: number | null): void {
  if (pct === null) {
    document.documentElement.style.removeProperty("--split-master");
    localStorage.removeItem(SPLIT_KEY);
    return;
  }
  const clamped = Math.round(Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, pct)));
  document.documentElement.style.setProperty("--split-master", `${clamped}%`);
  localStorage.setItem(SPLIT_KEY, String(clamped));
}

/**
 * Apply the persisted theme + density + split width. `main` calls this before
 * the first paint so the chosen theme is in place immediately (reproduces
 * app.js's top-level `applyTheme(preferredTheme())` / `applyDensity(...)`, as an
 * explicit call). A stored-but-invalid split key is left untouched and simply
 * not applied (the default grid wins).
 */
export function applyPrefs(): void {
  applyTheme(preferredTheme());
  applyDensity(preferredDensity());
  const split = preferredSplit();
  if (split !== null) applySplit(split);
}

/**
 * Follow live OS color-scheme changes while the reader hasn't pinned a theme.
 * `applyPrefs` reads the OS preference once before first paint; without this,
 * a later system light/dark switch only takes effect on the next page load.
 * A pinned light/dark choice (via the toggle) still wins over the OS.
 *
 * The query is retained in `liveMediaQueries` so Safari doesn't garbage-collect
 * it out from under its own listener (see the note on that binding).
 */
export function watchColorScheme(): void {
  const query = window.matchMedia("(prefers-color-scheme: light)");
  liveMediaQueries.push(query);
  query.addEventListener("change", () => {
    if (hasPinnedTheme()) return;
    applyTheme(preferredTheme());
  });
}

/** Wire the `#theme-toggle` / `#density-toggle` buttons and the OS color-scheme watcher. */
export function initControls(): void {
  $("#theme-toggle")?.addEventListener("click", toggleTheme);
  $("#density-toggle")?.addEventListener("click", toggleDensity);
  watchColorScheme();
}
