import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  applyDensity,
  applyPrefs,
  applyTheme,
  initControls,
  preferredTheme,
  toggleDensity,
  toggleTheme,
} from "../src/prefs";
import { mountInspectorDom, resetDom } from "./support/dom";

// The persisted storage keys (the reader-local preference contract; mirrors app.js).
const THEME_KEY = "shore-inspect-theme";
const DENSITY_KEY = "shore-inspect-density";

const realMatchMedia = window.matchMedia;

function fakeMediaQueryList(matches: boolean, media: string): MediaQueryList {
  return {
    matches,
    media,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  };
}

/** Make `prefers-color-scheme: light` resolve deterministically. */
function stubPrefersLight(prefersLight: boolean): void {
  window.matchMedia = (query: string) =>
    fakeMediaQueryList(prefersLight && query.includes("light"), query);
}

beforeEach(() => {
  mountInspectorDom();
  localStorage.clear();
  stubPrefersLight(false);
});

afterEach(() => {
  resetDom();
  localStorage.clear();
  window.matchMedia = realMatchMedia;
});

describe("preferredTheme", () => {
  it("returns the stored theme when it is light or dark", () => {
    localStorage.setItem(THEME_KEY, "light");
    expect(preferredTheme()).toBe("light");
    localStorage.setItem(THEME_KEY, "dark");
    expect(preferredTheme()).toBe("dark");
  });

  it("falls back to the OS color-scheme preference when unset", () => {
    stubPrefersLight(true);
    expect(preferredTheme()).toBe("light");
    stubPrefersLight(false);
    expect(preferredTheme()).toBe("dark");
  });

  it("ignores a junk stored value and uses the OS preference", () => {
    localStorage.setItem(THEME_KEY, "neon");
    stubPrefersLight(true);
    expect(preferredTheme()).toBe("light");
  });
});

describe("applyTheme / toggleTheme", () => {
  it("applyTheme sets data-theme on the document root", () => {
    applyTheme("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("toggleTheme flips light<->dark and persists the choice", () => {
    applyTheme("light");
    toggleTheme();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(localStorage.getItem(THEME_KEY)).toBe("dark");
    toggleTheme();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(localStorage.getItem(THEME_KEY)).toBe("light");
  });

  it("toggles to light from an unset root (only 'is it light?' is checked)", () => {
    toggleTheme();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });
});

describe("applyDensity / toggleDensity", () => {
  it("applyDensity toggles the compact class on the root", () => {
    applyDensity("compact");
    expect(document.documentElement.classList.contains("compact")).toBe(true);
    applyDensity("comfortable");
    expect(document.documentElement.classList.contains("compact")).toBe(false);
  });

  it("toggleDensity flips compact<->comfortable and persists the choice", () => {
    toggleDensity();
    expect(document.documentElement.classList.contains("compact")).toBe(true);
    expect(localStorage.getItem(DENSITY_KEY)).toBe("compact");
    toggleDensity();
    expect(document.documentElement.classList.contains("compact")).toBe(false);
    expect(localStorage.getItem(DENSITY_KEY)).toBe("comfortable");
  });
});

describe("applyPrefs", () => {
  it("applies the stored theme and density (the before-first-paint step)", () => {
    localStorage.setItem(THEME_KEY, "light");
    localStorage.setItem(DENSITY_KEY, "compact");
    applyPrefs();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(document.documentElement.classList.contains("compact")).toBe(true);
  });

  it("defaults density to comfortable when unset", () => {
    applyPrefs();
    expect(document.documentElement.classList.contains("compact")).toBe(false);
  });
});

describe("initControls", () => {
  it("wires the #theme-toggle and #density-toggle buttons", () => {
    applyTheme("light");
    initControls();
    document.getElementById("theme-toggle")?.click();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    document.getElementById("density-toggle")?.click();
    expect(document.documentElement.classList.contains("compact")).toBe(true);
  });
});
