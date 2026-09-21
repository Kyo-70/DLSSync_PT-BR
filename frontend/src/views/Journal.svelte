<script module lang="ts">
  import type { HistoryView, OperationKind, OperationRecord, OperationStatus } from "../lib/api";
  import { translate, type Locale, type TranslationVars } from "../lib/i18n/index";

  /** One Activity row. `source` records where the row came from, so the pre-snapshot fallback is
   *  never presented as the authoritative projection. */
  export interface HistoryRow {
    id: string;
    record: OperationRecord;
    recoveryOutcome: NonNullable<HistoryView["recovery_outcome"]> | null;
    at: string;
    source: "authoritative" | "journal_store";
  }

  /** Rows from the authoritative history projection (phase 4 contract, result 10.15).
   *  Nothing is reclassified here: the published record and its `recovery_outcome` pass through
   *  untouched, and the view only orders them for display. */
  export function projectHistoryRows(history: readonly HistoryView[]): HistoryRow[] {
    return history
      .map((item) => ({
        id: item.id,
        record: item.record,
        recoveryOutcome: item.recovery_outcome ?? null,
        at: item.historical_at ?? item.record.created_at,
        source: "authoritative" as const,
      }))
      .sort((left, right) => right.at.localeCompare(left.at));
  }

  /** Rows read from the journal command. Used only until the first authoritative history lands;
   *  they carry no recovery outcome because that projection is what publishes it. */
  export function journalStoreRows(records: readonly OperationRecord[]): HistoryRow[] {
    return records.map((record) => ({
      id: record.id,
      record,
      recoveryOutcome: null,
      at: record.created_at,
      source: "journal_store" as const,
    }));
  }

  /** Presentation filter over fields the backend already published. */
  export function filterHistoryRows(
    rows: readonly HistoryRow[],
    kind: OperationKind | "",
    status: OperationStatus | "",
  ): HistoryRow[] {
    return rows.filter(
      (row) =>
        (kind === "" || row.record.kind === kind) && (status === "" || row.record.status === status),
    );
  }

  /** Wording for a published recovery stage. An unlisted stage resolves to the neutral unknown
   *  label instead of being reported as a success or a failure. */
  const RECOVERY_FALLBACK_KEYS: Record<string, string> = {
    rolled_back: "view.journal.kinds.rollback",
    rollback_failed: "view.journal.failed",
  };

  export function recoveryLabel(outcome: string): { key: string; fallbackKey: string } {
    return {
      key: `view.journal.recovery.${outcome}`,
      fallbackKey: RECOVERY_FALLBACK_KEYS[outcome] ?? "status.unknown",
    };
  }

  /** Open-ended backend reason codes retain a neutral fallback for unknown values. */
  export function messageWithFallback(
    loc: Locale,
    key: string,
    fallbackKey: string,
    vars?: TranslationVars,
  ): string {
    const value = translate(loc, key, vars);
    return value === key ? translate(loc, fallbackKey, vars) : value;
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { exportJournal, listJournal } from "../lib/api";
  import { currentView } from "../lib/stores";
  import { authoritativeState } from "../lib/stateSync";
  import { locale, t } from "../lib/i18n/index";

  let records = $state<OperationRecord[]>([]);
  let loading = $state(true);
  let kind = $state<OperationKind | "">("");
  let status = $state<OperationStatus | "">("");

  // Rust owns Activity: the authoritative history projection is the source whenever it carries
  // rows. The journal command stays as the pre-snapshot fallback and is labelled as such.
  let projected = $derived(projectHistoryRows($authoritativeState.history));
  let usingProjection = $derived(projected.length > 0);
  let rows = $derived(
    filterHistoryRows(usingProjection ? projected : journalStoreRows(records), kind, status),
  );
  let syncPending = $derived($authoritativeState.historySyncPending);

  async function load(): Promise<void> {
    loading = true;
    try {
      records = await listJournal({ kind: kind || null, status: status || null, limit: 500 });
    } finally {
      loading = false;
    }
  }

  async function copyExport(): Promise<void> {
    const value = await exportJournal({ kind: kind || null, status: status || null, limit: 1000 });
    await navigator.clipboard.writeText(value);
  }

  onMount(() => { void load(); });
</script>

<div class="journal-page">
  <div class="view-tabs" role="group" aria-label={$t("view.backups.title")}>
    <button class="view-tab" data-testid="journal-to-backups" onclick={() => currentView.set("backups")}>{$t("view.backups.title")}</button>
    <button class="view-tab" aria-current="page">{$t("view.backups.activityTab")}</button>
  </div>
  <header class="view-header">
    <div>
      <h1 class="view-title">{$t("view.journal.title")}</h1>
      <p class="view-subtitle">{$t("view.journal.subtitle")}</p>
    </div>
    <button class="btn btn-ghost" onclick={copyExport}>{$t("view.journal.export")}</button>
  </header>

  <section class="journal-toolbar" aria-label={$t("view.journal.filters") }>
    <label><span>{$t("view.journal.kind")}</span><select bind:value={kind} onchange={load}><option value="">{$t("view.journal.all")}</option><option value="scan">{$t("view.journal.kinds.scan")}</option><option value="catalog_refresh">{$t("view.journal.kinds.catalog_refresh")}</option><option value="plan">{$t("view.journal.kinds.plan")}</option><option value="dll_apply">{$t("view.journal.kinds.dll_apply")}</option><option value="rollback">{$t("view.journal.kinds.rollback")}</option><option value="driver_install">{$t("view.journal.kinds.driver_install")}</option></select></label>
    <label><span>{$t("view.journal.result")}</span><select bind:value={status} onchange={load}><option value="">{$t("view.journal.all")}</option><option value="succeeded">{$t("view.journal.succeeded")}</option><option value="failed">{$t("view.journal.failed")}</option><option value="cancelled">{$t("view.journal.cancelled")}</option></select></label>
  </section>

  {#if syncPending}
    <p class="journal-empty" data-testid="journal-sync-pending">
      {translate($locale, "view.journal.syncPending")}
    </p>
  {/if}

  {#if !usingProjection && loading}
    <p class="journal-empty">{$t("view.journal.loading")}</p>
  {:else if rows.length === 0}
    <div class="journal-empty journal-empty-state">
      <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 1 0 3-6.7"/><polyline points="3 4 3 9 8 9"/><line x1="12" y1="8" x2="12" y2="12.5"/><line x1="12" y1="15.5" x2="12.01" y2="15.5"/></svg>
      <p class="journal-empty-title">{$t("view.journal.empty")}</p>
      <p class="journal-empty-hint">{$t("view.journal.subtitle")}</p>
    </div>
  {:else}
    <ol class="journal-list">
      {#each rows as row (row.id)}
        {@const record = row.record}
        <li
          class="journal-entry"
          data-status={record.status}
          data-source={row.source}
          data-recovery={row.recoveryOutcome}
        >
          <span class="entry-mark" aria-hidden="true"></span>
          <div class="entry-main">
            <div class="entry-line"><strong>{record.summary}</strong><span class="entry-status">{$t(`view.journal.${record.status}`)}</span></div>
            <div class="entry-meta"><span>{new Date(row.at).toLocaleString()}</span><span>{$t(`view.journal.actors.${record.actor}`)}</span><span>{$t(`view.journal.kinds.${record.kind}`)}</span>{#if record.duration_ms != null}<span>{record.duration_ms} ms</span>{/if}{#if row.recoveryOutcome}<span class="entry-recovery" data-testid="journal-recovery">{messageWithFallback($locale, recoveryLabel(row.recoveryOutcome).key, recoveryLabel(row.recoveryOutcome).fallbackKey)}</span>{/if}</div>
            {#if record.error}<p class="entry-error">{record.error}</p>{/if}
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</div>

<style>
  .journal-page { display: flex; flex-direction: column; gap: 18px; }
  .view-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  .journal-toolbar { display: flex; gap: 12px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--bg-card); }
  .journal-toolbar label { display: flex; align-items: center; gap: 8px; color: var(--text-muted); font-size: var(--fs-sm); }
  .journal-toolbar select { min-width: 140px; padding: 7px 10px; color: var(--text-primary); background: var(--bg-input); border: 1px solid var(--border); border-radius: var(--radius-md); }
  .journal-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .journal-entry { display: grid; grid-template-columns: 8px 1fr; gap: 12px; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--bg-card); }
  .entry-mark { width: 8px; height: 8px; margin-top: 5px; border-radius: 999px; background: var(--text-muted); box-shadow: 0 0 0 5px color-mix(in oklab, var(--text-muted) 10%, transparent); }
  .journal-entry[data-status="succeeded"] .entry-mark { background: var(--success); box-shadow: 0 0 0 5px color-mix(in oklab, var(--success) 12%, transparent); }
  .journal-entry[data-status="failed"] .entry-mark { background: var(--danger); box-shadow: 0 0 0 5px color-mix(in oklab, var(--danger) 12%, transparent); }
  .entry-line { display: flex; justify-content: space-between; gap: 12px; }
  .entry-status { color: var(--text-muted); font-size: var(--fs-xs); text-transform: uppercase; letter-spacing: .08em; }
  .entry-meta { display: flex; flex-wrap: wrap; gap: 6px 14px; margin-top: 6px; color: var(--text-muted); font-size: var(--fs-xs); }
  .entry-recovery { color: var(--text-secondary); font-weight: 600; }
  .journal-entry[data-recovery="rollback_failed"] .entry-recovery { color: var(--danger); }
  .journal-entry[data-recovery="rolled_back"] .entry-recovery { color: var(--success); }
  .entry-error { margin: 8px 0 0; color: var(--danger); font-size: var(--fs-sm); }
  .journal-empty { padding: 48px 16px; text-align: center; color: var(--text-muted); }
  .journal-empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 72px 16px;
    text-align: center;
    color: var(--text-muted);
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-card);
  }
  .journal-empty-state svg { opacity: 0.5; margin-bottom: 4px; }
  .journal-empty-title { font-size: var(--fs-md); font-weight: 600; color: var(--text-secondary); margin: 0; }
  .journal-empty-hint { font-size: var(--fs-sm); color: var(--text-muted); max-width: 46ch; margin: 0; line-height: var(--lh-snug); }
  @media (max-width: 700px) { .journal-toolbar { flex-direction: column; } .journal-toolbar label { justify-content: space-between; } }
</style>
