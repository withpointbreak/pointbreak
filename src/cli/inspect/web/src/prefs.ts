// Reader-local display preferences (theme / density). Ported from the served
// app.js prefs cluster. These are reader-local choices, persisted in localStorage
// and never encoded in the URL/hash — they are not shareable view state, so they
// live off the store, applied straight to the document root as app.js does.
//
// Unlike app.js, the apply-on-load is NOT a top-level import side-effect: `main`
// calls `applyPrefs()` explicitly before the first paint.

import { $ } from "./dom";

const THEME_KEY = "shore-inspect-theme";
const DENSITY_KEY = "shore-inspect-density";

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

/**
 * Apply the persisted theme + density. `main` calls this before the first paint
 * so the chosen theme is in place immediately (reproduces app.js's top-level
 * `applyTheme(preferredTheme())` / `applyDensity(...)`, as an explicit call).
 */
export function applyPrefs(): void {
  applyTheme(preferredTheme());
  applyDensity(preferredDensity());
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
