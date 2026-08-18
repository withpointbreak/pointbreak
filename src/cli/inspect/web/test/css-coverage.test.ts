import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, test } from "vitest";
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

// ── Cluster-wide literal-class drift guard ──────────────────────────────────
// The registry test above covers only ALL_EMITTABLE_CLASSES tokens; raw class
// literals written at imperative emission sites used to have no guard at all.
// This scan closes that blind spot: every literal class token emitted by the
// active cluster must have an app.css selector or an explicit allowlist entry.
//
// Scope boundary: imperative sites only (`className = …`, `classList.*`,
// `setAttribute("class", …)`, including template literals in each). Inline
// `class="…"` attributes are deliberately out of scope — the cluster emits
// none, and HTML-template modules route their classes through `CLASS`, which
// the registry guard above already covers.

interface BoundaryModule {
  path: string;
  activeImport: boolean;
}

// Enumerate governed modules from the architecture inventory, not a glob: a
// newly-reachable module is scanned the moment the inventory records it.
const ACTIVE_MODULES = (
  JSON.parse(
    readFileSync(
      resolve(process.cwd(), "src/change-inspector-architecture.json"),
      "utf8",
    ),
  ) as { modules: BoundaryModule[] }
).modules
  .filter((module) => module.activeImport)
  .map((module) => resolve(process.cwd(), "src", module.path));

const CLASS_NAME_STRING = /\.className\s*=\s*"([^"]*)"/g;
const CLASS_NAME_TEMPLATE = /\.className\s*=\s*`([^`]*)`/g;
// The argument group consumes complete quoted and backtick literals before it
// can meet a bare `)`, so a call whose interpolation itself contains
// parentheses — classList.add(`status-${normalize(status)}`) — is captured
// whole instead of truncating at the inner `)`.
const CLASS_LIST_CALL =
  /\.classList\.(?:add|remove|toggle|replace|contains)\(\s*((?:"[^"]*"|`[^`]*`|[^)"`])*)\)/g;
const SET_CLASS_ATTRIBUTE =
  /setAttribute\(\s*"class"\s*,\s*("[^"]*"|`[^`]*`)\s*\)/g;
const INTERPOLATION = /\$\{[^}]*\}/g;

// Splits a template-literal body into complete class tokens and dynamic family
// prefixes. A token abutting an interpolation boundary with no intervening
// whitespace is incomplete: it is dropped from the token set and recorded as a
// family prefix that must be declared in DYNAMIC_CLASS_FAMILIES. Limitations
// (deliberate): an interpolation with nested braces, or a nested template
// inside an interpolation, leaks garbage fragments — those then fail loudly
// against app.css, which is the desired behavior.
function splitTemplateBody(body: string): {
  tokens: string[];
  families: string[];
} {
  const tokens: string[] = [];
  const families: string[] = [];
  const segments = body.split(INTERPOLATION);
  segments.forEach((segment, index) => {
    const parts = segment.split(/\s+/).filter((part) => part.length > 0);
    if (parts.length === 0) return;
    const abutsPrevious = index > 0 && !/^\s/.test(segment);
    const abutsNext = index < segments.length - 1 && !/\s$/.test(segment);
    let first = 0;
    let last = parts.length - 1;
    if (abutsPrevious) {
      families.push(parts[first]);
      first += 1;
    }
    if (abutsNext && last >= first) {
      families.push(parts[last]);
      last -= 1;
    }
    for (let i = first; i <= last; i += 1) tokens.push(parts[i]);
  });
  return { tokens, families };
}

function splitPlainValue(value: string): string[] {
  return value.split(/\s+/).filter((token) => token.length > 0);
}

function scanLiteralClassTokens(sources: readonly string[]): {
  tokens: Set<string>;
  families: Set<string>;
} {
  const tokens = new Set<string>();
  const families = new Set<string>();
  const addPlain = (value: string) => {
    for (const token of splitPlainValue(value)) tokens.add(token);
  };
  const addTemplate = (body: string) => {
    const split = splitTemplateBody(body);
    for (const token of split.tokens) tokens.add(token);
    for (const family of split.families) families.add(family);
  };
  for (const source of sources) {
    for (const match of source.matchAll(CLASS_NAME_STRING)) addPlain(match[1]);
    for (const match of source.matchAll(CLASS_NAME_TEMPLATE))
      addTemplate(match[1]);
    for (const match of source.matchAll(CLASS_LIST_CALL)) {
      const args = match[1];
      for (const quoted of args.matchAll(/"([^"]*)"/g)) addPlain(quoted[1]);
      for (const template of args.matchAll(/`([^`]*)`/g))
        addTemplate(template[1]);
    }
    for (const match of source.matchAll(SET_CLASS_ATTRIBUTE)) {
      const value = match[1];
      if (value.startsWith('"')) addPlain(value.slice(1, -1));
      else addTemplate(value.slice(1, -1));
    }
  }
  return { tokens, families };
}

function scanActiveModules(): { tokens: Set<string>; families: Set<string> } {
  return scanLiteralClassTokens(
    ACTIVE_MODULES.map((path) => readFileSync(path, "utf8")),
  );
}

// The raw cssClassSelectors regex also matches `.token` mentions inside CSS
// comments, so a deleted rule whose name survives in a comment would still
// look covered. The literal guard therefore strips comments first. (The
// registry test above keeps the raw set; tightening it is out of scope.)
function commentStrippedCssClassSelectors(css: string): Set<string> {
  return cssClassSelectors(css.replace(/\/\*[\s\S]*?\*\//g, ""));
}

// Literal-emitted classes with deliberately no app.css rule, each with a
// one-line reason. Same contract as CSS_LESS_ALLOWLIST above: an entry that
// gains a rule or stops being emitted fails the honesty test below.
const LITERAL_CSS_LESS_ALLOWLIST: Record<string, string> = {
  "change-filter":
    "Change-lens facet visibility hook (change-inspector-render.ts:346,359); queried at :718/:776 only to toggle `.hidden` — a behavior marker, never styled",
  "timeline-filter":
    "Timeline-lens facet visibility hook (change-inspector-render.ts:350,354,400); same behavior-marker contract as change-filter",
};

// Dynamic template families whose members cannot be read from the literal
// alone. Each family's members must be enumerated in ALL_EMITTABLE_CLASSES so
// the registry test covers them; this declaration only pins that the family
// exists and is accounted for.
const DYNAMIC_CLASS_FAMILIES: Record<string, string> = {
  "verify-":
    "`verify verify-${status}` (change-inspector-timeline.ts) — members enumerated by VERIFY_STATUSES/verifyClass in ALL_EMITTABLE_CLASSES",
  "type-facet-row":
    '`type-facet-row${… " type-facet-row-off"}` (change-inspector-render.ts) — members enumerated by typeFacetRowClass in ALL_EMITTABLE_CLASSES',
};

describe("literal class extraction", () => {
  test("splits plain className strings into complete tokens", () => {
    const scan = scanLiteralClassTokens([
      'element.className = "unit-card selected";',
    ]);
    expect([...scan.tokens].sort()).toEqual(["selected", "unit-card"]);
    expect([...scan.families]).toEqual([]);
  });

  test("keeps whitespace-separated template tokens and drops abutting fragments as families", () => {
    const scan = scanLiteralClassTokens([
      "element.className = `verify verify-${status}`;",
    ]);
    expect([...scan.tokens]).toEqual(["verify"]);
    expect([...scan.families]).toEqual(["verify-"]);
  });

  test("records a leading interpolation's abutting fragment as a family", () => {
    const scan = scanLiteralClassTokens([
      "element.className = `${kind}-chip plain`;",
    ]);
    expect([...scan.tokens]).toEqual(["plain"]);
    expect([...scan.families]).toEqual(["-chip"]);
  });

  test("treats a whole-segment template token followed by an interpolation as a family", () => {
    const scan = scanLiteralClassTokens([
      'element.className = `type-facet-row${off ? " type-facet-row-off" : ""}`;',
    ]);
    expect([...scan.tokens]).toEqual([]);
    expect([...scan.families]).toEqual(["type-facet-row"]);
  });

  test("captures quoted and template classList arguments, surviving interpolated parentheses", () => {
    const scan = scanLiteralClassTokens([
      'element.classList.add("hidden", `status-${normalize(status)}`);',
    ]);
    expect([...scan.tokens]).toEqual(["hidden"]);
    expect([...scan.families]).toEqual(["status-"]);
  });

  test("captures quoted and template setAttribute class values", () => {
    const scan = scanLiteralClassTokens([
      'element.setAttribute("class", "badge mono");',
      'element.setAttribute("class", `badge badge-${tone}`);',
    ]);
    expect([...scan.tokens].sort()).toEqual(["badge", "mono"]);
    expect([...scan.families]).toEqual(["badge-"]);
  });

  test("a class named only inside a CSS comment counts as missing", () => {
    const selectors = commentStrippedCssClassSelectors(
      "/* .ghost-token was retired */ .real-token { color: red; }",
    );
    expect(selectors.has("real-token")).toBe(true);
    expect(selectors.has("ghost-token")).toBe(false);
  });
});

test("every raw class literal in the active cluster has an app.css selector (or an allowlist entry)", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  const selectors = commentStrippedCssClassSelectors(css);
  const scan = scanActiveModules();
  const missing = [...scan.tokens]
    .filter((token) => !selectors.has(token))
    .filter((token) => !(token in LITERAL_CSS_LESS_ALLOWLIST))
    .sort();
  expect(missing).toEqual([]);
});

test("the literal allowlist stays honest (every entry is still emitted and still rule-less)", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  const selectors = commentStrippedCssClassSelectors(css);
  const scan = scanActiveModules();
  const emittedButCovered = Object.keys(LITERAL_CSS_LESS_ALLOWLIST).filter(
    (cls) => selectors.has(cls),
  );
  const notEmitted = Object.keys(LITERAL_CSS_LESS_ALLOWLIST).filter(
    (cls) => !scan.tokens.has(cls),
  );
  expect({ emittedButCovered, notEmitted }).toEqual({
    emittedButCovered: [],
    notEmitted: [],
  });
});

test("every unresolved dynamic class family is declared", () => {
  const scan = scanActiveModules();
  expect([...scan.families].sort()).toEqual(
    Object.keys(DYNAMIC_CLASS_FAMILIES).sort(),
  );
});

test("de-emphasized and notice prose classes carry their theme-flipping colors", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(/\.dim \{[^}]*color: var\(--fg-dim\);/s);
  expect(css).toMatch(/\.info \{[^}]*color: var\(--info\);/s);
  expect(css).toMatch(/\.warning \{[^}]*color: var\(--warning\);/s);
});

test("detail-pane section wrappers share the governed section rhythm", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(
    /\.detail-facts,\s*\.detail-current-revisions,\s*\.captured-diff \{[^}]*margin: 0 0 14px;/s,
  );
});

test("dynamic detail headings get zero-specificity typography that section rules out-rank", () => {
  const css = readFileSync(APP_CSS_PATH, "utf8");
  expect(css).toMatch(
    /:where\(\.detail\) h3 \{[^}]*margin: 0 0 6px;[^}]*font-size: var\(--fs-base\);/s,
  );
  expect(css).toMatch(
    /:where\(\.detail\) h4 \{[^}]*margin: 10px 0 4px;[^}]*font-size: var\(--fs-md\);/s,
  );
  expect(css).toMatch(
    /:where\(\.detail\) h5 \{[^}]*margin: 0 0 4px;[^}]*font-size: var\(--fs-md\);/s,
  );
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
