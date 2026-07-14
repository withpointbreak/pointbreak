import { createHash, randomBytes } from "node:crypto";
import {
  type Disposable,
  type Event,
  EventEmitter,
  Uri,
  ViewColumn,
  type Webview,
  type WebviewPanel,
  window,
} from "vscode";
import type { DiffDataSource } from "./diffDataSource";
import type { ResolvedTargetResolution } from "./targetResolver";
import {
  type HostToWebview,
  isHostToWebview,
  isWebviewToHost,
  type ReviewPanelFocus,
} from "./webviewProtocol";

const VIEW_TYPE = "pointbreak.annotatedDiff";
const PANEL_TITLE = "Pointbreak Review";
const LOAD_ERROR = "Pointbreak could not load this annotated diff.";

export interface ReviewPanelLocation {
  readonly resolution: ResolvedTargetResolution;
  readonly revisionId: string;
  readonly focus?: ReviewPanelFocus;
}

export interface ReviewPanelOpenOptions {
  readonly preserveFocus?: boolean;
}

/** Owns the one annotated-diff presentation surface for this VS Code window. */
export class ReviewPanelManager implements Disposable {
  private readonly visibilityEmitter = new EventEmitter<boolean>();
  readonly onDidChangeVisibility: Event<boolean> = this.visibilityEmitter.event;

  private panel: WebviewPanel | undefined;
  private currentLocation: ReviewPanelLocation | undefined;
  private generation = 0;
  private ready = false;
  private pendingState: HostToWebview | undefined;
  private disposed = false;

  constructor(
    private readonly extensionUri: Uri,
    private readonly dataSource: DiffDataSource,
  ) {}

  async open(
    location: ReviewPanelLocation,
    options: ReviewPanelOpenOptions = {},
  ): Promise<void> {
    if (this.disposed) {
      throw new Error("Pointbreak Review is no longer available.");
    }

    const existingPanel = this.panel;
    const panel = existingPanel ?? this.createPanel(!!options.preserveFocus);
    if (existingPanel) {
      panel.reveal(ViewColumn.Active, !!options.preserveFocus);
    }

    const previous = this.currentLocation;
    const sameData = previous ? sameDataLocation(previous, location) : false;
    const sameFocus = previous
      ? equalFocus(previous.focus, location.focus)
      : false;
    this.currentLocation = location;

    if (sameData && sameFocus) {
      return;
    }
    if (sameData) {
      this.sendFocus(location.focus);
      return;
    }
    await this.load(location, panel);
  }

  async reloadActive(): Promise<void> {
    if (!this.panel || !this.currentLocation) {
      return;
    }
    await this.load(this.currentLocation, this.panel);
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.generation += 1;
    const panel = this.panel;
    this.clearPanel(panel);
    panel?.dispose();
    this.visibilityEmitter.dispose();
  }

  private createPanel(preserveFocus: boolean): WebviewPanel {
    const outputRoot = Uri.joinPath(this.extensionUri, "out");
    const panel = window.createWebviewPanel(
      VIEW_TYPE,
      PANEL_TITLE,
      { viewColumn: ViewColumn.Active, preserveFocus },
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [outputRoot],
      },
    );
    this.panel = panel;
    panel.onDidDispose(() => this.clearPanel(panel));
    panel.onDidChangeViewState(({ webviewPanel }) => {
      if (this.panel === webviewPanel) {
        this.visibilityEmitter.fire(webviewPanel.visible);
      }
    });
    panel.webview.onDidReceiveMessage((message: unknown) => {
      void this.receive(panel, message);
    });
    return panel;
  }

  private async load(
    location: ReviewPanelLocation,
    panel: WebviewPanel,
  ): Promise<void> {
    const generation = ++this.generation;
    this.ready = false;
    this.pendingState = undefined;
    panel.title = `${PANEL_TITLE}: loading`;
    panel.webview.html = webviewHtml(
      this.extensionUri,
      panel.webview,
      location,
    );

    try {
      const data = await this.dataSource.load({
        resolution: location.resolution,
        revisionId: location.revisionId,
      });
      if (!this.isCurrent(panel, generation, location)) {
        return;
      }
      const message: HostToWebview = {
        type: "render",
        data,
        focus: this.currentLocation?.focus,
      };
      if (
        data.revisionId !== location.revisionId ||
        !isHostToWebview(message)
      ) {
        this.queueState({ type: "error", message: LOAD_ERROR });
        return;
      }
      panel.title = `${PANEL_TITLE}: ${shortRevisionId(location.revisionId)}`;
      this.queueState(message);
    } catch {
      if (!this.isCurrent(panel, generation, location)) {
        return;
      }
      panel.title = `${PANEL_TITLE}: unavailable`;
      this.queueState({ type: "error", message: LOAD_ERROR });
    }
  }

  private async receive(panel: WebviewPanel, message: unknown): Promise<void> {
    if (panel !== this.panel || !isWebviewToHost(message)) {
      return;
    }
    if (message.type === "ready") {
      this.ready = true;
      this.flushState();
      return;
    }
    if (message.type === "reload") {
      await this.reloadActive();
    }
    // openSource is reserved for the source-crossing command.
  }

  private queueState(message: HostToWebview): void {
    this.pendingState = message;
    this.flushState();
  }

  private flushState(): void {
    if (!this.ready || !this.panel || !this.pendingState) {
      return;
    }
    const message = this.pendingState;
    this.pendingState = undefined;
    void this.panel.webview.postMessage(message);
  }

  private sendFocus(focus: ReviewPanelFocus | undefined): void {
    if (this.pendingState?.type === "render") {
      this.pendingState = { ...this.pendingState, focus };
      this.flushState();
      return;
    }
    if (this.ready && this.panel) {
      void this.panel.webview.postMessage({ type: "focus", focus });
    }
  }

  private isCurrent(
    panel: WebviewPanel,
    generation: number,
    location: ReviewPanelLocation,
  ): boolean {
    return (
      this.panel === panel &&
      this.generation === generation &&
      !!this.currentLocation &&
      sameDataLocation(this.currentLocation, location)
    );
  }

  private clearPanel(panel: WebviewPanel | undefined): void {
    if (!panel || this.panel !== panel) {
      return;
    }
    this.generation += 1;
    this.panel = undefined;
    this.currentLocation = undefined;
    this.ready = false;
    this.pendingState = undefined;
    if (!this.disposed) {
      this.visibilityEmitter.fire(false);
    }
  }
}

function webviewHtml(
  extensionUri: Uri,
  webview: Webview,
  location: ReviewPanelLocation,
): string {
  const nonce = randomBytes(18).toString("base64");
  const locationKey = createHash("sha256")
    .update(location.resolution.target.key)
    .update("\0")
    .update(location.revisionId)
    .digest("hex");
  const script = webview.asWebviewUri(
    Uri.joinPath(extensionUri, "out", "review.js"),
  );
  const style = webview.asWebviewUri(
    Uri.joinPath(extensionUri, "out", "review.css"),
  );
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource} data:; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="${style}">
  <title>${PANEL_TITLE}</title>
</head>
<body>
  <main id="review-root" data-location-key="${locationKey}"><p class="empty review-loading" role="status">Loading annotated diff…</p></main>
  <script nonce="${nonce}" src="${script}"></script>
</body>
</html>`;
}

function sameDataLocation(
  left: ReviewPanelLocation,
  right: ReviewPanelLocation,
): boolean {
  return (
    left.resolution.target.key === right.resolution.target.key &&
    left.revisionId === right.revisionId
  );
}

function equalFocus(
  left: ReviewPanelFocus | undefined,
  right: ReviewPanelFocus | undefined,
): boolean {
  return left?.kind === right?.kind && left?.id === right?.id;
}

function shortRevisionId(revisionId: string): string {
  return revisionId.split(":").at(-1)?.slice(0, 12) ?? revisionId;
}
