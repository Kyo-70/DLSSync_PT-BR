<script lang="ts">
  import { pendingUpdatePlan, completeUpdatePlan } from "../features/update-plan/model";
  import { getCatalogStatus, previewUpdatePlan, type CatalogRuntimeStatus, type UpdatePlan } from "../lib/api";
  import { buildApplyRequests } from "../lib/applyController";
  import { formatError } from "../lib/stores";
  import { familyLabel } from "../lib/labels";
  import { t, locale } from "../lib/i18n/index";
  import { focusTrap } from "../actions/focusTrap";

  type Evidence = { source: string; signer: string | null; sha256: string; algorithm: string; signed: boolean };
  let selected = $state<Record<string, boolean>>({});
  let evidence = $state<Record<string, Evidence>>({});
  let status = $state<CatalogRuntimeStatus | null>(null);
  let serverPlan = $state<UpdatePlan | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let loadedKey = $state("");

  const normalizePath = (path: string): string => path.replace(/^\\\\\?\\/, "").replace(/\\/g, "/").toLowerCase();
  const targetKey = (path: string, version: string): string => `${normalizePath(path)}::${version}`;

  $effect(() => {
    const pending = $pendingUpdatePlan;
    if (!pending) { loadedKey = ""; return; }
    const key = pending.targets.map((target) => targetKey(target.record.path, target.target_version)).join("|");
    if (key === loadedKey) return;
    loadedKey = key;
    selected = Object.fromEntries(pending.targets.map((target) => [targetKey(target.record.path, target.target_version), true]));
    void loadEvidence(pending.targets, key);
  });

  async function loadEvidence(targets: NonNullable<typeof $pendingUpdatePlan>["targets"], key: string): Promise<void> {
    loading = true; error = null; status = null; serverPlan = null;
    try {
      const [currentStatus, plan] = await Promise.all([getCatalogStatus(), previewUpdatePlan(buildApplyRequests(targets))]);
      if (loadedKey !== key) return;
      if (plan.schema_version !== 1 || !plan.changes || !plan.catalog_revision) throw new Error("Incomplete verified update plan");
      status = currentStatus; serverPlan = plan;
      evidence = Object.fromEntries(plan.changes.map((change) => [
        targetKey(change.precondition.absolute_path, change.artifact.file_version ?? ""), {
          source: change.artifact.source_url, signer: change.artifact.expected_publisher,
          sha256: change.artifact.hash.digest, algorithm: change.artifact.hash.algorithm === "md5" ? "MD5" : "SHA-256",
          signed: change.artifact.signature_status === "verified",
        },
      ]));
    } catch (failure) { if (loadedKey === key) error = formatError(failure); }
    finally { if (loadedKey === key) loading = false; }
  }

  function selectedTargets(): NonNullable<typeof $pendingUpdatePlan>["targets"] {
    return ($pendingUpdatePlan?.targets ?? []).filter((target) => selected[targetKey(target.record.path, target.target_version)]);
  }

  function toggleTarget(key: string, checked: boolean): void {
    const change = serverPlan?.changes?.find((change) => targetKey(change.precondition.absolute_path, change.artifact.file_version ?? "") === key);
    const next = { ...selected, [key]: checked };
    if (change) {
      for (const member of serverPlan?.changes ?? []) {
        if (member.set_id === change.set_id) next[targetKey(member.precondition.absolute_path, member.artifact.file_version ?? "")] = checked;
      }
    }
    selected = next;
  }

  const dependencies = $derived((serverPlan?.changes ?? []).filter((change) => change.added_as_dependency &&
    (serverPlan?.changes ?? []).some((member) => !member.added_as_dependency && member.set_id === change.set_id && selected[targetKey(member.precondition.absolute_path, member.artifact.file_version ?? "")])));

  async function apply(): Promise<void> {
    if (!status || !serverPlan || loading) return;
    const targets = selectedTargets();
    if (targets.length === 0) return;
    loading = true; error = null;
    try {
      const plan = await previewUpdatePlan(buildApplyRequests(targets), serverPlan);
      completeUpdatePlan({ targets, catalogGeneratedAt: plan.catalog_generated_at, plan });
    } catch (failure) { error = formatError(failure); }
    finally { loading = false; }
  }

  async function exportPlan(): Promise<void> {
    if (!serverPlan) return;
    try {
      const plan = await previewUpdatePlan(buildApplyRequests(selectedTargets()), serverPlan);
      await navigator.clipboard.writeText(JSON.stringify(plan, null, 2));
    } catch (failure) { error = formatError(failure); }
  }
</script>

{#if $pendingUpdatePlan}
  <div class="plan-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) completeUpdatePlan(null); }}>
    <div
      class="plan-modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="update-plan-title"
      use:focusTrap
    >
      <header>
        <div><h2 id="update-plan-title">{$t("component.updatePlan.title")}</h2><p>{$t("component.updatePlan.subtitle")}</p></div>
        <button class="close" aria-label={$t("common.close")} onclick={() => completeUpdatePlan(null)}>×</button>
      </header>
      {#if error}<p class="plan-error" role="alert">{error}</p>{/if}
      <div class="plan-list">
        {#each $pendingUpdatePlan.targets as target (targetKey(target.record.path, target.target_version))}
          {@const key = targetKey(target.record.path, target.target_version)}
          <label class="plan-row" class:is-selected={selected[key]}>
            <input type="checkbox" checked={selected[key]} onchange={(event) => toggleTarget(key, event.currentTarget.checked)} disabled={loading || !serverPlan}>
            <div class="file-main">
              <strong class="file-name">{target.game_label}</strong>
              <span class="file-path" title={target.record.path}>{familyLabel(target.record.family)}</span>
              <div class="version-delta" aria-label="{target.record.current_version ?? '?'} → {target.target_version}">
                <span class="v-from mono">{target.record.current_version ?? "?"}</span>
                <svg class="v-arrow" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/></svg>
                <span class="v-to mono">{target.target_version}</span>
              </div>
            </div>
          </label>
        {/each}
        {#each dependencies as dependency (dependency.precondition.absolute_path)}
          <div class="plan-row is-selected" data-testid="plan-dependency">
            <span aria-hidden="true">+</span>
            <div class="file-main"><strong>{$t("component.updatePlan.dependencies")}</strong><code class="file-path">{dependency.precondition.identity.relative_path}</code><span>{dependency.artifact.file_version ?? "?"}</span></div>
          </div>
        {/each}
      </div>
      <details class="plan-technical">
        <summary>{$t("common.technicalDetails")}</summary>
      <div class="plan-proof">
        <span class="proof-item" class:is-verified={status?.provenance.signature_verified} data-testid="plan-proof-signature">
          <svg class="proof-glyph" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/></svg>
          {status?.provenance.signature_verified ? "✓" : "—"} Ed25519
        </span>
        <span class="proof-item">
          <svg class="proof-glyph" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15 14"/></svg>
          {status ? new Date(status.provenance.generated_at).toLocaleString($locale) : $t("component.updatePlan.loading")}
        </span>
        <span class="proof-item">
          <svg class="proof-glyph" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
          {$t("component.applyModal.fileCount", { count: $pendingUpdatePlan.targets.length })}
        </span>
      </div>
        {#each $pendingUpdatePlan.targets as target (targetKey(target.record.path, target.target_version))}
          {@const proof = evidence[targetKey(target.record.path, target.target_version)]}
          <div class="technical-row"><strong>{target.record.path.split(/[\\/]/).pop()}</strong><span>{proof?.signer ?? "—"}</span><code>{proof?.algorithm ?? ""} {proof?.sha256 ?? "—"}</code><span>{proof?.source ?? "—"}</span></div>
        {/each}
      </details>
      <footer><button class="btn btn-ghost" onclick={exportPlan} disabled={!status}>{$t("component.updatePlan.export")}</button><div class="actions"><button class="btn btn-ghost" onclick={() => completeUpdatePlan(null)}>{$t("common.cancel")}</button><button class="btn btn-primary" onclick={apply} disabled={loading || !serverPlan || selectedTargets().length === 0}>{$t("component.updatePlan.apply")}</button></div></footer>
    </div>
  </div>
{/if}

<style>
  .plan-technical { padding: 12px 24px; color: var(--text-secondary); }
  .plan-technical summary { cursor: pointer; }
  .technical-row { display: grid; gap: 6px; padding: 12px 0; overflow-wrap: anywhere; font-size: var(--fs-xs); }
  .plan-error { color: var(--color-danger, #e05252); padding: 0 20px; overflow-wrap: anywhere; }
  .plan-backdrop{position:fixed;inset:0;z-index:1000;display:grid;place-items:center;padding:24px;background:rgba(3,5,9,.72);backdrop-filter:blur(14px)}
  .plan-modal{width:min(960px,96vw);max-height:88vh;display:flex;flex-direction:column;overflow:hidden;border:1px solid var(--border);border-radius:var(--radius-2xl);background:var(--bg-elevated);box-shadow:var(--shadow-lg)}
  header{display:flex;justify-content:space-between;gap:24px;padding:22px 26px 14px}h2{margin:3px 0;font-size:var(--fs-2xl);letter-spacing:var(--letter-tighter)}header p{margin:0;color:var(--text-muted);font-size:var(--fs-sm)}.close{border:0;background:transparent;color:var(--text-muted);font-size:28px;line-height:1;cursor:pointer;border-radius:var(--radius-md);width:32px;height:32px}.close:hover{background:var(--bg-input);color:var(--text-primary)}

  .plan-proof{display:flex;gap:8px;flex-wrap:wrap;padding:0 26px 18px}
  .plan-proof .proof-item{display:inline-flex;align-items:center;gap:6px;padding:6px 12px;border-radius:var(--radius-full);background:var(--bg-input);border:1px solid var(--border);color:var(--text-secondary);font-size:var(--fs-xs);font-weight:600;font-variant-numeric:tabular-nums}
  .plan-proof .proof-glyph{color:var(--text-muted);flex-shrink:0}
  .plan-proof .proof-item.is-verified{background:var(--success-dim);border-color:color-mix(in oklab,var(--success) 32%,transparent);color:var(--success)}
  .plan-proof .proof-item.is-verified .proof-glyph{color:var(--success)}

  .plan-list{overflow:auto;padding:0 22px 14px;display:flex;flex-direction:column;gap:10px}
  .plan-row{display:grid;grid-template-columns:auto minmax(0,1fr);gap:16px;align-items:center;padding:14px 16px;border:1px solid var(--border);border-radius:var(--radius-lg);background:var(--bg-card);transition:border-color var(--dur-fast) var(--ease),background var(--dur-fast) var(--ease)}
  .plan-row:hover{border-color:var(--border-hover)}
  .plan-row.is-selected{border-color:color-mix(in oklab,var(--accent) 42%,var(--border));background:color-mix(in oklab,var(--accent) 5%,var(--bg-card))}
  .plan-row input{width:18px;height:18px;flex-shrink:0;accent-color:var(--accent)}
  .file-main{min-width:0;display:flex;flex-direction:column;gap:7px}
  .file-name{font-size:var(--fs-base);font-weight:650;color:var(--text-primary);letter-spacing:var(--letter-tight);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .file-path{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--text-muted);font-family:var(--font-mono);font-size:var(--fs-2xs)}
  .version-delta{display:inline-flex;align-items:center;gap:9px;align-self:flex-start;padding:4px 12px;border-radius:var(--radius-full);background:var(--bg-input);border:1px solid var(--border);font-variant-numeric:tabular-nums}
  .version-delta .v-from{color:var(--text-muted);font-size:var(--fs-sm);text-decoration:line-through;text-decoration-color:color-mix(in oklab,var(--text-muted) 55%,transparent)}
  .version-delta .v-arrow{color:var(--text-placeholder);flex-shrink:0}
  .version-delta .v-to{color:var(--success);font-weight:700;font-size:var(--fs-sm)}


  footer{display:flex;justify-content:space-between;gap:12px;padding:16px 24px;border-top:1px solid var(--border);background:var(--bg-elevated)}.actions{display:flex;gap:10px}
  @media(max-width:760px){.plan-row{grid-template-columns:auto minmax(0,1fr);row-gap:12px}.version-delta{align-self:flex-start}footer{align-items:stretch;flex-direction:column}.actions{display:grid;grid-template-columns:1fr 1fr}}
</style>
