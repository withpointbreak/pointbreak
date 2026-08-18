import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import { ALL_EMITTABLE_CLASSES } from "../src/classNames";

// The served stylesheet, resolved from the web package root (vitest's working
// directory is `src/cli/inspect/web`, where this suite always runs). This reads
// the committed source CSS, not the bundle.
const APP_CSS_PATH = resolve(process.cwd(), "../assets/app.css");
const TOKENS_CSS_PATH = resolve(process.cwd(), "../assets/tokens.css");

const COMPACT_ALLOWED_PROPERTIES = new Set([
  "--row-pad",
  "--line",
  "--card-pad",
]);

// Classes the inspector can emit that have no `app.css` rule and fall back to
// their base class, each with a one-line reason. Whether any is a real styling
// gap is being evaluated in withpointbreak/pointbreak#296; this list keeps the drift
// test green while that decision is owned there. An emitted class with no rule
// and no entry here fails the test — that is the JS-vs-CSS drift catch.
const REF_BASE_STYLED =
  "clickable ref chip; styled via `.ref[data-ref-kind]` (accent), the per-kind class is only a hook — intentional, the `.ref-commit`/`.ref-hash` rules exist to dim the non-clickable kinds (#296)";
const REF_NONCLICKABLE_STYLED =
  "non-clickable content-id chip (no resolveRef route); base `.ref` styling without the `.ref[data-ref-kind]` accent — display-only membership (#344)";
const CSS_LESS_ALLOWLIST: Record<string, string> = {
  // anno-validation and s-modified were #296 gaps and now have app.css rules, so
  // they are NOT allowlisted here (the guard test below would flag them if they were).
  resolved:
    "`fact-status resolved` cue; inherits base `.fact-status` (intentional — only emits for a resolved assessment with no value) — see #296",
  "ref-input-request-response": REF_BASE_STYLED,
  "ref-input-request": REF_BASE_STYLED,
  "ref-obs": REF_BASE_STYLED,
  "ref-assess": REF_BASE_STYLED,
  "ref-rev": REF_BASE_STYLED,
  "ref-evt": REF_BASE_STYLED,
  "ref-validation": REF_BASE_STYLED,
  "ref-track": REF_BASE_STYLED,
  "ref-actor": REF_BASE_STYLED,
  // #344 promoted content ids: linkified as non-clickable chips.
  "ref-obj": REF_NONCLICKABLE_STYLED,
  "ref-engagement": REF_NONCLICKABLE_STYLED,
  "ref-checkpoint": REF_NONCLICKABLE_STYLED,
  "ref-task-attempt": REF_NONCLICKABLE_STYLED,
  "ref-assoc-commit": REF_NONCLICKABLE_STYLED,
  "ref-assoc-ref": REF_NONCLICKABLE_STYLED,
  "ref-withdraw-commit": REF_NONCLICKABLE_STYLED,
  "ref-withdraw-ref": REF_NONCLICKABLE_STYLED,
};

// Every `.class` token in the stylesheet, INCLUDING those inside compound /
// descendant / pseudo selectors (`.dag-node.head rect`, `.fact-status.passed`,
// `.cmd-item:hover`), so a class counts as present if it appears in any selector.
function cssClassSelectors(css: string): Set<string> {
  return new Set(
    [...css.matchAll(/\.([a-z][a-z0-9_-]*)/g)].map((match) => match[1]),
  );
}

test("every emittable class has an app.css selector (or is an allowlisted CSS-less class)", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  const selectors = cssClassSelectors(css);
  const missing = ALL_EMITTABLE_CLASSES.filter(
    (cls) => !selectors.has(cls),
  ).filter((cls) => !(cls in CSS_LESS_ALLOWLIST));
  expect(missing).toEqual([]);
});

test("the CSS-less allowlist stays honest (every entry is still emittable and still rule-less)", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  const selectors = cssClassSelectors(css);
  const emittable = new Set(ALL_EMITTABLE_CLASSES);
  // An allowlist entry the JS can no longer emit, or one that now HAS an app.css
  // rule (e.g. a #296 gap was closed), is dead weight — surface it for removal.
  const emittableButCovered = Object.keys(CSS_LESS_ALLOWLIST).filter((cls) =>
    selectors.has(cls),
  );
  const notEmittable = Object.keys(CSS_LESS_ALLOWLIST).filter(
    (cls) => !emittable.has(cls),
  );
  expect({ emittableButCovered, notEmittable }).toEqual({
    emittableButCovered: [],
    notEmittable: [],
  });
});

test("detail key/value rows reserve a content-sized label track", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(
    /\.detail \.kv \{[^}]*grid-template-columns: minmax\(130px, max-content\) minmax\(0, 1fr\);/s,
  );
  expect(css).toMatch(/\.detail \.kv dt \{[^}]*white-space: nowrap;/s);
  expect(css).toMatch(/\.detail \.kv dd \{[^}]*min-width: 0;/s);
  expect(css).toMatch(/\.detail \.kv dd \{[^}]*overflow-wrap: anywhere;/s);
});

test("applied-filter badges wrap long exact identities inside the disclosure", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(/\.filter-chips \.badge \{[^}]*max-width: 100%;/s);
  expect(css).toMatch(
    /\.filter-chips \.badge \{[^}]*overflow-wrap: anywhere;/s,
  );
});

test("unit cards read their padding from the density-aware card token", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(/\.unit-card \{[^}]*padding: var\(--card-pad\);/s);
});

test("Timeline rows read their padding only from the density-aware row token", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(/\.event \{[^}]*padding: var\(--row-pad\);/s);
  const compact = css.match(/html\.compact \{([^}]*)\}/s)?.[1] ?? "";
  expect(compact).not.toMatch(/--(?:row|card)-pad\s*:/);
});

test("bounded Change grids keep sparse result cards at their content height", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(/\.units \{[^}]*align-content: start;/s);
});

test("exact detail identities wrap below persistent non-scrolling controls", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(/\.detail \.mono \{[^}]*overflow-wrap: anywhere;/s);
  expect(css).toMatch(/\.detail \.mono \{[^}]*word-break: break-word;/s);
  expect(css).toMatch(/\.detail \{[^}]*display: flex;/s);
  expect(css).toMatch(/\.detail \{[^}]*flex-direction: column;/s);
  expect(css).toMatch(/\.detail \{[^}]*overflow: hidden;/s);
  expect(css).toMatch(/#detail-body \{[^}]*min-height: 0;/s);
  expect(css).toMatch(/#detail-body \{[^}]*overflow-y: auto;/s);
  expect(css).toMatch(/\.detail-head \{[^}]*background: var\(--bg\);/s);
});

test("narrow Attention cards keep their headline, reason, and exact Revision artifact inside the card", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(/\.units \{[^}]*min-width: 0;/s);
  expect(css).toMatch(/\.unit-card \{[^}]*min-width: 0;/s);
  expect(css).toMatch(/\.change-card-primary \{[^}]*min-width: 0;/s);
  expect(css).toMatch(
    /\.change-card-headline \{[^}]*overflow-wrap: anywhere;/s,
  );
  expect(css).toMatch(/\.change-card-attention \{[^}]*min-width: 0;/s);
  expect(css).toMatch(
    /\.change-card-attention-reason,[\s\S]*?\.change-card-attention-additional \{[^}]*overflow-wrap: anywhere;/s,
  );
  expect(css).toMatch(/\.change-card-current \{[^}]*overflow-wrap: anywhere;/s);
});

test("Attention reason groups style their server-owned headings", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(
    /\.attention-group-heading \{[^}]*font-size: var\(--fs-base\);/s,
  );
});

test("the card heading wrapper resets heading typography so the headline styling stays put", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(
    /\.unit-card > h3\.change-card-heading \{[^}]*margin: 0;[^}]*font-size: inherit;[^}]*font-weight: inherit;[^}]*font-family: inherit;/s,
  );
});

test("parallel exact Revision choices retain a visible at-rest action affordance", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  const peerActionRule =
    css.match(/\.change-card-peer-open \{([^}]*)\}/s)?.[1] ?? "";
  // These choices are rendered as `.ghost` buttons, whose base treatment is
  // intentionally borderless. Each peer choice therefore needs its own
  // persistent boundary and fill rather than relying on hover or focus to read
  // as an action. Naming the tokens also prevents a transparent border/fill
  // from satisfying the contract accidentally.
  expect(peerActionRule).toMatch(/border:\s*1px solid var\(--border\);/);
  expect(peerActionRule).toMatch(/background:\s*var\(--bg-row\);/);
});

test("Attention reasons use the AA-tuned warning color on their cream surface", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  // --warning-strong also serves fixed dark diagnostics, so the component must
  // choose the established light-safe warning foreground instead of retuning
  // that shared token.
  expect(css).toMatch(
    /\.change-card-attention-reason \{[^}]*color: var\(--warning\);/s,
  );
});

test("the compact preset overrides the card token", () => {
  const tokens = readFileSync(TOKENS_CSS_PATH, "utf8");
  expect(tokens).toMatch(/\.compact \{[^}]*--card-pad:/s);
});

test("the compact preset declares only the non-color rhythm tokens", () => {
  const tokens = readFileSync(TOKENS_CSS_PATH, "utf8");
  const block = tokens.match(/\.compact \{([^}]*)\}/s)?.[1] ?? "";
  const declared = [...block.matchAll(/(--[a-z-]+)\s*:/g)].map(
    (match) => match[1],
  );
  expect(declared.length).toBeGreaterThan(0);
  for (const property of declared) {
    expect(
      COMPACT_ALLOWED_PROPERTIES.has(property),
      `unexpected .compact property ${property}`,
    ).toBe(true);
  }
  for (const property of COMPACT_ALLOWED_PROPERTIES) {
    expect(declared).toContain(property);
  }
});

test("the lens row/card layout tracks are pinned", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(/\.event \{[^}]*grid-template-columns: 84px 12px 1fr;/s);
  expect(css).toMatch(
    /\.unit-card \.kv \{[^}]*grid-template-columns: 110px 1fr;/s,
  );
  expect(css).toMatch(/\.compact \.tier-medium \{[^}]*display: none;/s);
});

test("Decision context stays visible and resets inline annotation indentation", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(/\.diff-decision-context \{[^}]*display: grid;/s);
  expect(css).toMatch(/\.diff-decision-context \.anno \{[^}]*margin-left: 0;/s);
  expect(css).not.toMatch(/\.diff-decision-context \{[^}]*display: none;/s);
});
