import { createHash } from "node:crypto";
import {
  type Disposable,
  type Event,
  EventEmitter,
  ViewColumn,
  type WebviewPanel,
  window,
} from "vscode";
import type {
  ChangeDetailDoc,
  ChangeRevisionDoc,
  RevisionRefV1,
} from "./changeProtocol";
import type { InspectChildManager } from "./inspectChild";
import { InspectClientError } from "./inspectClient";
import type { ResolvedTargetResolution } from "./targetResolver";

const VIEW_TYPE = "pointbreak.changeRevision";
const LOAD_ERROR = "Pointbreak could not load this exact Change revision.";

export interface ChangeRevisionPanelLocation {
  readonly resolution: ResolvedTargetResolution;
  readonly changeId: string;
  readonly revision: RevisionRefV1;
  readonly projectionStamp: string;
}

interface PanelSession {
  readonly key: string;
  readonly panel: WebviewPanel;
  readonly location: ChangeRevisionPanelLocation;
  active: boolean;
  visible: boolean;
}

/**
 * Owns immutable contextual Revision panels. A panel key includes every
 * identity scalar required to prevent a refresh from silently retargeting the
 * presentation to another Change, Revision artifact, or projection generation.
 */
export class ChangeResourcePanelManager implements Disposable {
  private readonly visibilityEmitter = new EventEmitter<boolean>();
  readonly onDidChangeVisibility: Event<boolean> = this.visibilityEmitter.event;
  private readonly sessions = new Map<string, PanelSession>();
  private reportedVisibility = false;
  private disposed = false;

  constructor(private readonly inspectManager: InspectChildManager) {}

  async open(location: ChangeRevisionPanelLocation): Promise<void> {
    if (this.disposed) {
      throw new Error("Pointbreak Review is no longer available.");
    }
    let change: ChangeDetailDoc;
    let revision: ChangeRevisionDoc;
    try {
      const { client } = await this.inspectManager.ensure(location.resolution);
      [change, revision] = await Promise.all([
        client.changeDetail(location.changeId, location.projectionStamp),
        client.changeRevision(
          location.changeId,
          location.revision,
          location.projectionStamp,
        ),
      ]);
    } catch (error) {
      await window.showErrorMessage(
        error instanceof InspectClientError ? error.message : LOAD_ERROR,
      );
      return;
    }

    const key = changeRevisionPanelKey(
      location,
      revision.exactRevisionDocument.cacheKey,
    );
    const existing = this.sessions.get(key);
    if (existing) {
      existing.panel.reveal(ViewColumn.Active);
      return;
    }

    const panel = window.createWebviewPanel(
      VIEW_TYPE,
      panelTitle(location),
      ViewColumn.Active,
      { enableScripts: false, retainContextWhenHidden: true },
    );
    const session: PanelSession = {
      key,
      panel,
      location,
      active: panel.active,
      visible: panel.visible,
    };
    this.sessions.set(key, session);
    this.emitVisibilityChange();
    panel.onDidDispose(() => this.clear(session));
    panel.onDidChangeViewState(({ webviewPanel }) => {
      if (this.sessions.get(key) !== session) return;
      session.active = webviewPanel.active;
      session.visible = webviewPanel.visible;
      this.emitVisibilityChange();
    });
    panel.webview.html = renderChangeRevisionHtml(change, revision, key);
  }

  /** Exact panels are immutable; freshness refreshes the tree, not their key. */
  async reloadActive(
    _targetKey?: string,
    _signal?: AbortSignal,
  ): Promise<void> {
    return Promise.resolve();
  }

  isVisible(): boolean {
    return this.reportedVisibility;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const sessions = [...this.sessions.values()];
    this.sessions.clear();
    for (const session of sessions) session.panel.dispose();
    this.visibilityEmitter.dispose();
  }

  private clear(session: PanelSession): void {
    if (this.sessions.get(session.key) !== session) return;
    this.sessions.delete(session.key);
    if (!this.disposed) this.emitVisibilityChange();
  }

  private emitVisibilityChange(): void {
    const visible = [...this.sessions.values()].some(
      (session) => session.visible,
    );
    if (visible === this.reportedVisibility) return;
    this.reportedVisibility = visible;
    this.visibilityEmitter.fire(visible);
  }
}

export function changeRevisionPanelKey(
  location: ChangeRevisionPanelLocation,
  resourceCacheKey: string,
): string {
  return createHash("sha256")
    .update(location.resolution.target.key)
    .update("\0")
    .update(location.changeId)
    .update("\0")
    .update(location.revision.revisionId)
    .update("\0")
    .update(location.revision.objectArtifactContentHash)
    .update("\0")
    .update(location.projectionStamp)
    .update("\0")
    .update(resourceCacheKey)
    .digest("hex");
}

export function renderChangeRevisionHtml(
  change: ChangeDetailDoc,
  document: ChangeRevisionDoc,
  locationKey: string,
): string {
  const facts = document.factPresentations
    .map(
      (fact) =>
        `<tr><td>${escapeHtml(fact.family)}</td><td>${escapeHtml(
          fact.originRevision.revisionId,
        )}</td><td>${escapeHtml(fact.revisionCurrency)}</td><td>${escapeHtml(
          fact.familyState,
        )}</td><td>${escapeHtml(fact.availability)}</td></tr>`,
    )
    .join("");
  const resource = document.exactRevisionDocument;
  const captured =
    resource.capturedDocument === undefined
      ? ""
      : `<h2>Exact captured document</h2><pre>${escapeHtml(
          JSON.stringify(resource.capturedDocument, null, 2),
        )}</pre>`;
  return documentHtml(
    locationKey,
    `<h1>Change ${escapeHtml(document.changeId)}</h1>
<dl>
  <dt>Revision</dt><dd>${escapeHtml(document.revision.revisionId)}</dd>
  <dt>Artifact</dt><dd>${escapeHtml(document.revision.objectArtifactContentHash)}</dd>
  <dt>Projection</dt><dd>${escapeHtml(document.projectionStamp)}</dd>
  <dt>Currency</dt><dd>${escapeHtml(document.revisionCurrency)}</dd>
  <dt>Relation</dt><dd>${escapeHtml(document.relationClassification)}</dd>
  <dt>Availability</dt><dd>${escapeHtml(document.availability)}</dd>
</dl>
<h2>Facts</h2>
<table><thead><tr><th>Family</th><th>Origin Revision</th><th>Currency</th><th>State</th><th>Availability</th></tr></thead><tbody>${facts}</tbody></table>
<h2>Membership support</h2><pre>${escapeHtml(JSON.stringify(document.membershipSupport, null, 2))}</pre>
<h2>Relation claims</h2><pre>${escapeHtml(JSON.stringify(change.relationClaims, null, 2))}</pre>
<h2>Associations</h2><pre>${escapeHtml(JSON.stringify(document.associations, null, 2))}</pre>
${captured}`,
  );
}

function documentHtml(locationKey: string, body: string): string {
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline';">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>body{font-family:var(--vscode-font-family);color:var(--vscode-foreground);padding:1rem}h1{font-size:1.25rem}h2{font-size:1rem;margin-top:1.5rem}dl{display:grid;grid-template-columns:max-content 1fr;gap:.35rem 1rem}dt{font-weight:600}dd{margin:0;overflow-wrap:anywhere}table{border-collapse:collapse;width:100%}th,td{text-align:left;border-bottom:1px solid var(--vscode-panel-border);padding:.35rem}pre{white-space:pre-wrap;overflow-wrap:anywhere}</style>
</head><body data-location-key="${escapeHtml(locationKey)}">${body}</body></html>`;
}

function panelTitle(location: ChangeRevisionPanelLocation): string {
  return `Pointbreak: ${shortId(location.changeId)} · ${shortId(
    location.revision.revisionId,
  )}`;
}

function shortId(value: string): string {
  return value.split(":").at(-1)?.slice(0, 8) || value;
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
