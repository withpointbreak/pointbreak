// Shared esbuild options for the inspector bundle. Imported by the build script
// (build.mjs) and the determinism test (test/build.test.ts), so the committed
// artifact and the freshness-gate check build from one option set — they cannot
// drift. The bundle is a non-minified, keep-names IIFE so it stays diffable and the
// served `<script src="/app.js">` boots it at eval. Only `outfile` differs between
// the writers (it does not affect the emitted bytes).
/** @type {import("esbuild").BuildOptions} */
export const buildOptions = {
  entryPoints: ["src/entry.ts"],
  bundle: true,
  format: "iife",
  keepNames: true,
  minify: false,
  target: "es2022",
  charset: "utf8",
  legalComments: "none",
};
