// Lightweight anchored control panels. These are deliberately separate from the
// full-screen overlay manager: opening a filter or view panel must not suspend
// timeline navigation, trap focus, or make the rest of the reader inert.

import { $ } from "./dom";

export interface DisclosureController {
  isOpen(): boolean;
  open(): void;
  close(returnFocus?: boolean): void;
  toggle(): void;
  sync(): void;
  dispose(): void;
}

interface DisclosureOptions {
  container: string;
  trigger: string;
  panel: string;
}

let active: DisclosureController | null = null;

/**
 * Create one anchored disclosure. The caller owns the panel's controls; this
 * controller owns only visibility, aria-expanded, outside-click dismissal, and
 * the local Escape rung.
 */
export function createDisclosure({
  container,
  trigger,
  panel,
}: DisclosureOptions): DisclosureController {
  let open = false;
  const triggerElement = $<HTMLElement>(trigger);
  const containerElement = $<HTMLElement>(container);
  let controller: DisclosureController;

  const onTriggerClick = (event: Event): void => {
    event.stopPropagation();
    controller.toggle();
  };
  const onContainerKeydown = (event: KeyboardEvent): void => {
    if (event.key !== "Escape" || !open) return;
    event.preventDefault();
    event.stopPropagation();
    controller.close(true);
  };
  // Capture phase sees a panel row before a synchronous repaint can detach it.
  const onDocumentClick = (event: MouseEvent): void => {
    if (!open) return;
    const root = $(container);
    if (event.target instanceof Node && root?.contains(event.target)) return;
    controller.close();
  };

  controller = {
    isOpen: () => open,
    open: () => {
      if (active && active !== controller) active.close();
      open = true;
      active = controller;
      controller.sync();
    },
    close: (returnFocus = false) => {
      open = false;
      if (active === controller) active = null;
      controller.sync();
      if (returnFocus) $<HTMLElement>(trigger)?.focus();
    },
    toggle: () => {
      if (open) controller.close();
      else controller.open();
    },
    sync: () => {
      $(panel)?.classList.toggle("hidden", !open);
      $(trigger)?.setAttribute("aria-expanded", String(open));
    },
    dispose: () => {
      triggerElement?.removeEventListener("click", onTriggerClick);
      containerElement?.removeEventListener("keydown", onContainerKeydown);
      document.removeEventListener("click", onDocumentClick, true);
      if (active === controller) active = null;
      open = false;
      controller.sync();
    },
  };

  triggerElement?.addEventListener("click", onTriggerClick);
  containerElement?.addEventListener("keydown", onContainerKeydown);
  document.addEventListener("click", onDocumentClick, true);

  controller.sync();
  return controller;
}
