/// <reference lib="dom" />

import "./review.css";
import {
  type HostToWebview,
  isHostToWebview,
  type WebviewToHost,
} from "../webviewProtocol";
import { ReviewWebviewController } from "./reviewController";

interface VsCodeApi {
  postMessage(message: WebviewToHost): void;
  getState(): unknown;
  setState(state: unknown): void;
}

declare function acquireVsCodeApi(): VsCodeApi;

const vscode = acquireVsCodeApi();

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", start, { once: true });
} else {
  start();
}

function start(): void {
  const root = document.querySelector<HTMLElement>("#review-root");
  if (!root) {
    return;
  }
  const controller = new ReviewWebviewController(root, vscode);
  window.addEventListener("message", ({ data }: MessageEvent<unknown>) => {
    if (isHostToWebview(data)) {
      renderMessage(controller, data);
    }
  });
  vscode.postMessage({ type: "ready" });
}

function renderMessage(
  controller: ReviewWebviewController,
  message: HostToWebview,
): void {
  switch (message.type) {
    case "render":
      controller.render(message.data, message.focus);
      return;
    case "error":
      controller.error(message.message);
      return;
    case "freshness":
      document.body.dataset.freshness = message.changed ? "changed" : "current";
      return;
    case "focus":
      controller.focus(message.focus);
      return;
  }
}
