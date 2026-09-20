import { get, writable } from "svelte/store";
import type { ApplyTarget } from "../../lib/applyController";
import type { UpdatePlan } from "../../lib/api";

export interface ReviewedUpdatePlan {
  targets: ApplyTarget[];
  catalogGeneratedAt: string;
  plan?: UpdatePlan;
}

interface PendingPlan {
  targets: ApplyTarget[];
  resolve: (result: ReviewedUpdatePlan | null) => void;
}

export const pendingUpdatePlan = writable<PendingPlan | null>(null);

export function completeUpdatePlan(result: ReviewedUpdatePlan | null): void {
  const pending = get(pendingUpdatePlan);
  if (!pending) return;
  pendingUpdatePlan.set(null);
  pending.resolve(result);
}
