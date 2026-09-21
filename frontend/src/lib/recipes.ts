// Phase 7 / C4 - view model for the mod (recipe) surface.
//
// Ownership: this file and `components/RecipePanel.svelte` only. Everything here is pure and
// synchronous except the injected port, so the panel never decides an outcome locally: apply,
// configure and removal results come from the durable backend API and are rendered as received.
//
// Wire types are generated from the shared Rust contracts.
//
// The four state axes never promote each other. Support is not compatibility, documented is not
// tested, and an observation is not ownership. A locally imported file can never claim official
// support: see `importedSupportView`.
import type {
  RecipeCatalog,
  RecipeDescriptor,
  RecipeIssue,
  RecipeFileRole,
  RecipeState,
  RecipeOwnershipState,
  RecipeObservationState,
  RecipeCompatibilityState,
  RecipeSupportState,
} from "../generated/bindings";
import { translate, type Locale, type TranslationVars } from "./i18n/index";

export type {
  RecipeCatalog,
  RecipeDescriptor,
  RecipeIssue,
  RecipeFileRole,
  RecipeState,
  RecipeOwnershipState,
  RecipeObservationState,
  RecipeCompatibilityState,
  RecipeSupportState,
};

import type {
  RecipePreviewIntent as RecipeIntent, RecipeLocalPreviewRequest, RecipePreviewFile,
  RecipePreviewConfigEdit, RecipeLocalPreviewResult, RecipeApplyRequest,
  RecipeConfigureRequest, RecipeDurableRemovalRequest, OwnedRecipeListRequest,
  OwnedRecipeReceipt as OwnedRecipe, OwnedRecipeListResult, RecipeOperationStatus,
  RecipeOperationResult,
} from "../generated/bindings";
export type {
  RecipeIntent, RecipeLocalPreviewRequest, RecipePreviewFile, RecipePreviewConfigEdit,
  RecipeLocalPreviewResult, RecipeApplyRequest, RecipeConfigureRequest,
  RecipeDurableRemovalRequest, OwnedRecipeListRequest, OwnedRecipe,
  OwnedRecipeListResult, RecipeOperationStatus, RecipeOperationResult,
};

// Everything the panel is allowed to call. The parent supplies the real implementation from
// `lib/api.ts` once the commands are registered; tests supply fakes. Pickers are part of the port
// so the panel never imports a Tauri plugin directly.
export interface RecipeApi {
  listKnownRecipes(): Promise<RecipeCatalog>;
  listOwnedRecipes(request: OwnedRecipeListRequest): Promise<OwnedRecipeListResult>;
  previewLocalRecipe(request: RecipeLocalPreviewRequest): Promise<RecipeLocalPreviewResult>;
  applyRecipe(request: RecipeApplyRequest): Promise<RecipeOperationResult>;
  configureRecipe(request: RecipeConfigureRequest): Promise<RecipeOperationResult>;
  removeRecipe(request: RecipeDurableRemovalRequest): Promise<RecipeOperationResult>;
  // Absolute path of a mod file the user already has, or `null` when the user cancelled.
  pickRecipeFile(): Promise<string | null>;
  // Absolute path of the folder holding the files the user already downloaded.
  pickSourceDirectory(): Promise<string | null>;
}

// ----------------------------------------------------------------------------- localisation

// Recipe-local wording fallback. It is owned by this lane so the mod surface does not depend on the
// Drivers helper, which that lane is retiring. Resolve the owned key once the i18n batch has landed
// and otherwise fall back to an existing translated key, so no untranslated literal reaches the
// screen. The requested keys are listed in `tests/recipe-locale-keys.json`; when the parent lands
// them in the eight catalogues, every call resolves on the first lookup and this helper can be
// replaced by `$t` without touching the markup.
export function recipeText(
  loc: Locale,
  key: string,
  fallbackKey: string,
  vars?: TranslationVars,
): string {
  const value = translate(loc, key, vars);
  return value === key ? translate(loc, fallbackKey, vars) : value;
}

// ------------------------------------------------------------------------------- catalogue

export interface CatalogAvailability {
  // True only when the backend actually lists a usable mod. Today it lists none.
  hasRecipes: boolean;
  // True when every acquisition needs an explicit user action. DLSSync downloads nothing here.
  requiresUserAction: boolean;
}

export function catalogAvailability(catalog: RecipeCatalog | null): CatalogAvailability {
  return {
    hasRecipes: (catalog?.recipes?.length ?? 0) > 0,
    requiresUserAction: catalog?.acquisition_requires_user_action ?? true,
  };
}

// -------------------------------------------------------------------------- the four axes

export type RecipeTone = "neutral" | "accent" | "success" | "warning" | "danger";

export interface AxisView {
  // Suffix of the locale key for this axis value, never a promoted verdict.
  key: string;
  tone: RecipeTone;
  // Paths the backend attached to this state, shown as read-only evidence.
  paths: string[];
}

export function ownershipView(state: RecipeOwnershipState): AxisView {
  switch (state.status) {
    case "installed_by_dlssync":
      return { key: "installed_by_dlssync", tone: "accent", paths: [] };
    case "owned_modified":
      return { key: "owned_modified", tone: "warning", paths: state.paths };
    case "owned_missing":
      return { key: "owned_missing", tone: "warning", paths: state.paths };
    case "partially_removed":
      return { key: "partially_removed", tone: "warning", paths: state.retained_paths };
    case "removed":
      return { key: "removed", tone: "neutral", paths: [] };
    default:
      return { key: "none", tone: "neutral", paths: [] };
  }
}

export function observationView(state: RecipeObservationState): AxisView {
  switch (state.status) {
    case "files_detected":
      return { key: "files_detected", tone: "accent", paths: state.paths };
    case "runtime_detected":
      return { key: "runtime_detected", tone: "accent", paths: state.module_paths };
    case "unknown_or_inaccessible":
      return { key: "unknown_or_inaccessible", tone: "warning", paths: [] };
    default:
      return { key: "not_observed", tone: "neutral", paths: [] };
  }
}

// `documented_only` is not a compatibility result and never renders as success.
export function compatibilityView(state: RecipeCompatibilityState): AxisView {
  switch (state.status) {
    case "documented_only":
      return { key: "documented_only", tone: "neutral", paths: [] };
    case "tested_pass":
      return { key: "tested_pass", tone: "success", paths: [] };
    case "tested_fail":
      return { key: "tested_fail", tone: "danger", paths: [] };
    case "stale":
      return { key: "stale", tone: "warning", paths: [] };
    default:
      return { key: "unknown", tone: "warning", paths: [] };
  }
}

// Official support says who maintains the mod entry. It says nothing about compatibility.
export function supportView(state: RecipeSupportState): AxisView {
  switch (state.status) {
    case "experimental":
      return { key: "experimental", tone: "warning", paths: [] };
    case "official":
      return { key: "official", tone: "accent", paths: [] };
    default:
      return { key: "unknown", tone: "neutral", paths: [] };
  }
}

// Support level for a file the user imported from their own disk. The signed catalogue is
// deliberately empty, so a local import carries no reviewed entry: whatever the file claims about
// itself, the panel shows an unverified local source and never an official badge. Only a descriptor
// that came back inside an active receipt or the signed catalogue may use `supportView`.
export function importedSupportView(state: RecipeSupportState | null | undefined): AxisView {
  if (state?.status === "experimental") return { key: "experimental", tone: "warning", paths: [] };
  return { key: "local_import", tone: "warning", paths: [] };
}

export interface RecipeStateView {
  ownership: AxisView;
  observation: AxisView;
  compatibility: AxisView;
  support: AxisView;
}

export function stateView(state: RecipeState): RecipeStateView {
  return {
    ownership: ownershipView(state.ownership),
    observation: observationView(state.observation),
    compatibility: compatibilityView(state.compatibility),
    support: supportView(state.support),
  };
}

// --------------------------------------------------------------------------------- issues

// Conflict codes emitted by `RecipeConflictError::code` in `dlssync-application`.
export const RECIPE_CONFLICT_CODES: readonly string[] = [
  "recipe_path_conflict",
  "official_component_path_conflict",
  "unsupported_proxy_filename",
  "unsupported_proxy_chain",
];

export interface ClassifiedIssues {
  // Path or loader collisions; these are shown on their own, never folded into a generic error.
  conflicts: RecipeIssue[];
  // Everything else the backend reported, kept verbatim.
  problems: RecipeIssue[];
}

export function classifyIssues(
  issues: readonly RecipeIssue[] | null | undefined,
): ClassifiedIssues {
  const conflicts: RecipeIssue[] = [];
  const problems: RecipeIssue[] = [];
  for (const issue of issues ?? []) {
    if (RECIPE_CONFLICT_CODES.includes(issue.code)) conflicts.push(issue);
    else problems.push(issue);
  }
  return { conflicts, problems };
}

export interface ConflictView {
  code: string;
  // Relative path inside the game folder, when the backend named one.
  path: string | null;
  // The other side of the collision: an official component id or another mod id.
  otherParty: string | null;
  // Backend message, used verbatim when no localised sentence exists for the code.
  message: string;
}

export function conflictView(issue: RecipeIssue): ConflictView {
  const context: Record<string, string> = issue.context ?? {};
  return {
    code: issue.code,
    path: context.path ?? null,
    otherParty:
      context.component_id ??
      context.other_recipe_id ??
      context.other_party ??
      context.recipe_id ??
      null,
    message: issue.message,
  };
}

// Localised sentence for an issue when the catalogue has one, otherwise the backend message. The
// backend text is never dropped: an unknown code still reaches the user.
export function issueText(loc: Locale, issue: RecipeIssue): string {
  const key = `component.recipes.issue.${issue.code}`;
  const view = conflictView(issue);
  const resolved = translate(loc, key, {
    path: view.path ?? "",
    party: view.otherParty ?? "",
  });
  return resolved === key ? issue.message : resolved;
}

// -------------------------------------------------------------------------------- preview

export interface RecipeSelection {
  recipePath: string | null;
  sourceDirectory: string | null;
  intent: RecipeIntent;
}

export function emptySelection(): RecipeSelection {
  return { recipePath: null, sourceDirectory: null, intent: "apply" };
}

// A check needs the game (held by the panel) and the mod file. The source folder is optional at
// this level precisely because it must stay an explicit user selection: when the user did not pick
// one, the backend answers with an issue instead of the UI guessing a folder.
export function selectionComplete(selection: RecipeSelection): boolean {
  return Boolean(selection.recipePath);
}

export function sameSelection(a: RecipeSelection, b: RecipeSelection): boolean {
  return (
    a.recipePath === b.recipePath &&
    a.sourceDirectory === b.sourceDirectory &&
    a.intent === b.intent
  );
}

export function previewRequest(
  gameId: string,
  selection: RecipeSelection,
): RecipeLocalPreviewRequest | null {
  if (!gameId || !selection.recipePath) return null;
  return {
    gameId,
    recipePath: selection.recipePath,
    sourceDirectory: selection.sourceDirectory,
    intent: selection.intent,
  };
}

// A preview with an unparsable or past `expiresAt` is treated as expired: a write is refused and
// the user is asked to check again.
export function previewExpired(expiresAt: string, now: number): boolean {
  const deadline = Date.parse(expiresAt);
  if (Number.isNaN(deadline)) return true;
  return deadline <= now;
}

export type WriteBlockReason =
  | "no_selection"
  | "not_checked"
  | "expired"
  | "conflicts"
  | "not_allowed"
  | "busy";

export interface WriteGate {
  ready: boolean;
  reason: WriteBlockReason | null;
}

// The single gate for the install / save-settings button. Only a fresh, allowed, conflict-free
// preview for the selected intent opens it.
export function writeGate(input: {
  selection: RecipeSelection;
  preview: RecipeLocalPreviewResult | null;
  previewIntent: RecipeIntent | null;
  now: number;
  busy: boolean;
}): WriteGate {
  if (input.busy) return { ready: false, reason: "busy" };
  if (!selectionComplete(input.selection)) return { ready: false, reason: "no_selection" };
  const preview = input.preview;
  if (!preview || input.previewIntent !== input.selection.intent) {
    return { ready: false, reason: "not_checked" };
  }
  if (previewExpired(preview.expiresAt, input.now)) return { ready: false, reason: "expired" };
  if (classifyIssues(preview.issues).conflicts.length > 0) {
    return { ready: false, reason: "conflicts" };
  }
  if (!preview.allowed) return { ready: false, reason: "not_allowed" };
  return { ready: true, reason: null };
}

export interface PreviewSummary {
  fileCount: number;
  configEditCount: number;
  conflicts: RecipeIssue[];
  problems: RecipeIssue[];
  allowed: boolean;
}

export function previewSummary(preview: RecipeLocalPreviewResult): PreviewSummary {
  const { conflicts, problems } = classifyIssues(preview.issues);
  return {
    fileCount: preview.files?.length ?? 0,
    configEditCount: preview.configEdits?.length ?? 0,
    conflicts,
    problems,
    allowed: preview.allowed,
  };
}

// ------------------------------------------------------------------------------- outcomes

export interface OperationSummary {
  failed: boolean;
  tone: RecipeTone;
  changedCount: number;
  // Files the backend deliberately left on disk. Always surfaced, success or failure.
  retainedCount: number;
  issues: RecipeIssue[];
}

export function operationSummary(result: RecipeOperationResult): OperationSummary {
  const failed = result.status !== "completed";
  return {
    failed,
    tone: failed ? "danger" : "success",
    changedCount: result.changedPaths?.length ?? 0,
    retainedCount: result.retainedPaths?.length ?? 0,
    issues: result.issues ?? [],
  };
}

// Rows for the installed list. An empty or missing list means nothing is installed by DLSSync in
// this installation; it is not an error and not a compatibility statement.
export function ownedRows(result: OwnedRecipeListResult | null): OwnedRecipe[] {
  return result?.recipes ?? [];
}

// Stable display label. Identity comes from the backend id and the immutable upstream version;
// nothing is invented for presentation.
export function recipeLabel(recipe: Pick<OwnedRecipe, "recipeId" | "upstreamVersion">): string {
  return recipe.upstreamVersion ? `${recipe.recipeId} - ${recipe.upstreamVersion}` : recipe.recipeId;
}

export function descriptorLabel(descriptor: RecipeDescriptor): string {
  return descriptor.upstream_version
    ? `${descriptor.id} - ${descriptor.upstream_version}`
    : descriptor.id;
}

// Last path segment, for a compact file column. The full path stays available as evidence.
export function fileName(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part.length > 0);
  return parts.length > 0 ? parts[parts.length - 1] : path;
}
