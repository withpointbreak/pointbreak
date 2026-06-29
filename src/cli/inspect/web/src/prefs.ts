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

/** The stored theme if it is `light`/`dark`, else the OS color-scheme preference. */
export function preferredTheme(): string {
  const stored = localStorage.getItem(THEME_KEY);
  if (stored === "light" || stored === "dark") return stored;
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

/** Wire the `#theme-toggle` / `#density-toggle` buttons (the fixed-id listener tier). */
export function initControls(): void {
  $("#theme-toggle")?.addEventListener("click", toggleTheme);
  $("#density-toggle")?.addEventListener("click", toggleDensity);
}
