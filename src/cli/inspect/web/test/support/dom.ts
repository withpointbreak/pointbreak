// Test harness: mount the inspector's fixed-id DOM under happy-dom so the impure
// modules (render / overlay / diff / palette / prefs …) have the structure they
// render into and wire listeners onto. This is an append-only shared surface —
// extend it with ids as new controls are wired; never rewrite an existing
// section, and keep the markup a faithful **mirror of assets/index.html** (a
// missing fixed id is a harness gap, not a module bug).
//
// The render-injected lens bodies (`#timeline` and the list/threads bodies) are
// deliberately absent: `renderMaster` creates them inside `#master`, so the
// static mount leaves `#master` empty, exactly as index.html does.

/** A verbatim mirror of `assets/index.html`'s `<body>` (minus the `<script>` tag). */
const INDEX_BODY = `
<header id="topbar">
  <div class="brand">shore<span>inspector</span></div>
  <nav id="lens-switcher" aria-label="master pane lens">
    <button class="lens-tab" type="button" data-lens="timeline" aria-pressed="true">Timeline</button>
    <button class="lens-tab" type="button" data-lens="list" aria-pressed="false">Revisions</button>
    <button class="lens-tab" type="button" data-lens="threads" aria-pressed="false">Threads</button>
  </nav>
  <div class="stats">
    <span id="stat-events" class="stat" title="durable events in the store">— events</span>
    <span id="stat-units" class="stat" title="captured revisions">— units</span>
    <span id="stat-threads" class="stat" title="supersession threads">— threads</span>
    <span id="stat-hash" class="stat mono" title="eventSetHash">—</span>
    <span id="refresh" class="stat" title="auto-refresh status">idle</span>
    <span id="advisory-mode" class="stat advisory-mode" title="the inspector reports recorded state; it never gates a write, and verification is reader-relative">read-only · advisory</span>
    <button id="theme-toggle" class="ghost" aria-label="Toggle color theme" title="Toggle light/dark theme">theme</button>
    <button id="density-toggle" class="ghost" aria-label="Toggle density" title="Toggle comfortable/compact density">density</button>
  </div>
</header>

<div id="diagnostics" class="diagnostics hidden"></div>
<div id="route-diagnostic" class="route-diagnostic hidden" role="status" aria-live="polite"></div>

<main>
  <div id="toolbar" class="toolbar">
    <input
      id="filter-text"
      type="search"
      placeholder="search — text, or field:value (type: track: revision: snapshot: status:)"
    />
    <div id="filter-types" class="type-toggles"></div>
    <button id="order-toggle" class="ghost" title="toggle timeline order">newest first</button>
    <button id="filter-clear" class="ghost">clear</button>
  </div>
  <div class="split">
    <section id="master" class="master" aria-label="master pane"></section>
    <aside id="detail" class="detail">
      <p class="empty">Select an event or revision to inspect.</p>
    </aside>
  </div>
</main>

<div id="error" class="error hidden"></div>

<div id="diff-modal" class="modal hidden" role="dialog" aria-modal="true" aria-labelledby="diff-title">
  <div class="modal-card">
    <header class="modal-head">
      <span id="diff-title" class="mono"></span>
      <button id="diff-close" class="ghost" aria-label="close diff">close</button>
    </header>
    <div class="diff-layout">
      <nav id="diff-nav" class="diff-nav" aria-label="diff files"></nav>
      <div id="diff-body" class="diff-body"></div>
    </div>
  </div>
</div>

<div id="cmd-palette" class="modal hidden" role="dialog" aria-modal="true" aria-label="Command palette">
  <div class="modal-card cmd-card">
    <input
      id="cmd-input"
      class="cmd-input"
      type="text"
      role="combobox"
      aria-expanded="true"
      aria-controls="cmd-results"
      aria-autocomplete="list"
      placeholder="Jump to a revision, snapshot, track, or run a command…"
    />
    <ul id="cmd-results" class="cmd-results" role="listbox" aria-label="commands"></ul>
  </div>
</div>

<div id="key-help" class="modal hidden" role="dialog" aria-modal="true" aria-label="Keyboard shortcuts">
  <div class="modal-card key-help-card">
    <header class="modal-head">
      <h2>Keyboard shortcuts</h2>
      <button id="key-help-close" class="ghost">close</button>
    </header>
    <dl class="key-help-list">
      <dt><kbd>Cmd</kbd> / <kbd>Ctrl</kbd> + <kbd>K</kbd></dt><dd>open the command palette</dd>
      <dt><kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>P</kbd></dt><dd>open the command palette</dd>
      <dt><kbd>j</kbd> / <kbd>k</kbd></dt><dd>move the selection down / up in the active lens</dd>
      <dt><kbd>Enter</kbd></dt><dd>open the selection's snapshot diff</dd>
      <dt><kbd>n</kbd> / <kbd>p</kbd></dt><dd>jump to the next / previous review fact in the diff</dd>
      <dt><kbd>]</kbd> / <kbd>[</kbd></dt><dd>jump to the next / previous change in the diff</dd>
      <dt><kbd>/</kbd></dt><dd>focus the search box</dd>
      <dt><kbd>g</kbd> then <kbd>t</kbd> / <kbd>l</kbd> / <kbd>r</kbd></dt><dd>jump to the timeline / list / threads lens</dd>
      <dt><kbd>Esc</kbd></dt><dd>close the diff, then this sheet, then blur, then clear the query</dd>
      <dt><kbd>?</kbd></dt><dd>toggle this cheat sheet</dd>
    </dl>
  </div>
</div>
`;

/**
 * Replace `document.body` with the inspector's static fixed-id shell. Idempotent:
 * a second call replaces (never duplicates) the prior mount.
 */
export function mountInspectorDom(): void {
  document.body.innerHTML = INDEX_BODY;
}

/**
 * Clear the mounted DOM and the prefs-applied root state, so each test starts from
 * a clean `document`. Call in an `afterEach` (or before a fresh `mountInspectorDom`).
 */
export function resetDom(): void {
  document.body.innerHTML = "";
  const root = document.documentElement;
  root.removeAttribute("data-theme");
  root.classList.remove("compact");
}
