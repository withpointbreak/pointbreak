import { describe, expect, it } from "vitest";
import { renderMarkdown as renderInspectorMarkdown } from "../../../src/cli/inspect/web/src/markdown";
import { buildProblemsSnapshot } from "../src/problemsSnapshot";
import { renderMarkdown as renderWebviewMarkdown } from "../src/webview/diff/markdown";

const timestamp = "2026-07-15T12:34:56.000Z";

describe("buildProblemsSnapshot", () => {
  it("keeps only safe target-relative diagnostics and sorts every documented field", () => {
    const markdown = buildProblemsSnapshot(
      [
        diagnostics("/outside/ignored.ts", [
          diagnostic("outside the target", 0, 0, 0, "outside", "1"),
        ]),
        diagnostics("/repo/src/same.ts", [
          diagnostic("z message", 0, 0, 1, "z-source", "2"),
          diagnostic("a message", 0, 0, 1, "z-source", "2"),
          diagnostic("code first", 0, 0, 1, "z-source", "1"),
          diagnostic("source first", 0, 0, 1, "a-source", "9"),
          diagnostic("error first", 0, 0, 0, "z-source", "9"),
          diagnostic("later range", 1, 0, 0, "a-source", "1"),
        ]),
        diagnostics("/repo/src/z.ts", [
          diagnostic("later path", 0, 0, 0, "a-source", "1"),
        ]),
        diagnostics("/repo/src/a.ts", [
          diagnostic("first path", 0, 0, 0, "a-source", "1"),
        ]),
        diagnostics("/repo/../escape.ts", [
          diagnostic("traversal", 0, 0, 0, "outside", "1"),
        ]),
        diagnostics("relative.ts", [
          diagnostic("unresolvable", 0, 0, 0, "outside", "1"),
        ]),
        diagnostics("/repo/src\\unsafe.ts", [
          diagnostic("unsafe separator", 0, 0, 0, "outside", "1"),
        ]),
      ],
      { repoRoot: "/repo", targetLabel: "repo", timestamp },
    );

    expect(markdown).toContain(
      "**Counts:** 8 total; error 4; warning 4; information 0; hint 0; unknown 0",
    );
    expect(markdown).not.toMatch(
      /outside the target|traversal|unresolvable|unsafe separator/,
    );
    expect(entryMessages(markdown)).toEqual([
      "first path",
      "error first",
      "source first",
      "code first",
      "a message",
      "z message",
      "later range",
      "later path",
    ]);
  });

  it("normalizes multiline content and escapes Markdown deterministically", () => {
    const sample = [
      diagnostics("/repo/docs/[guide].md", [
        {
          ...diagnostic(
            "  first *line*  \r\nsecond [line]  \rthird | line\n\n",
            0,
            1,
            0,
            "lint*",
            { value: "rule[1]", target: { fsPath: "/rules/one" } },
          ),
          range: {
            start: { line: 0, character: 1 },
            end: { line: 2, character: 4 },
          },
        },
      ]),
    ] as const;

    const first = buildProblemsSnapshot(sample, {
      repoRoot: "/repo",
      targetLabel: "Repo *alpha*",
      timestamp,
    });
    const second = buildProblemsSnapshot(sample, {
      repoRoot: "/repo",
      targetLabel: "Repo *alpha*",
      timestamp,
    });

    expect(second).toBe(first);
    expect(first).toBe(
      `
# Problems snapshot

**Target:** Repo \\*alpha\\*
**Sampled:** ${timestamp}
**Counts:** 1 total; error 1; warning 0; information 0; hint 0; unknown 0

> This is an incomplete point-in-time view of diagnostics VS Code currently surfaces.

## Diagnostics

- **Error** — docs/\\[guide\\]\\.md:1:2–3:5 — source: lint\\* — code: rule\\[1\\] — first \\*line\\*<br>second \\[line\\]<br>third \\| line
`.trimStart(),
    );
    const rendered = renderWebviewMarkdown(first);
    expect(rendered).toContain(
      "first *line*&lt;br&gt;second [line]&lt;br&gt;third | line",
    );
    expect(rendered).not.toContain("<em>line</em>");
  });

  it("keeps escaped diagnostic backticks literal in both Review renderers", () => {
    const markdown = buildProblemsSnapshot(
      [
        diagnostics("/repo/src/example.ts", [
          diagnostic("literal `code` and *emphasis*"),
        ]),
      ],
      { repoRoot: "/repo", targetLabel: "repo", timestamp },
    );

    expect(markdown).toContain("literal \\`code\\` and \\*emphasis\\*");
    for (const render of [renderWebviewMarkdown, renderInspectorMarkdown]) {
      const rendered = render(markdown);
      expect(rendered).toContain("literal `code` and *emphasis*");
      expect(rendered).not.toContain("<code>code</code>");
      expect(rendered).not.toContain("<em>emphasis</em>");
    }
  });

  it("preserves duplicate diagnostics and renders every severity shape", () => {
    const duplicate = diagnostic("same", 0, 0, 0, undefined, undefined);
    const markdown = buildProblemsSnapshot(
      [
        diagnostics("/repo/all.ts", [
          diagnostic("unknown", 0, 0, 99, undefined, 0),
          diagnostic("hint", 0, 0, 3, undefined, 3),
          diagnostic("information", 0, 0, 2, undefined, 2),
          diagnostic("warning", 0, 0, 1, undefined, 1),
          duplicate,
          duplicate,
        ]),
      ],
      { repoRoot: "/repo", targetLabel: "repo", timestamp },
    );

    expect(markdown).toContain(
      "**Counts:** 6 total; error 2; warning 1; information 1; hint 1; unknown 1",
    );
    expect(entryMessages(markdown)).toEqual([
      "same",
      "same",
      "warning",
      "information",
      "hint",
      "unknown",
    ]);
  });

  it("makes an empty sample explicit without implying validation success", () => {
    const markdown = buildProblemsSnapshot(
      [diagnostics("/outside/ignored.ts", [diagnostic("ignored")])],
      { repoRoot: "/repo", targetLabel: "repo", timestamp },
    );

    expect(markdown).toContain(
      "**Counts:** 0 total; error 0; warning 0; information 0; hint 0; unknown 0",
    );
    expect(markdown).toContain("No diagnostics were currently reported.");
    expect(
      markdown.match(
        /This is an incomplete point-in-time view of diagnostics VS Code currently surfaces\./g,
      ),
    ).toHaveLength(2);
    expect(markdown).not.toMatch(/\b(pass|passed|passing|validated)\b/i);
  });
});

function diagnostics(
  fsPath: string,
  entries: readonly ReturnType<typeof diagnostic>[],
) {
  return [{ fsPath }, entries] as const;
}

function diagnostic(
  message: string,
  line = 0,
  character = 0,
  severity = 0,
  source?: string,
  code?:
    | string
    | number
    | { value: string | number; target: { fsPath: string } },
) {
  return {
    message,
    range: {
      start: { line, character },
      end: { line, character: character + 1 },
    },
    severity,
    source,
    code,
  };
}

function entryMessages(markdown: string): string[] {
  return markdown
    .split("\n")
    .filter((line) => line.startsWith("- **"))
    .map((line) => line.split(" — ").at(-1) ?? "");
}
