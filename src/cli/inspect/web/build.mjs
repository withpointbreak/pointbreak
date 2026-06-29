// Build the inspector bundle: esbuild → the committed src/cli/inspect/assets/app.js.
// Run via `npm run build` / `just web-build` after editing web/src. Uses the shared
// esbuild.config.mjs options so the output matches the determinism test exactly.
import { build } from "esbuild";
import { buildOptions } from "./esbuild.config.mjs";

await build({ ...buildOptions, outfile: "../assets/app.js" });
