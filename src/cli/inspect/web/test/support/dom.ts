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
  <div class="brand">
    <span class="brand-mark" aria-hidden="true"></span>
    <span class="brand-text">Pointbreak<span class="brand-accent">Review</span></span>
  </div>
  <nav id="lens-switcher" aria-label="master pane lens">
    <div class="lens-group-record">
      <button class="lens-tab" type="button" data-lens="timeline" aria-pressed="true">Timeline</button>
      <button class="lens-tab" type="button" data-lens="list" aria-pressed="false">Revisions</button>
    </div>
    <button class="lens-tab" type="button" data-lens="attention" aria-pressed="false">Attention</button>
  </nav>
  <div class="stats">
    <div id="store-identity" class="store-identity">
      <span id="store-chip" class="store-identity-chip" tabindex="0" aria-label="local review server">
        <span id="refresh" class="store-live" data-state="idle" title="Auto-refresh status" aria-hidden="true"></span>
        <span id="store-chip-repo" class="store-identity-repo">local server</span>
        <span class="store-identity-caret" aria-hidden="true">▾</span>
      </span>
      <div class="store-identity-detail" aria-hidden="true">
        <dl id="store-identity-rows"></dl>
        <div class="store-identity-stats">
          <span id="stat-events" class="stat" title="durable events in the store">— events</span>
          <span id="stat-units" class="stat" title="captured revisions">— units</span>
          <span id="stat-threads" class="stat" title="supersession threads">— threads</span>
          <span id="stat-hash" class="stat mono" title="eventSetHash">—</span>
        </div>
        <dl class="connection-status-rows">
          <dt>connection</dt><dd id="connection-status">connecting</dd>
          <dt>refresh</dt><dd id="refresh-status">idle</dd>
        </dl>
        <span id="stat-live" class="hidden" data-state="idle" aria-hidden="true">idle</span>
        <div class="connection-actions">
          <button id="connection-action" type="button" class="ghost hidden"></button>
          <button id="connect-another" type="button" class="ghost hidden">Connect to another server</button>
        </div>
        <p class="store-identity-note">Recorded state only — never gates writes; signature checks are reader-relative to your committed allow-list.</p>
      </div>
    </div>
    <span id="refresh-word" class="store-live-word" role="status" aria-live="polite"></span>
    <div id="view-controls" class="disclosure">
      <button id="view-toggle" class="ghost disclosure-toggle" type="button" aria-haspopup="dialog" aria-expanded="false" aria-controls="view-panel">View · newest</button>
      <div id="view-panel" class="control-panel view-panel hidden" role="dialog" aria-label="View controls">
        <section id="view-order-section" class="control-section" aria-labelledby="view-order-label">
          <h2 id="view-order-label" class="control-heading">Order</h2>
          <div class="control-choices">
            <label><input id="order-newest" type="radio" name="view-order" value="desc" checked /> Newest first</label>
            <label><input id="order-oldest" type="radio" name="view-order" value="asc" /> Oldest first</label>
          </div>
        </section>
        <section id="view-sort-section" class="control-section hidden" aria-labelledby="view-sort-label">
          <label id="view-sort-label" class="control-heading" for="sort-picker">Sort revisions by</label>
          <select id="sort-picker" aria-label="sort revisions by">
            <option value="captured">captured</option>
            <option value="activity">latest activity</option>
          </select>
        </section>
        <section class="control-section" aria-labelledby="view-navigation-label">
          <h2 id="view-navigation-label" class="control-heading">Navigate</h2>
          <div class="control-actions">
            <button id="jump-latest" class="ghost" type="button">Latest</button>
            <button id="jump-oldest" class="ghost" type="button">Oldest</button>
          </div>
        </section>
        <section class="control-section" aria-labelledby="view-display-label">
          <h2 id="view-display-label" class="control-heading">Display</h2>
          <div class="control-subsection">
            <span class="control-label">Theme</span>
            <div class="control-choices control-choices-inline">
              <label><input id="theme-system" type="radio" name="theme-mode" value="system" checked /> System</label>
              <label><input id="theme-light" type="radio" name="theme-mode" value="light" /> Light</label>
              <label><input id="theme-dark" type="radio" name="theme-mode" value="dark" /> Dark</label>
            </div>
          </div>
          <div class="control-subsection">
            <span class="control-label">Density</span>
            <div class="control-choices control-choices-inline">
              <label><input id="density-comfortable" type="radio" name="density-mode" value="comfortable" checked /> Comfortable</label>
              <label><input id="density-compact" type="radio" name="density-mode" value="compact" /> Compact</label>
            </div>
          </div>
        </section>
      </div>
    </div>
  </div>
</header>

<div id="diagnostics" class="diagnostics hidden"></div>
<div id="route-diagnostic" class="route-diagnostic hidden" role="status" aria-live="polite"></div>

<main>
  <div id="toolbar" class="toolbar">
    <input
      id="filter-text"
      type="search"
      placeholder="search — text or field:value"
    />
    <ul id="filter-suggestions" class="filter-suggestions hidden" aria-label="search suggestions"></ul>
    <div id="filter-controls" class="disclosure filter-controls">
      <button id="filters-toggle" type="button" class="ghost disclosure-toggle" aria-haspopup="dialog" aria-expanded="false" aria-controls="filters-panel">Filters</button>
      <div id="filters-panel" class="control-panel filters-panel hidden" role="dialog" aria-label="Filters">
        <section class="control-section" aria-labelledby="applied-filters-label">
          <h2 id="applied-filters-label" class="control-heading">Applied filters</h2>
          <div id="filter-chips" class="filter-chips" aria-label="applied filters"></div>
          <p id="filter-chips-empty" class="control-empty">No structured filters</p>
        </section>
        <section id="filter-types" class="control-section type-facet" aria-labelledby="filter-types-label">
          <h2 id="filter-types-label" class="control-heading">Event types</h2>
          <ul id="filter-types-menu" class="type-facet-menu" aria-label="event types"></ul>
        </section>
        <div id="filter-footer" class="control-footer">
          <button id="filter-clear" class="ghost" type="button">Clear all</button>
        </div>
      </div>
    </div>
    <button id="follow-toggle" class="ghost follow-toggle" type="button" aria-pressed="true">Following</button>
  </div>
  <div class="split">
    <button id="master-rail" class="master-rail" aria-label="Show timeline" title="Show timeline">›</button>
    <section id="master" class="master" aria-label="master pane"></section>
    <div class="divider" role="separator" aria-orientation="vertical" tabindex="0"
         aria-label="Resize panes — arrow keys adjust, Enter resets"
         aria-valuenow="50" aria-valuemin="25" aria-valuemax="75"></div>
    <aside id="detail" class="detail">
      <header class="detail-head">
        <button id="detail-back" class="ghost detail-back" aria-label="Back to timeline">‹ timeline</button>
        <button id="detail-read" class="ghost" aria-label="Reading mode" title="Reading mode">⤢</button>
        <button id="detail-close" class="ghost" aria-label="Close detail" title="Close detail (Esc)">✕</button>
      </header>
      <div id="detail-body">
        <p class="empty">Select an event or revision to inspect.</p>
      </div>
    </aside>
  </div>
  <section id="diff-page" class="diff-page hidden" aria-label="annotated diff">
    <header class="diff-page-head">
      <span id="diff-page-title" class="mono"></span>
      <button id="diff-page-close" class="ghost" aria-label="Back to the record">‹ back</button>
    </header>
    <div class="diff-layout">
      <nav id="diff-page-nav" class="diff-nav" aria-label="diff files">
        <input
          id="diff-file-query"
          type="search"
          class="diff-file-search"
          placeholder="filter files — path, change:, has:facts, is:unanchored"
          aria-label="filter diff files"
        />
        <div id="diff-page-nav-list"></div>
      </nav>
      <div id="diff-page-body" class="diff-body"></div>
    </div>
  </section>
</main>

<div id="error" class="error hidden"></div>

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
      <dt><kbd>1</kbd> / <kbd>2</kbd> / <kbd>3</kbd></dt><dd>jump to the timeline / revisions / attention lens</dd>
      <dt><kbd>g</kbd> / <kbd>G</kbd></dt><dd>jump to the top / bottom of the timeline</dd>
      <dt><kbd>f</kbd> / <kbd>b</kbd></dt><dd>page forward / backward through visible timeline entries</dd>
      <dt><kbd>d</kbd> / <kbd>u</kbd></dt><dd>move half a page down / up through visible timeline entries</dd>
      <dt><kbd>h</kbd> / <kbd>l</kbd></dt><dd>resize the split: shrink / grow the timeline pane</dd>
      <dt><kbd>Enter</kbd></dt><dd>open the cursor's detail, then its snapshot diff</dd>
      <dt><kbd>n</kbd> / <kbd>p</kbd></dt><dd>jump to the next / previous review fact in the diff</dd>
      <dt><kbd>]</kbd> / <kbd>[</kbd></dt><dd>jump to the next / previous change in the diff</dd>
      <dt><kbd>/</kbd></dt><dd>focus the search box</dd>
      <dt><kbd>Space</kbd> / <kbd>Shift</kbd>+<kbd>Space</kbd></dt><dd>scroll the open detail pane</dd>
      <dt><kbd>Esc</kbd></dt><dd>close the diff page or an overlay, restore the split, close the detail, clear the cursor, then the query</dd>
      <dt><kbd>?</kbd></dt><dd>toggle this cheat sheet</dd>
    </dl>
    <div class="key-help-workflow">
      <h3>Review stages and the CLI</h3>
      <p>Work -> Claims -> Evidence -> Questions -> Call maps to <code>capture</code>/<code>revision</code>/<code>inspect</code>, then <code>observation</code>, <code>validation</code>, <code>input-request</code>, and <code>assessment</code>. <code>attention</code> lists outstanding judgment; <code>association</code> records where the same revision landed. Validation is evidence, never a verdict or merge gate.</p>
      <p>Review is local, read-only, and advisory: it shows and copies commands but never runs them. Copied commands keep visible placeholder tokens like <code>&lt;your-track&gt;</code> — replace each placeholder before running.</p>
    </div>
  </div>
</div>

<div id="reconnect-dialog" class="modal hidden" role="dialog" aria-modal="true" aria-labelledby="reconnect-title">
  <form class="modal-card reconnect-card">
    <h2 id="reconnect-title">Connect to Pointbreak Review</h2>
    <p>Enter a token or an HTTP loopback capability URL.</p>
    <input id="reconnect-input" type="password" autocomplete="off" spellcheck="false" aria-label="token or capability URL" />
    <p id="reconnect-error" class="error hidden" role="alert"></p>
    <div class="reconnect-actions">
      <button id="reconnect-cancel" type="button" class="ghost">Cancel</button>
      <button id="reconnect-submit" type="submit">Reconnect</button>
    </div>
  </form>
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
  root.style.removeProperty("--split-master");
}
