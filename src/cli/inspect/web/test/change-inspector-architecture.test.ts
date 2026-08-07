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
      expect(file).not.toMatch(
        /\/(main|store|router|data|render|detail|access|model|projection|navigation|follow|cards|refs)\.ts$/,
      );
      expect(source).not.toMatch(
        /\.\/(main|store|router|data|render|detail|access|model|projection|navigation|follow|cards|refs)["']/,
      );
      expect(source).not.toMatch(/\/api\/(?!v2\/)/);
    }
  });
});
