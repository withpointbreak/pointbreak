import { readFile } from "node:fs/promises";
import { dirname, relative, resolve, sep } from "node:path";
import { describe, expect, it } from "vitest";

interface BoundaryModule {
  path: string;
  classification: "adapted" | "model-neutral-retained" | "quarantined";
  activeImport: boolean;
  reason: string;
}

interface BoundaryInventory {
  version: number;
  purpose: string;
  keyboardAndFocusOwnership: string;
  modules: BoundaryModule[];
}

async function readBoundaryInventory(): Promise<BoundaryInventory> {
  return JSON.parse(
    await readFile("src/change-inspector-architecture.json", "utf8"),
  ) as BoundaryInventory;
}

function importsIn(source: string): string[] {
  const fromImports = [
    ...source.matchAll(
      /(?:import|export)\s+(?:type\s+)?[^;]*?\s+from\s+["'](\.[^"']+)["']/g,
    ),
  ];
  const sideEffectImports = [
    ...source.matchAll(/import\s+["'](\.[^"']+)["']/g),
  ];
  const dynamicImports = [
    ...source.matchAll(/import\s*\(\s*["'](\.[^"']+)["']\s*\)/g),
  ];
  return [...fromImports, ...sideEffectImports, ...dynamicImports].map(
    (match) => match[1] ?? "",
  );
}

const ACTIVE_SOURCE_ROOT = resolve("src");

/**
 * These four leaves are deliberately omitted from the semantic inventory:
 * each is a tiny generic primitive with no Pointbreak model, transport, route,
 * or state vocabulary. Keeping this allowlist explicit and exact prevents a
 * newly reachable module from becoming "neutral" merely by going unrecorded.
 */
const NEUTRAL_IMPORT_ALLOWLIST = new Map<string, string>([
  ["classNames.ts", "pure CSS class composition"],
  ["dom.ts", "typed DOM lookup"],
  ["escape.ts", "HTML escaping"],
  ["format.ts", "pure scalar formatting"],
]);

function importedTypeScriptPath(
  importer: string,
  specifier: string,
): string | null {
  const unresolved = resolve(dirname(importer), specifier);
  if (specifier.endsWith(".json")) return null;
  if (specifier.endsWith(".ts")) return unresolved;
  if (specifier.endsWith(".js")) return `${unresolved.slice(0, -3)}.ts`;
  return `${unresolved}.ts`;
}

async function activeImportClosure(): Promise<Map<string, string>> {
  const pending = [resolve(ACTIVE_SOURCE_ROOT, "entry.ts")];
  const closure = new Map<string, string>();
  while (pending.length > 0) {
    const file = pending.pop();
    if (file === undefined || closure.has(file)) continue;
    const source = await readFile(file, "utf8");
    closure.set(file, source);
    for (const imported of importsIn(source)) {
      const candidate = importedTypeScriptPath(file, imported);
      if (candidate === null) continue;
      const fromSourceRoot = relative(ACTIVE_SOURCE_ROOT, candidate);
      if (fromSourceRoot !== ".." && !fromSourceRoot.startsWith(`..${sep}`)) {
        pending.push(candidate);
      }
    }
  }
  return closure;
}

function sourcePath(file: string): string {
  return relative(ACTIVE_SOURCE_ROOT, file).replaceAll("\\", "/");
}

describe("active Change inspector architecture", () => {
  it("records the retained, adapted, and quarantined Inspector boundary", async () => {
    const inventory = await readBoundaryInventory();
    expect(inventory.version).toBe(1);
    expect(inventory.purpose).not.toHaveLength(0);
    expect(inventory.keyboardAndFocusOwnership).toContain(
      "change-inspector.ts",
    );
    expect(inventory.modules).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          path: "change-inspector-interaction.ts",
          classification: "adapted",
          activeImport: true,
        }),
        expect.objectContaining({
          path: "change-inspector-timeline-boundary.ts",
          classification: "adapted",
          activeImport: true,
        }),
        expect.objectContaining({
          path: "change-inspector-timeline-monitor.ts",
          classification: "adapted",
          activeImport: true,
        }),
        expect.objectContaining({
          path: "change-inspector-timeline-navigation.ts",
          classification: "adapted",
          activeImport: true,
        }),
        expect.objectContaining({
          path: "change-inspector-timeline.ts",
          classification: "adapted",
          activeImport: true,
        }),
        expect.objectContaining({ path: "detail.ts" }),
        expect.objectContaining({ path: "diff/controller.ts" }),
        expect.objectContaining({ path: "keyboard.ts" }),
        expect.objectContaining({ path: "prefs.ts" }),
        expect.objectContaining({ path: "palette.ts" }),
        expect.objectContaining({ path: "render.ts" }),
        expect.objectContaining({ path: "main.ts" }),
      ]),
    );
    expect(new Set(inventory.modules.map((module) => module.path)).size).toBe(
      inventory.modules.length,
    );
    for (const module of inventory.modules) {
      expect(module.classification).toMatch(
        /^(adapted|model-neutral-retained|quarantined)$/,
      );
      expect(module.reason).not.toHaveLength(0);
      await expect(readFile(`src/${module.path}`, "utf8")).resolves.toBeTypeOf(
        "string",
      );
      if (module.classification === "quarantined") {
        expect(module.activeImport).toBe(false);
      }
    }
  });

  it("boots only the Change-first composition and has no legacy semantic imports or aggregate URLs", async () => {
    const inventory = await readBoundaryInventory();
    const boundaries = new Map<string, BoundaryModule>(
      inventory.modules.map((module) => [module.path, module]),
    );
    const entry = await readFile("src/entry.ts", "utf8");
    expect(entry).toContain('from "./change-inspector"');
    expect(entry).not.toContain("bootstrapChangeReader");
    const closure = await activeImportClosure();
    const closurePaths = new Set([...closure.keys()].map(sourcePath));
    expect(closurePaths).toContain("entry.ts");

    for (const [path, reason] of NEUTRAL_IMPORT_ALLOWLIST) {
      expect(reason).not.toHaveLength(0);
      expect(
        closurePaths.has(path),
        `${path} is allowlisted as neutral but is not in the active closure`,
      ).toBe(true);
      expect(
        boundaries.has(path),
        `${path} must be either inventoried or neutral, never both`,
      ).toBe(false);
    }
    for (const path of closurePaths) {
      const boundary = boundaries.get(path);
      expect(
        boundary !== undefined || NEUTRAL_IMPORT_ALLOWLIST.has(path),
        `${path} is reachable from entry.ts but has no architecture inventory entry`,
      ).toBe(true);
      if (boundary !== undefined) {
        expect(
          boundary.activeImport,
          `${path} is ${boundary.classification} and must not be reachable from the active composition`,
        ).toBe(true);
      }
    }
    for (const boundary of inventory.modules) {
      if (boundary.activeImport) {
        expect(
          closurePaths.has(boundary.path),
          `${boundary.path} is marked activeImport=true but is not reachable from entry.ts`,
        ).toBe(true);
      }
      if (boundary.classification === "quarantined") {
        expect(
          closurePaths.has(boundary.path),
          `${boundary.path} is quarantined but reachable from entry.ts`,
        ).toBe(false);
      }
    }

    // Type-only imports are still architectural dependencies: a neutral renderer
    // must not regain the legacy aggregate store through a type declared beside
    // the state-reading model. Keep the whole active import closure free of both
    // legacy semantic roots, not merely the three composition entry modules.
    for (const file of closure.keys()) {
      expect(file).not.toMatch(/\/src\/(model|store)\.ts$/);
    }
    for (const [file, source] of closure) {
      const activeComposition =
        file.endsWith("/src/entry.ts") ||
        file.endsWith("/src/change-inspector.ts") ||
        file.endsWith("/src/change-inspector-render.ts");
      if (activeComposition) {
        expect(file).not.toMatch(
          /\/src\/(main|store|router|data|render|detail|access|model|projection|navigation|follow)\.ts$/,
        );
        expect(source).not.toMatch(
          /\.\/(main|store|router|data|render|detail|access|model|projection|navigation|follow)["']/,
        );
      }
      // The Change-first composition may only call the versioned reader API.
      // Retained pure render helpers can document the historical endpoint they
      // were adapted from, but never make a transport request themselves.
      if (
        activeComposition ||
        file.endsWith("/src/change-inspector-reading.ts")
      )
        expect(source).not.toMatch(/\/api\/(?!v2\/)/);
    }
  });
});
