import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("active Change inspector architecture", () => {
  it("boots only the Change-first composition and has no legacy semantic imports or aggregate URLs", async () => {
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
      for (const match of source.matchAll(/from\s+["'](\.[^"']+)["']/g)) {
        const candidate = resolve(dirname(file), `${match[1]}.ts`);
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
  });
});
