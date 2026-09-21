<script lang="ts">
  // Phase 7 / C4 - mod surface for one game installation, on top of the durable recipe IPC.
  //
  // Rules this panel keeps:
  //  * DLSSync never fetches a mod. The user points at a file and a folder they already have, and
  //    the backend reads and hashes that local input.
  //  * Every write awaits the durable backend API. Nothing is marked installed, configured or
  //    removed from local state; the returned operation result is what the panel renders.
  //  * Conflicts, ownership and unknown compatibility stay visible and separate. A support level
  //    is never promoted into a compatibility claim, and a file imported from the user's disk is
  //    always shown as an unverified local source - the signed catalogue is deliberately empty.
  //  * Files the backend retained are always listed, including after a completed removal.
  //
  import { onMount } from "svelte";
  import { locale } from "../lib/i18n/index";
  import Select from "./Select.svelte";
  import {
    catalogAvailability,
    classifyIssues,
    compatibilityView,
    conflictView,
    emptySelection,
    fileName,
    importedSupportView,
    issueText,
    operationSummary,
    ownedRows,
    previewRequest,
    previewSummary,
    recipeLabel,
    recipeText,
    selectionComplete,
    stateView,
    writeGate,
    type OwnedRecipe,
    type RecipeApi,
    type RecipeCatalog,
    type RecipeIntent,
    type RecipeLocalPreviewResult,
    type RecipeOperationResult,
    type RecipeSelection,
    type OwnedRecipeListResult,
  } from "../lib/recipes";

  let {
    gameId,
    api,
  }: {
    gameId: string;
    // Injected port. The parent binds it to `lib/api.ts` once the B3 commands are registered.
    api: RecipeApi;
  } = $props();

  let catalog = $state<RecipeCatalog | null>(null);
  let owned = $state<OwnedRecipeListResult | null>(null);
  let selection = $state<RecipeSelection>(emptySelection());
  let preview = $state<RecipeLocalPreviewResult | null>(null);
  // Intent the current preview was issued for. A preview is never redeemed by the other action.
  let previewIntent = $state<RecipeIntent | null>(null);
  let now = $state(Date.now());
  let checking = $state(false);
  let writing = $state(false);
  let removingId = $state<string | null>(null);
  let listError = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let result = $state<RecipeOperationResult | null>(null);

  let busy = $derived(checking || writing || removingId !== null);
  let availability = $derived(catalogAvailability(catalog));
  let rows = $derived(ownedRows(owned));
  // A preview belongs to the intent it was issued for. Switching the action retires it on screen,
  // and the gate refuses it as well.
  let activePreview = $derived(preview !== null && previewIntent === selection.intent ? preview : null);
  let summary = $derived(activePreview ? previewSummary(activePreview) : null);
  let gate = $derived(writeGate({ selection, preview, previewIntent, now, busy }));
  let outcome = $derived(result ? operationSummary(result) : null);
  let previewConflicts = $derived(activePreview ? classifyIssues(activePreview.issues).conflicts : []);
  let previewProblems = $derived(activePreview ? classifyIssues(activePreview.issues).problems : []);
  // The mod file came from the user's own disk, so its self-declared support level is not evidence.
  let importedCompatibility = $derived(
    activePreview?.recipe ? compatibilityView(activePreview.recipe.state.compatibility) : null,
  );
  let importedSupport = $derived(
    activePreview?.recipe ? importedSupportView(activePreview.recipe.state.support) : null,
  );

  function tx(key: string, fallbackKey: string): string {
    return recipeText($locale, key, fallbackKey);
  }

  let intentOptions = $derived<{ value: RecipeIntent; label: string }[]>([
    { value: "apply", label: tx("component.recipes.intent.apply", "common.install") },
    { value: "configure", label: tx("component.recipes.intent.configure", "common.update") },
  ]);

  // Any change to the selection retires the preview: a stale check can never authorise a write.
  function invalidatePreview(): void {
    preview = null;
    previewIntent = null;
  }

  async function loadCatalog(): Promise<void> {
    try {
      catalog = await api.listKnownRecipes();
    } catch {
      catalog = null;
    }
  }

  async function loadOwned(): Promise<void> {
    try {
      owned = await api.listOwnedRecipes({ gameId });
      listError = null;
    } catch (err) {
      owned = null;
      listError = String(err);
    }
  }

  onMount(() => {
    void loadCatalog();
    void loadOwned();
  });

  async function chooseRecipeFile(): Promise<void> {
    actionError = null;
    try {
      const picked = await api.pickRecipeFile();
      if (picked === null) return;
      selection = { ...selection, recipePath: picked };
      invalidatePreview();
    } catch (err) {
      actionError = String(err);
    }
  }

  async function chooseSourceDirectory(): Promise<void> {
    actionError = null;
    try {
      const picked = await api.pickSourceDirectory();
      if (picked === null) return;
      selection = { ...selection, sourceDirectory: picked };
      invalidatePreview();
    } catch (err) {
      actionError = String(err);
    }
  }

  function clearSelection(): void {
    selection = emptySelection();
    invalidatePreview();
    result = null;
    actionError = null;
  }

  async function check(): Promise<void> {
    const request = previewRequest(gameId, selection);
    if (request === null || busy) return;
    checking = true;
    actionError = null;
    result = null;
    try {
      const answer = await api.previewLocalRecipe(request);
      preview = answer;
      previewIntent = request.intent;
      now = Date.now();
    } catch (err) {
      invalidatePreview();
      actionError = String(err);
    } finally {
      checking = false;
    }
  }

  // The write is authorised by the backend preview only. On return, the preview is spent and the
  // installed list is re-read from the backend rather than patched locally.
  async function commit(): Promise<void> {
    now = Date.now();
    const current = preview;
    if (current === null || !writeGate({ selection, preview: current, previewIntent, now, busy }).ready) {
      return;
    }
    writing = true;
    actionError = null;
    try {
      const answer =
        selection.intent === "configure"
          ? await api.configureRecipe({ previewId: current.previewId })
          : await api.applyRecipe({ previewId: current.previewId });
      result = answer;
    } catch (err) {
      actionError = String(err);
    } finally {
      writing = false;
      invalidatePreview();
      await loadOwned();
    }
  }

  async function remove(recipe: OwnedRecipe): Promise<void> {
    if (busy) return;
    removingId = recipe.recipeId;
    actionError = null;
    result = null;
    try {
      result = await api.removeRecipe({ gameId, recipeId: recipe.recipeId });
    } catch (err) {
      actionError = String(err);
    } finally {
      removingId = null;
      await loadOwned();
    }
  }
</script>

<div class="rp" data-testid="recipe-panel">
  <div class="rp-head">
    <h4>{tx("component.recipes.heading", "component.gameDrawer.featureFallback")}</h4>
  </div>
  <p class="rp-note">{tx("component.recipes.sub", "view.settings.filesDisclosure.show")}</p>

  {#if !availability.hasRecipes}
    <p class="rp-note" data-testid="recipe-catalog-empty">
      {tx("component.recipes.empty.catalog", "view.library.empty.noGames.detail")}
    </p>
  {/if}

  <section class="rp-group" aria-label={tx("component.recipes.installed.heading", "component.gameDrawer.stat.files")}>
    <span class="rp-group-title">{tx("component.recipes.installed.heading", "component.gameDrawer.stat.files")}</span>
    {#if listError}
      <p class="rp-error" role="alert" data-testid="recipe-list-error">
        {tx("component.recipes.error.listFailed", "status.scan_failed")}
        <span class="rp-detail">{listError}</span>
      </p>
    {:else if rows.length === 0}
      <p class="rp-note" data-testid="recipe-installed-empty">
        {tx("component.recipes.installed.empty", "view.library.empty.noMatch.detail")}
      </p>
    {:else}
      <ul class="rp-list">
        {#each rows as row (row.recipeId)}
          {@const view = stateView(row.state)}
          <li class="rp-row" data-testid="recipe-row" data-recipe-id={row.recipeId}>
            <div class="rp-row-head">
              <span class="rp-row-name">{recipeLabel(row)}</span>
              <button
                class="rp-remove"
                data-testid="recipe-remove"
                onclick={() => remove(row)}
                disabled={busy}
              >
                {removingId === row.recipeId
                  ? tx("component.recipes.removing", "component.dlss.working")
                  : tx("component.recipes.remove", "view.backups.delete")}
              </button>
            </div>
            <div class="rp-chips">
              <span class="rp-chip" data-axis="ownership" data-state={view.ownership.key} data-tone={view.ownership.tone}>
                {tx("component.recipes.state.ownership." + view.ownership.key, "status.unknown")}
              </span>
              <span class="rp-chip" data-axis="observation" data-state={view.observation.key} data-tone={view.observation.tone}>
                {tx("component.recipes.state.observation." + view.observation.key, "status.unknown")}
              </span>
              <span class="rp-chip" data-axis="compatibility" data-state={view.compatibility.key} data-tone={view.compatibility.tone}>
                {tx("component.recipes.state.compatibility." + view.compatibility.key, "status.unknown")}
              </span>
              <span class="rp-chip" data-axis="support" data-state={view.support.key} data-tone={view.support.tone}>
                {tx("component.recipes.state.support." + view.support.key, "status.unknown")}
              </span>
            </div>
            {#if view.ownership.paths.length > 0}
              <ul class="rp-paths" data-testid="recipe-owned-paths">
                {#each view.ownership.paths as path (path)}
                  <li class="mono">{path}</li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
      <p class="rp-note">{tx("component.recipes.state.note", "component.gameDrawer.status.externallyManaged")}</p>
    {/if}
  </section>

  <section class="rp-group" aria-label={tx("component.recipes.add.heading", "view.library.addFolder")}>
    <span class="rp-group-title">{tx("component.recipes.add.heading", "view.library.addFolder")}</span>

    <div class="rp-field">
      <span class="rp-field-label">{tx("component.recipes.add.recipeFileLabel", "component.gameDrawer.stat.files")}</span>
      <button class="rp-choose" data-testid="recipe-pick-file" onclick={chooseRecipeFile} disabled={busy}>
        {tx("component.recipes.add.recipeFile", "view.settings.filesDisclosure.show")}
      </button>
    </div>
    <p class="rp-picked mono" data-testid="recipe-file-path">
      {selection.recipePath ?? tx("component.recipes.add.nothingChosen", "status.unknown")}
    </p>

    <div class="rp-field">
      <span class="rp-field-label">{tx("component.recipes.add.sourceFolderLabel", "view.library.addFolder")}</span>
      <button class="rp-choose" data-testid="recipe-pick-folder" onclick={chooseSourceDirectory} disabled={busy}>
        {tx("component.recipes.add.sourceFolder", "view.library.empty.noGames.addCustomFolder")}
      </button>
    </div>
    <p class="rp-picked mono" data-testid="recipe-folder-path">
      {selection.sourceDirectory ?? tx("component.recipes.add.nothingChosen", "status.unknown")}
    </p>
    <p class="rp-note">{tx("component.recipes.add.sourceFolderHint", "view.library.empty.noGames.customPrefix")}</p>

    <div class="rp-field">
      <span class="rp-field-label">{tx("component.recipes.intent.label", "component.gameDrawer.feature.toggleAria")}</span>
      <div class="rp-control">
        <Select
          bind:value={selection.intent}
          options={intentOptions}
          ariaLabel={tx("component.recipes.intent.label", "component.gameDrawer.feature.toggleAria")}
          disabled={busy}
        />
      </div>
    </div>

    <div class="rp-actions">
      <button
        class="rp-check"
        data-testid="recipe-check"
        onclick={check}
        disabled={busy || !selectionComplete(selection)}
      >
        {checking
          ? tx("component.recipes.checking", "component.dlss.working")
          : tx("component.recipes.check", "component.gameDrawer.menu.rescan")}
      </button>
      <button class="rp-clear" data-testid="recipe-clear" onclick={clearSelection} disabled={busy}>
        {tx("component.recipes.add.clear", "component.gameDrawer.clearSelection")}
      </button>
    </div>
  </section>

  {#if activePreview && summary}
    <section class="rp-group" data-testid="recipe-preview" aria-label={tx("component.recipes.preview.heading", "component.gameDrawer.stat.files")}>
      <span class="rp-group-title">{tx("component.recipes.preview.heading", "component.gameDrawer.stat.files")}</span>

      {#if importedCompatibility && importedSupport}
        <div class="rp-chips" data-testid="recipe-import-state">
          <span class="rp-chip" data-axis="compatibility" data-state={importedCompatibility.key} data-tone={importedCompatibility.tone}>
            {tx("component.recipes.state.compatibility." + importedCompatibility.key, "status.unknown")}
          </span>
          <span class="rp-chip" data-axis="support" data-state={importedSupport.key} data-tone={importedSupport.tone}>
            {tx("component.recipes.state.support." + importedSupport.key, "status.unknown")}
          </span>
        </div>
        <p class="rp-note" data-testid="recipe-import-note">
          {tx("component.recipes.import.unverified", "component.gameDrawer.status.externallyManaged")}
        </p>
      {/if}

      <div class="rp-stats">
        <span class="rp-stat" data-testid="recipe-file-count">
          <span class="rp-stat-label">{tx("component.recipes.preview.filesLabel", "component.gameDrawer.stat.files")}</span>
          <span class="rp-stat-value">{summary.fileCount}</span>
        </span>
        <span class="rp-stat" data-testid="recipe-config-count">
          <span class="rp-stat-label">{tx("component.recipes.preview.settingsLabel", "component.gameDrawer.stat.updates")}</span>
          <span class="rp-stat-value">{summary.configEditCount}</span>
        </span>
      </div>

      {#if activePreview.files.length > 0}
        <ul class="rp-files" data-testid="recipe-preview-files">
          {#each activePreview.files as file (file.relativePath)}
            <li data-action={file.action} data-role={file.role}>
              <span class="rp-file-action">{tx("component.recipes.preview.action." + file.action, "common.update")}</span>
              <span class="mono rp-file-name" title={file.relativePath}>{fileName(file.relativePath)}</span>
              <span class="rp-file-role">{tx("component.recipes.role." + file.role, "component.gameDrawer.featureFallback")}</span>
            </li>
          {/each}
        </ul>
      {/if}

      {#if activePreview.configEdits.length > 0}
        <ul class="rp-files" data-testid="recipe-preview-config">
          {#each activePreview.configEdits as edit (edit.relativePath + "|" + (edit.section ?? "") + "|" + edit.key)}
            <li>
              <span class="mono rp-file-name" title={edit.relativePath}>{fileName(edit.relativePath)}</span>
              <span class="mono rp-config-key">{edit.section ? edit.section + " / " + edit.key : edit.key}</span>
              <span class="mono rp-config-value">{edit.desired}</span>
            </li>
          {/each}
        </ul>
      {/if}

      {#if previewConflicts.length > 0}
        <div class="rp-conflicts" data-testid="recipe-conflicts" role="status">
          <p class="rp-conflicts-head">{tx("component.recipes.conflicts.heading", "view.backups.meta.missingHint")}</p>
          <ul class="rp-issue-list">
            {#each previewConflicts as issue, index (issue.code + index)}
              {@const view = conflictView(issue)}
              <li data-testid="recipe-conflict" data-code={issue.code} data-path={view.path} data-party={view.otherParty}>
                {issueText($locale, issue)}
              </li>
            {/each}
          </ul>
          <p class="rp-note">{tx("component.recipes.conflicts.blocked", "view.backups.entry.missingHint")}</p>
        </div>
      {/if}

      {#if previewProblems.length > 0}
        <div class="rp-problems" data-testid="recipe-problems" role="status">
          <p class="rp-conflicts-head">{tx("component.recipes.issues.heading", "status.scan_failed")}</p>
          <ul class="rp-issue-list">
            {#each previewProblems as issue, index (issue.code + index)}
              <li data-testid="recipe-problem" data-code={issue.code}>{issueText($locale, issue)}</li>
            {/each}
          </ul>
        </div>
      {/if}

      <div class="rp-actions">
        <button class="rp-commit" data-testid="recipe-commit" onclick={commit} disabled={!gate.ready}>
          {#if writing}
            {tx("component.recipes.working", "component.dlss.working")}
          {:else if selection.intent === "configure"}
            {tx("component.recipes.configure", "common.apply")}
          {:else}
            {tx("component.recipes.apply", "common.install")}
          {/if}
        </button>
      </div>
      {#if gate.reason && gate.reason !== "busy"}
        <p class="rp-note" data-testid="recipe-block-reason" data-reason={gate.reason}>
          {tx("component.recipes.blocked." + gate.reason, "status.unknown")}
        </p>
      {/if}
      <p class="rp-note">{tx("component.recipes.preview.recheck", "view.backups.legend.restoredHint")}</p>
    </section>
  {/if}

  {#if actionError}
    <p class="rp-error" role="alert" data-testid="recipe-action-error">
      {tx("component.recipes.error.actionFailed", "status.scan_failed")}
      <span class="rp-detail">{actionError}</span>
    </p>
  {/if}

  {#if result && outcome}
    <section class="rp-group" data-testid="recipe-result" data-status={result.status} aria-live="polite">
      <span class="rp-group-title">
        {outcome.failed
          ? tx("component.recipes.result.failed", "view.journal.failed")
          : tx("component.recipes.result.completed", "component.gameDrawer.stat.updates")}
      </span>
      <div class="rp-stats">
        <span class="rp-stat" data-testid="recipe-changed-count">
          <span class="rp-stat-label">{tx("component.recipes.result.changedLabel", "component.gameDrawer.stat.files")}</span>
          <span class="rp-stat-value">{outcome.changedCount}</span>
        </span>
        <span class="rp-stat" data-testid="recipe-retained-count">
          <span class="rp-stat-label">{tx("component.recipes.result.retainedLabel", "component.gameDrawer.status.externallyManaged")}</span>
          <span class="rp-stat-value">{outcome.retainedCount}</span>
        </span>
      </div>
      {#if result.retainedPaths.length > 0}
        <ul class="rp-paths" data-testid="recipe-retained-paths">
          {#each result.retainedPaths as path (path)}
            <li class="mono">{path}</li>
          {/each}
        </ul>
        <p class="rp-note">{tx("component.recipes.result.retainedHint", "component.gameDrawer.status.externallyManaged")}</p>
      {/if}
      {#if outcome.issues.length > 0}
        <ul class="rp-issue-list" data-testid="recipe-result-issues">
          {#each outcome.issues as issue, index (issue.code + index)}
            <li data-code={issue.code}>{issueText($locale, issue)}</li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>

<style>
  .rp {
    display: flex;
    flex-direction: column;
    gap: 14px;
    container-type: inline-size;
  }
  .rp-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .rp-head h4 {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .rp-group {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-card);
  }
  .rp-group-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: var(--letter-wider);
    color: var(--text-muted);
  }
  .rp-note {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-muted);
    margin: 0;
  }
  .rp-error {
    font-size: 12px;
    line-height: 1.5;
    margin: 0;
    color: var(--danger, #d65a5a);
    background: var(--danger-dim, rgba(214, 90, 90, 0.12));
    border: 1px solid var(--danger, #d65a5a);
    border-radius: var(--radius-md);
    padding: 8px 12px;
  }
  .rp-detail {
    display: block;
    color: var(--text-muted);
  }
  .rp-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .rp-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--bg-elevated);
  }
  .rp-row-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .rp-row-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    flex: 1 1 auto;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .rp-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .rp-chip {
    font-size: 11px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: var(--radius-full);
    background: var(--bg-card);
    color: var(--text-secondary);
    border: 1px solid var(--border);
  }
  .rp-chip[data-tone="accent"] {
    background: var(--accent-dim);
    color: var(--accent);
    border-color: transparent;
  }
  .rp-chip[data-tone="success"] {
    background: var(--success-dim, var(--accent-dim));
    color: var(--success, var(--accent));
    border-color: transparent;
  }
  .rp-chip[data-tone="warning"] {
    background: var(--warning-dim, rgba(214, 160, 50, 0.12));
    color: var(--warning, #d6a032);
    border-color: transparent;
  }
  .rp-chip[data-tone="danger"] {
    background: var(--danger-dim, rgba(214, 90, 90, 0.12));
    color: var(--danger, #d65a5a);
    border-color: transparent;
  }
  .rp-paths,
  .rp-issue-list,
  .rp-files {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-muted);
  }
  .rp-files li {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .rp-file-action {
    min-width: 64px;
    font-weight: 600;
    color: var(--text-secondary);
  }
  .rp-file-name {
    flex: 1 1 auto;
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--text-secondary);
  }
  .rp-file-role,
  .rp-config-key,
  .rp-config-value {
    color: var(--text-muted);
  }
  .rp-field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
  }
  .rp-field-label {
    font-size: 13px;
    color: var(--text-secondary);
    flex: 1 1 auto;
    min-width: 0;
  }
  .rp-control {
    flex: 0 0 auto;
    width: 240px;
    max-width: 60%;
  }
  .rp-picked {
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-muted);
    margin: 0;
    overflow-wrap: anywhere;
  }
  .rp-actions {
    display: flex;
    gap: 10px;
  }
  .rp-check,
  .rp-commit,
  .rp-clear,
  .rp-choose,
  .rp-remove {
    height: 34px;
    padding: 0 14px;
    border-radius: var(--radius-lg);
    font-size: 13px;
    font-weight: 600;
    background: var(--bg-elevated);
    color: var(--text-secondary);
  }
  .rp-commit {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .rp-commit:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .rp-check:hover:not(:disabled),
  .rp-clear:hover:not(:disabled),
  .rp-choose:hover:not(:disabled),
  .rp-remove:hover:not(:disabled) {
    color: var(--text-primary);
  }
  .rp-check:focus-visible,
  .rp-commit:focus-visible,
  .rp-clear:focus-visible,
  .rp-choose:focus-visible,
  .rp-remove:focus-visible {
    outline: none;
    box-shadow: var(--shadow-ring);
  }
  .rp-check:disabled,
  .rp-commit:disabled,
  .rp-clear:disabled,
  .rp-choose:disabled,
  .rp-remove:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .rp-stats {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
  }
  .rp-stat {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .rp-stat-label {
    font-size: 11px;
    color: var(--text-muted);
  }
  .rp-stat-value {
    font-size: 15px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .rp-conflicts,
  .rp-problems {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
    background: var(--bg-elevated);
  }
  .rp-conflicts {
    border-color: var(--warning, #d6a032);
  }
  .rp-conflicts-head {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }
  @container (max-width: 380px) {
    .rp-field {
      flex-direction: column;
      align-items: stretch;
    }
    .rp-control {
      width: 100%;
      max-width: none;
    }
  }
</style>
