import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
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
  return [...source.matchAll(/from\s+["'](\.[^"']+)["']/g)].map(
    (match) => match[1] ?? "",
  );
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
    const boundaries = new Map(
      inventory.modules.map((module) => [resolve("src", module.path), module]),
    );
    const entry = await readFile("src/entry.ts", "utf8");
    expect(entry).toContain('from "./change-inspector"');
    expect(entry).not.toContain("bootstrapChangeReader");
    const pending = [resolve("src/entry.ts")];
    const closure = new Map<string, string>();
    while (pending.length > 0) {
      const file = pending.pop();
      if (file === undefined) continue;
      if (closure.has(file)) continue;
      const source = await readFile(file, "utf8");
      closure.set(file, source);
      for (const imported of importsIn(source)) {
        const candidate = resolve(dirname(file), `${imported}.ts`);
        if (candidate.includes("/src/cli/inspect/web/src/"))
          pending.push(candidate);
      }
    }
    expect(
      [...closure.keys()].map((file) => file.replace(process.cwd(), "")),
    ).toContain("/src/entry.ts");
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
    for (const [file, boundary] of boundaries) {
      if (!closure.has(file)) continue;
      expect(
        boundary.activeImport,
        `${boundary.path} is ${boundary.classification} and must not be reachable from the active composition`,
      ).toBe(true);
    }
  });
});
