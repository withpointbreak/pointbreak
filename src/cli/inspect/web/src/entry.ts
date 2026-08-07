// The esbuild bundle entry point. The served capable reader deliberately starts
// with profile negotiation and never falls back to the legacy aggregate UI:
// legacy and in-progress stores render only their typed transition status.
// Activating the new store cohort is a separate, explicit operator transition,
// so shipping this reader before that transition cannot expose mixed semantics.
// The composition root does not auto-run so it stays testable; this entry is the
// single invoker, exactly as the served `<script src="/app.js">` does.

import { bootstrapChangeInspector } from "./change-inspector";

void bootstrapChangeInspector();
