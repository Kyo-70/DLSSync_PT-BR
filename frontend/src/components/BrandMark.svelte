<script lang="ts">
  import {
    BRANDS,
    brandfetchLogoUrl,
    resolveBrandDomain,
    resolveBrandKey,
    type BrandKey,
  } from "../lib/brands";

  let {
    key = undefined,
    label = undefined,
    size = 14,
    showLabel = true,
    tone = "color",
    fit = "square",
  }: {
    key?: string | null;
    label?: string;
    size?: number;
    showLabel?: boolean;
    tone?: "mono" | "color";
    fit?: "square" | "wordmark";
  } = $props();

  let resolved = $derived<BrandKey | null>(resolveBrandKey(key));
  let brand = $derived(resolved ? BRANDS[resolved] : null);
  let remoteUrl = $derived(brand ? null : brandfetchLogoUrl(resolveBrandDomain(key), { size: size * 2 }));
  let remoteFailed = $state(false);
  let showRemote = $derived(!brand && !!remoteUrl && !remoteFailed);
  let text = $derived(label ?? brand?.label ?? key ?? "");
  let accent = $derived(brand && tone === "color" ? `var(${brand.accentVar})` : "currentColor");
  // These bounds preserve the original paths. Wordmarks must not be compressed into an icon square.
  const wordmarkBounds: Record<string, { viewBox: string; ratio: number }> = {
    amd: { viewBox: "0 9.136 24 5.728", ratio: 24 / 5.728 },
    intel: { viewBox: "0 7.344 24 9.312", ratio: 24 / 9.312 },
    nvidia: { viewBox: "0 4.062 24 15.876", ratio: 24 / 15.876 },
  };
  let bounds = $derived(fit === "wordmark" && resolved ? wordmarkBounds[resolved] : undefined);
  let wordmarkHasName = $derived(fit === "wordmark" && (resolved === "amd" || resolved === "intel"));

  $effect(() => {
    void remoteUrl;
    remoteFailed = false;
  });
</script>

<span class="brand-mark" data-tone={tone} data-fit={fit} title={text} aria-label={text}>
  {#if brand}
    <svg
      class="brand-glyph"
      style:width={`${size * (bounds?.ratio ?? 1)}px`}
      style:height={`${size}px`}
      style:color={accent}
      viewBox={bounds?.viewBox ?? brand.viewBox}
      fill="currentColor"
      aria-hidden="true"
    >
      <path d={brand.path} />
    </svg>
  {:else if showRemote}
    <img
      class="brand-img"
      src={remoteUrl}
      alt=""
      width={size}
      height={size}
      loading="lazy"
      aria-hidden="true"
      onerror={() => (remoteFailed = true)}
    />
  {/if}
  {#if showLabel && text && !wordmarkHasName}
    <span class="brand-label">{text}</span>
  {/if}
</span>

<style>
  .brand-mark {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .brand-glyph {
    flex-shrink: 0;
    display: block;
  }
  .brand-img {
    flex-shrink: 0;
    display: block;
    object-fit: contain;
    border-radius: var(--radius-xs, 3px);
  }
  .brand-label {
    line-height: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
