<script lang="ts">
  import "./styles/layout.css";
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { invokeCommand as transport, COMMANDS } from "./generated/bindings";
  import { fade, fly } from "svelte/transition";
  import { portal } from "./actions/portal";
  import Sidebar from "./components/Sidebar.svelte";
  import TopBar from "./components/TopBar.svelte";
  import Toast from "./components/Toast.svelte";
  import UpdateBanner from "./components/UpdateBanner.svelte";
  import EfficiencyModeController from "./components/EfficiencyModeController.svelte";
  import CommandPalette from "./components/CommandPalette.svelte";
  import NotificationsBell from "./components/NotificationsBell.svelte";
  import LanguageMenu from "./components/LanguageMenu.svelte";
  import ShortcutOverlay from "./components/ShortcutOverlay.svelte";
  import SupportNudge from "./components/SupportNudge.svelte";
  import ApplyProgressModal from "./components/ApplyProgressModal.svelte";
  import ActivityDock from "./components/ActivityDock.svelte";
  import UpdatePlanModal from "./widgets/UpdatePlanModal.svelte";
  import Library from "./views/Library.svelte";
  import GameDetailDrawer from "./components/GameDetailDrawer.svelte";
  import {
    loadAboutView,
    loadBackupsView,
    loadCatalogView,
    loadDriversView,
    loadJournalView,
    loadSettingsView,
  } from "./lib/lazyViews";
  import {
    currentView,
    drawerGameId,
    loadSettings,
    settings,
    persistUiPreferences,
    ensureSystemInfo,
    bootstrapCatalog,
    requestThemeToggle,
    applyModalOpen,
    notificationsOpen,
    languageMenuOpen,
  } from "./lib/stores";
  import { activeArt, clearActiveArt } from "./lib/artContext";
  import { coverAccent } from "./lib/coverAccent";
  import { installApplyEventListeners } from "./lib/applyEvents";
  import { startStateSync } from "./lib/stateSync";
  import { installBackgroundScanListeners } from "./lib/backgroundScan";
  import {
    installDriverInstallListener,
    installSystemDriverListener,
  } from "./lib/driverInstallEvents";
  import { isLocale, loadLocale, localeFromNavigator, t } from "./lib/i18n/index";
  import { motionDuration } from "./lib/ux";
  import {
    createAppNavigationHistory,
    navigationDirectionForMouseButton,
    type AppNavigationState,
  } from "./lib/appNavigation";

  let theme = $state(localStorage.getItem("dlssync-theme") || "dark");

  function toggleTheme(): void {
    theme = theme === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem("dlssync-theme", theme);
    if ($settings) {
      void persistUiPreferences({ theme });
    }
  }

  let viewportWidth = $state(typeof window === "undefined" ? 1200 : window.innerWidth);
  let collapsed = $derived(viewportWidth <= 960 || ($settings?.ui_prefs.sidebar_collapsed ?? false));
  let railGameId = $derived($currentView === "library" ? $drawerGameId : null);

  const navHistory = createAppNavigationHistory({
    view: get(currentView),
    drawerGameId: get(drawerGameId),
  });
  $effect(() => {
    const state: AppNavigationState = { view: $currentView, drawerGameId: $drawerGameId };
    navHistory.record(state);
  });

  let gameAccent = $state<string | null>(null);
  $effect(() => {
    const url = $activeArt;
    if (!url) {
      gameAccent = null;
      return;
    }
    let stale = false;
    void coverAccent(url).then((color) => {
      if (!stale) gameAccent = color;
    });
    return () => {
      stale = true;
    };
  });

  onMount(async () => {
    await loadSettings();
    if ($settings) {
      const persistedTheme = $settings.ui_prefs.theme;
      if (persistedTheme && persistedTheme !== theme) {
        theme = persistedTheme;
      }
      const persistedLocale = $settings.ui_prefs.language;
      if (isLocale(persistedLocale)) {
        await loadLocale(persistedLocale);
      } else {
        const guess = localeFromNavigator();
        await loadLocale(guess);
        void persistUiPreferences({ language: guess });
      }
    }
    document.documentElement.setAttribute("data-theme", theme);
    // Authoritative state first: the listener is installed and the snapshot is loaded before any
    // dispatch, so a command response can never land on an uninitialised local view of state.
    // A failure here leaves the legacy listeners working instead of blocking startup.
    try {
      await startStateSync();
    } catch (error) {
      console.error("authoritative state sync unavailable", error);
    }
    void installApplyEventListeners();
    void installBackgroundScanListeners();
    void installDriverInstallListener();
    void installSystemDriverListener();
    void bootstrapCatalog();
    void ensureSystemInfo().catch(() => undefined);
  });

  let lastThemeSignal = $state(0);
  // Bumped by the retry control when a deferred view chunk fails to load, so the keyed block
  // re-evaluates the loader instead of showing the previous rejection.
  let viewAttempt = $state(0);
  $effect(() => {
    const n = $requestThemeToggle;
    if (n !== lastThemeSignal) {
      lastThemeSignal = n;
      if (n > 0) toggleTheme();
    }
  });

  $effect(() => {
    if ($currentView !== "library") {
      clearActiveArt();
      if ($drawerGameId) drawerGameId.set(null);
    }
  });

  onMount(() => {
    const onKey = async (e: KeyboardEvent): Promise<void> => {
      if (e.key === "F12") {
        e.preventDefault();
        try {
          await transport(COMMANDS.open_devtools);
        } catch {
          /* outside tauri context */
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  onMount(() => {
    const onMouse = (e: MouseEvent): void => {
      const direction = navigationDirectionForMouseButton(e.button);
      if (!direction) return;
      e.preventDefault();
      if (!navHistory.canMove(direction)) return;
      const target = navHistory.move(direction);
      if (!target) return;
      currentView.set(target.view);
      drawerGameId.set(target.drawerGameId);
    };
    window.addEventListener("mouseup", onMouse);
    return () => window.removeEventListener("mouseup", onMouse);
  });
</script>

<svelte:window bind:innerWidth={viewportWidth} />
<div class="app-shell layout-refined" class:sidebar-collapsed={collapsed}>
  <div class="app-ambient" aria-hidden="true" style:--game-accent={gameAccent}>
    <div class="ambient-mesh"></div>
    {#if $activeArt}
      <img class="ambient-art" src={$activeArt} alt="" transition:fade={{ duration: motionDuration(600) }} />
    {/if}
    {#if gameAccent}
      <div class="ambient-accent"></div>
    {/if}
    <div class="ambient-grain"></div>
  </div>
  <Sidebar />
  <TopBar onToggleTheme={toggleTheme} {theme} />
  <div class="app-main">
    <main class="main-content" class:dialog-open={!!railGameId}>
      <div class="main-inner">
        <div class="main-primary">
          {#snippet viewLoadFailed()}
            <div class="view-load-failed" role="alert">
              <button type="button" class="view-load-retry" onclick={() => (viewAttempt += 1)}>
                {$t("common.retry")}
              </button>
            </div>
          {/snippet}
          {#key viewAttempt}
            {#if $currentView === "library"}
              <div in:fly={{ y: 8, duration: motionDuration(200) }} data-testid="view-library"><Library /></div>
            {:else if $currentView === "catalog"}
              <div in:fly={{ y: 8, duration: motionDuration(200) }} data-testid="view-catalog">
                {#await loadCatalogView() then Catalog}<Catalog />{:catch}{@render viewLoadFailed()}{/await}
              </div>
            {:else if $currentView === "backups"}
              <div in:fly={{ y: 8, duration: motionDuration(200) }} data-testid="view-backups">
                {#await loadBackupsView() then Backups}<Backups />{:catch}{@render viewLoadFailed()}{/await}
              </div>
            {:else if $currentView === "journal"}
              <div in:fly={{ y: 8, duration: motionDuration(200) }} data-testid="view-journal">
                {#await loadJournalView() then Journal}<Journal />{:catch}{@render viewLoadFailed()}{/await}
              </div>
            {:else if $currentView === "drivers"}
              <div in:fly={{ y: 8, duration: motionDuration(200) }} data-testid="view-drivers">
                {#await loadDriversView() then Drivers}<Drivers />{:catch}{@render viewLoadFailed()}{/await}
              </div>
            {:else if $currentView === "settings"}
              <div in:fly={{ y: 8, duration: motionDuration(200) }} data-testid="view-settings">
                {#await loadSettingsView() then Settings}<Settings
                    onToggleTheme={toggleTheme}
                    currentTheme={theme}
                  />{:catch}{@render viewLoadFailed()}{/await}
              </div>
            {:else if $currentView === "about"}
              <div in:fly={{ y: 8, duration: motionDuration(200) }} data-testid="view-about">
                {#await loadAboutView() then About}<About />{:catch}{@render viewLoadFailed()}{/await}
              </div>
            {/if}
          {/key}
        </div>
      </div>
    </main>
  </div>
  {#if railGameId}
    <button class="rail-scrim" use:portal aria-label={$t("common.close")} onclick={() => drawerGameId.set(null)}></button>
    <aside class="game-detail-dialog" use:portal in:fade={{ duration: motionDuration(160) }}>
      <GameDetailDrawer
        gameId={railGameId}
        onClose={() => drawerGameId.set(null)}
        onApplyStart={() => applyModalOpen.set(true)}
      />
    </aside>
  {/if}
</div>
<Toast />
<ActivityDock />
<UpdatePlanModal />
<EfficiencyModeController />
<UpdateBanner />
<CommandPalette />
<NotificationsBell open={$notificationsOpen} onClose={() => notificationsOpen.set(false)} />
<LanguageMenu open={$languageMenuOpen} onClose={() => languageMenuOpen.set(false)} />
<ShortcutOverlay />
<SupportNudge />
{#if $applyModalOpen}
  <ApplyProgressModal onClose={() => applyModalOpen.set(false)} />
{/if}

<style>
  /* One cohesive floating app surface (sidebar + topbar + content integrated),
     on the subtle ambient backdrop. Rounded + overflow-clipped = the "docker" feel. */
  /* Edge-to-edge: the app fills the window (no inner margin → no square frame).
     The window's own corners are rounded by the OS. */
  .app-shell {
    position: fixed;
    inset: 0;
    z-index: 1;
    display: grid;
    grid-template-rows: var(--topbar-height) 1fr;
    grid-template-columns: var(--sidebar-width) minmax(0, 1fr) 0fr;
    overflow: hidden;
    background: transparent;
    transition: none;
  }
  .app-shell.sidebar-collapsed {
    grid-template-columns: var(--sidebar-width-collapsed) minmax(0, 1fr) 0fr;
  }
  .app-shell :global(.sidebar) {
    grid-row: 1 / -1;
    grid-column: 1;
    height: 100%;
    z-index: 70;
  }
  .app-shell :global(.topbar) {
    grid-row: 1;
    grid-column: 2 / -1;
    position: relative;
    z-index: 70;
  }
  @media (prefers-reduced-motion: reduce) {
    .app-shell { transition: none; }
  }
  .app-ambient {
    position: absolute;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    pointer-events: none;
    background: var(--bg-base);
  }
  .ambient-mesh {
    position: absolute;
    inset: -20%;
    background:
      radial-gradient(ellipse 72% 62% at 5% 0%, var(--ambient-glow-1), transparent 60%),
      radial-gradient(ellipse 60% 55% at 98% 6%, var(--ambient-glow-2), transparent 62%),
      radial-gradient(ellipse 82% 78% at 0% 100%, var(--ambient-glow-3), transparent 60%);
    animation: ambient-drift 64s ease-in-out infinite alternate;
  }
  /* Tidal-style art tint: a heavily blurred + darkened copy of the focused game's
     cover, behind all chrome so the frosted glass refracts real colour. */
  .ambient-art {
    position: absolute;
    inset: -15%;
    width: 130%;
    height: 130%;
    object-fit: cover;
    filter: blur(var(--art-blur)) saturate(var(--art-sat)) brightness(var(--art-dim));
    opacity: var(--art-opacity);
    pointer-events: none;
    -webkit-mask-image:
      linear-gradient(to right, #000 var(--art-edge-left), transparent var(--art-edge-left-fade)),
      linear-gradient(to bottom, #000 var(--art-edge-top), transparent var(--art-edge-top-fade));
    mask-image:
      linear-gradient(to right, #000 var(--art-edge-left), transparent var(--art-edge-left-fade)),
      linear-gradient(to bottom, #000 var(--art-edge-top), transparent var(--art-edge-top-fade));
    mask-composite: add;
  }
  @keyframes ambient-drift {
    from { transform: translate3d(0, 0, 0) scale(1); }
    to { transform: translate3d(2.5%, -2%, 0) scale(1.1); }
  }
  @media (prefers-reduced-motion: reduce) {
    .ambient-mesh { animation: none; }
  }
  .ambient-grain {
    position: absolute;
    inset: 0;
    background-image: var(--noise-url);
    opacity: 0.4;
    mix-blend-mode: overlay;
  }
  /* The content column: the topbar overlays the top so scrolling content frosts
     under it (real glass), and the sidebar is an integrated column to the left. */
  .app-main {
    grid-row: 2;
    grid-column: 2;
    position: relative;
    z-index: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .main-content {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-gutter: auto;
    background: transparent;
  }
  @media (max-width: 960px) {
    .app-shell, .app-shell.sidebar-collapsed { grid-template-columns: var(--sidebar-width-collapsed) minmax(0, 1fr) 0fr; }
    }
  .main-inner {
    max-width: var(--content-max);
    padding: clamp(18px, 2.4vw, 36px) clamp(18px, 3.2vw, 48px) clamp(16px, 2vw, 28px);
    margin: 0 auto;
  }
  .main-primary { min-width: 0; }
  .main-content.dialog-open { overflow: hidden; }
  .view-load-failed {
    display: flex;
    justify-content: center;
    padding: var(--space-6, 1.5rem);
  }
  .view-load-retry {
    border: 1px solid var(--border-strong, rgba(255, 255, 255, 0.24));
    border-radius: var(--radius-md, 8px);
    background: var(--bg-elevated, rgba(255, 255, 255, 0.06));
    color: var(--text-primary, #fff);
    padding: var(--space-2, 0.5rem) var(--space-4, 1rem);
    font-size: var(--text-sm, 0.875rem);
    cursor: pointer;
  }

  .game-detail-dialog {
    position: fixed;
    inset: 50% auto auto 50%;
    transform: translate(-50%, -50%);
    width: min(960px, calc(100vw - 40px));
    height: min(900px, calc(100dvh - 48px));
    z-index: 121;
    border-radius: 16px;
    background: var(--bg-card);
    box-shadow: 0 24px 80px rgba(0, 0, 0, .38);
    overflow: hidden;
  }
  .rail-scrim {
    position: fixed;
    inset: 0;
    z-index: 120;
    border: none;
    background: rgba(0, 0, 0, .62);
  }
  @media (max-width: 600px) {
    .game-detail-dialog { width: calc(100vw - 16px); height: calc(100dvh - 16px); border-radius: 12px; }
  }

  @media (max-width: 720px) {
    .main-inner { padding: 16px 16px 20px; }
  }
</style>
