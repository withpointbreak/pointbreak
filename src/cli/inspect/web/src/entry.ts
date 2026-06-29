// The esbuild bundle entry point. The composition root (`main`) does not auto-run
// so it stays testable; this entry is the single invoker — it calls `main()` and
// ignores the returned load chain, exactly as the served `<script src="/app.js">`
// (at the end of <body>) does at eval.

import { main } from "./main";

void main();
