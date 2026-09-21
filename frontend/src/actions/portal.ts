import type { Action } from "svelte/action";

/** Move an overlay element to `document.body`.
 *
 *  A `position: fixed` overlay is only viewport-fixed while no ancestor creates a containing block.
 *  A transformed, filtered or `backdrop-filter` ancestor turns the overlay into a child of that
 *  ancestor's stacking context, so its `z-index` competes locally and the parent can intercept
 *  pointer input. Overlays that must sit above the whole shell use this action, keeping their scoped
 *  styles, which Svelte applies through generated classes rather than by DOM position. */
export const portal: Action<HTMLElement> = (node) => {
  const target = document.body;
  target.appendChild(node);
  return {
    destroy() {
      if (node.parentNode === target) {
        target.removeChild(node);
      }
    },
  };
};
