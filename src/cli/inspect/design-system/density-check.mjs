// Manual density release gate for Pointbreak Review (NOT CI).
// Audits the live token source, then bakes the pairs used by the human pass.
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { declarations } from "./contrast-check.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const designSystemDirectory = path.dirname(scriptPath);
const tokenPath = path.resolve(designSystemDirectory, "../assets/tokens.css");
const stylePath = path.resolve(designSystemDirectory, "styles.css");
const bodyDirectory = path.resolve(designSystemDirectory, "_bodies");
const outputDirectory = path.resolve(designSystemDirectory, "output-density");

// Keep this in lockstep with COMPACT_ALLOWED_PROPERTIES in
// web/test/css-coverage.test.ts. The test owns CI; this script owns the manual gate.
const ALLOWED_COMPACT_PROPERTIES = new Set([
  "--row-pad",
  "--line",
  "--card-pad",
]);

const galleries = [
  { body: "data-timeline.body.html", name: "timeline", title: "Timeline" },
  { body: "data-cards.body.html", name: "changes", title: "Changes" },
  { body: "data-attention.body.html", name: "attention", title: "Attention" },
];
const themes = ["dark", "light"];

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function densityTokens(css) {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const comfortableBlock = withoutComments.match(
    /:root\s*,\s*\[data-theme\s*=\s*["']dark["']\]\s*\{([\s\S]*?)\}/i,
  );
  const compactBlock = withoutComments.match(/\.compact\s*\{([\s\S]*?)\}/i);

  assert(comfortableBlock, `${tokenPath}: missing :root/[data-theme="dark"] block`);
  assert(compactBlock, `${tokenPath}: missing .compact block`);

  const comfortable = declarations(comfortableBlock[1], "comfortable density");
  const compact = declarations(compactBlock[1], "compact density");

  for (const property of compact.keys()) {
    assert(
      ALLOWED_COMPACT_PROPERTIES.has(property),
      `.compact declares disallowed property ${property}`,
    );
  }
  for (const property of ALLOWED_COMPACT_PROPERTIES) {
    assert(compact.has(property), `.compact is missing required property ${property}`);
    assert(
      comfortable.has(property),
      `comfortable density is missing required property ${property}`,
    );
  }

  return { comfortable, compact };
}

function printDensityTable({ comfortable, compact }) {
  console.log("property   | comfortable | compact");
  console.log("-----------|-------------|---------");
  for (const property of ALLOWED_COMPACT_PROPERTIES) {
    console.log(
      `${property.padEnd(10)} | ${comfortable.get(property).padEnd(11)} | ${compact.get(property)}`,
    );
  }
}

function page({ body, density, styles, theme, title, tokens }) {
  const compactClass = density === "compact" ? ' class="compact"' : "";
  return `<!doctype html>
<html lang="en" data-theme="${theme}"${compactClass}>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <link rel="icon" href="data:," />
    <title>${title} — ${theme} ${density}</title>
    <style>
${tokens}
${styles}
    </style>
  </head>
  <body>
    <div class="ds-card">
${body}
    </div>
  </body>
</html>
`;
}

function indexPage() {
  const pairs = themes
    .flatMap((theme) =>
      galleries.map(
        ({ name, title }) => `
    <section>
      <h2>${title} — ${theme}</h2>
      <div class="pair">
        <div><h3>comfortable</h3><iframe title="${title} ${theme} comfortable" src="${name}-${theme}-comfortable.html"></iframe></div>
        <div><h3>compact</h3><iframe title="${title} ${theme} compact" src="${name}-${theme}-compact.html"></iframe></div>
      </div>
    </section>`,
      ),
    )
    .join("");

  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <link rel="icon" href="data:," />
    <title>Pointbreak Review density comparison</title>
    <style>
      :root { color-scheme: dark; font-family: system-ui, sans-serif; background: #080c0d; color: #e5ebe7; }
      body { margin: 0; padding: 24px; }
      h1 { margin: 0 0 8px; font-size: 24px; }
      p { margin: 0 0 24px; color: #a5b2ad; }
      section { margin-bottom: 28px; }
      h2 { margin: 0 0 8px; font-size: 16px; text-transform: capitalize; }
      h3 { margin: 0 0 6px; color: #a5b2ad; font-size: 13px; text-transform: uppercase; letter-spacing: .06em; }
      .pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; }
      iframe { width: 100%; height: 720px; border: 1px solid #2d3d39; border-radius: 8px; background: #080c0d; }
      @media (max-width: 900px) { .pair { grid-template-columns: 1fr; } }
    </style>
  </head>
  <body>
    <h1>Pointbreak Review density comparison</h1>
    <p>Comfortable on the left; compact on the right. Inspect every pair before promotion.</p>${pairs}
  </body>
</html>
`;
}

async function bake(tokens, styles) {
  await rm(outputDirectory, { force: true, recursive: true });
  await mkdir(outputDirectory, { recursive: true });

  for (const gallery of galleries) {
    const body = await readFile(path.resolve(bodyDirectory, gallery.body), "utf8");
    for (const theme of themes) {
      for (const density of ["comfortable", "compact"]) {
        const filename = `${gallery.name}-${theme}-${density}.html`;
        await writeFile(
          path.resolve(outputDirectory, filename),
          page({
            body,
            density,
            styles,
            theme,
            title: gallery.title,
            tokens,
          }),
        );
        console.log(`baked ${filename}`);
      }
    }
  }

  await writeFile(path.resolve(outputDirectory, "index.html"), indexPage());
  console.log(`density output: ${outputDirectory}`);
}

async function main() {
  assert(process.argv.length === 2, "usage: node design-system/density-check.mjs");
  const [tokens, styles] = await Promise.all([
    readFile(tokenPath, "utf8"),
    readFile(stylePath, "utf8"),
  ]);

  printDensityTable(densityTokens(tokens));
  console.log("");
  await bake(tokens, styles);
}

if (path.resolve(process.argv[1] ?? "") === scriptPath) {
  main().catch((error) => {
    console.error(`Review density audit failed: ${error.message}`);
    process.exitCode = 1;
  });
}
