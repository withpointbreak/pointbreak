import { build } from "esbuild";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildOptions } from "../esbuild.config.mjs";
import { mountInspectorDom, resetDom } from "./support/dom";
import { installFetchMock, uninstallFetchMock } from "./support/fetch";

// The build verification: esbuild bundles the entry to the production shape the
// served `assets/app.js` will take after the emit flip — a deterministic,
// non-minified IIFE that invokes `main()` and boots in happy-dom. The bundle is
// built to an in-memory result (`write: false`), so `assets/app.js` stays
// byte-unchanged in this PR. The build script (`node build.mjs`) and this test share
// one option set (`esbuild.config.mjs`), so the committed artifact and the gate's
// check build from identical options.

/** Bundle the entry to an in-memory string with the shared production options. */
async function bundleText(): Promise<string> {
  const result = await build({ ...buildOptions, write: false });
  const file = result.outputFiles?.[0];
  if (!file) throw new Error("esbuild produced no output file");
  return file.text;
}

describe("the inspector bundle is the served-artifact shape", () => {
  it("is a deterministic, non-minified IIFE that invokes main() with no ESM export", async () => {
    const first = await bundleText();
    const second = await bundleText();
    // Idempotent / byte-reproducible — the freshness gate must never flap.
    expect(first).toEqual(second);
    // An auto-executing IIFE (not an ESM module): wrapped and invoked immediately.
    expect(first).toContain("(() => {");
    expect(first.trimEnd().endsWith("})();")).toBe(true);
    expect(first).not.toMatch(/^export[ {]/m);
    // The entry invokes main() (keep-names preserves the symbol; non-minified keeps newlines).
    expect(first).toMatch(/\bmain\(\)/);
    expect(first).toContain("\n");
  });
});

describe("the bundle boots the inspector in happy-dom", () => {
  beforeEach(() => {
    mountInspectorDom();
    installFetchMock();
    history.replaceState(null, "", "/");
  });

  afterEach(() => {
    uninstallFetchMock();
    resetDom();
  });

  it("renders the master pane from the loaded fixtures when evaluated", async () => {
    const text = await bundleText();
    // Evaluate the IIFE in the happy-dom window — the same eval the served
    // `<script src="/app.js">` performs at end of <body>.
    new Function(text)();
    await vi.waitFor(() => {
      const master = document.querySelector("#master");
      expect((master?.children.length ?? 0) > 0).toBe(true);
    });
  });
});
