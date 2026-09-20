import type { Action } from "svelte/action";

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusable(node: HTMLElement): HTMLElement[] {
  return Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => el.offsetParent !== null || el === document.activeElement,
  );
}

export const focusTrap: Action<HTMLElement, { initialFocusRing?: boolean } | undefined> = (node, options) => {
  const opener = document.activeElement as HTMLElement | null;

  function onKeydown(event: KeyboardEvent) {
    if (event.key !== "Tab") return;
    const items = focusable(node);
    if (items.length === 0) {
      event.preventDefault();
      node.focus();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !node.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  // Programmatic focus does not always match `:focus-visible`, so the user can lose track of where
  // the keyboard is. The marker lets one global rule draw the ring for this exact case, and it is
  // removed as soon as focus moves on.
  const initial = focusable(node)[0] ?? node;
  let marked: HTMLElement | null = null;

  function clearMarker(): void {
    if (marked !== null) {
      delete marked.dataset.initialFocus;
      marked = null;
    }
  }

  function onFocusOut(): void {
    clearMarker();
  }

  function markActive(): void {
    if (options?.initialFocusRing === false) return;
    const active = document.activeElement;
    if (active instanceof HTMLElement && node.contains(active) && marked !== active) {
      clearMarker();
      active.dataset.initialFocus = "true";
      marked = active;
    }
  }

  if (!node.contains(document.activeElement)) {
    if (initial === node && node.tabIndex < 0) node.tabIndex = -1;
    initial.focus();
  }
  // Some surfaces focus their own control, before or right after this action runs, so the marker is
  // applied to whatever ends up focused inside the surface, then re-checked once on the next frame.
  markActive();
  const settleFrame =
    typeof requestAnimationFrame === "function" ? requestAnimationFrame(() => {
      // A parent portal can move after its child's action and clear the child's initial focus.
      if (node.isConnected && document.activeElement === document.body) {
        (focusable(node)[0] ?? node).focus();
      }
      markActive();
    }) : null;
  node.addEventListener("keydown", onKeydown);
  node.addEventListener("focusout", onFocusOut);
  node.addEventListener("pointerdown", clearMarker);

  return {
    destroy() {
      if (settleFrame !== null && typeof cancelAnimationFrame === "function") {
        cancelAnimationFrame(settleFrame);
      }
      node.removeEventListener("keydown", onKeydown);
      node.removeEventListener("focusout", onFocusOut);
      node.removeEventListener("pointerdown", clearMarker);
      clearMarker();
      // A search result can open another dialog in the same render. Do not steal its focus.
      if (node.contains(document.activeElement) || document.activeElement === document.body) {
        opener?.focus?.();
      }
    },
  };
};

export default focusTrap;
