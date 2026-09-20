# DLSSync layout system

The user's 2026-09-19 correction owns this design: preserve the existing colors and fonts. The light canvas is `#FFFFFF`; the dark canvas is `#000000`. Geist remains the UI font. JetBrains Mono remains the version and measurement font.

## Layout

- Use one main content alignment and a 24–32px inset, with responsive wrapping to the available width.
- Place primary actions beside the view heading or its immediate status summary.
- Use a spacious, artwork-led gallery by default. Preserve compact and table modes as explicit alternatives.
- Use readable metric groups with fewer enclosing containers. Retain generous section spacing.
- Make settings navigation and content distinct without an enclosing card around both.
- Keep local package controls in the Advanced tab of the centered game dialog.
- Preserve real data, unknown states, keyboard controls and all existing actions.
- Catalog lists exact library families. Vendor search retains their libraries; vendor buttons narrow the same result set. Do not hide named libraries under an "Other technologies" bucket.
- Use original vendor paths at their natural aspect ratio. Wordmark height, not a square SVG box, determines their visual size. Do not invent official subcomponent logos.
- Hover belongs to the actionable target. A notification dismiss control must not highlight its neighboring body. Keyboard focus stays visible on the focused control.
- At 960px and below, reserve a 64px navigation rail. Expansion overlays the content instead of squeezing it. This temporary layout must not overwrite the user's saved desktop preference.
- Use one narrow, theme-colored scrollbar with a transparent track. Hide scrollbar arrows in WebView2. Keep a forced-colors fallback and native light/dark controls.
- Settings puts file shortcuts behind an optional disclosure. Narrow layouts use one section selector. Labels and toggles stay in the same row; implementation details belong in their related setting disclosure.
- Drivers keeps graphics, devices and DLSS profiles on one page. Its section controls scroll to their matching sections. Do not repeat each card's health in a separate summary strip.
- Backup rows reserve explicit selection, vendor, content and action columns. Do not hide a grid child without changing its track placement. Catalog rows separate family identity, package version and history count; do not repeat generic feature blurbs for distinct support DLLs.
- The Library uses one continuous artwork grid. Counts, primary actions and optional filters stay compact above it. View changes render before persistence completes.
- Search opens next to its top-bar trigger. It includes actual games and commands, uses an opaque surface, and keeps keyboard navigation.
- Hardware affinity comes from typed local GPU observations. Include every detected vendor on hybrid systems. Other libraries remain accessible, and affinity never substitutes for Rust compatibility or eligibility. Unknown hardware permits browsing and explicit selection, but creates no default or automatic selection.

## Legibility

Body and secondary text use the existing typefaces at readable sizes. Keep status colors semantic, with theme-specific contrast. Small gray text must remain readable on the pure canvas. Avoid decorative glow, animated entrance delays, metallic button fills and nested translucent surfaces.

## Implementation

The earlier compact concepts were rejected and are not implementation authority. `src/styles/global.css` owns color, spacing, motion and type tokens. `src/styles/layout.css` coordinates cross-view refinements. Component-specific rules remain with the component. This is a layout redesign, not a new brand.

## Current evidence

The 2026-09-19 real-profile audit captured 98 unique frames. Its relative workspace location is `../plans/real-library-20260919/design-before-stable/index.html`; the implementation plan is `../plans/real-library-20260919/layout-audit.md`. The existing artwork-led product is the authority. No compact mockup is approved.

The user explicitly removed the Trust Center from the UI. Keep catalog verification in the backend; do not reintroduce a trust dashboard or disclosure.

## Game and driver detail surfaces

Game details use a centered, opaque dialog. The Library keeps its width underneath. Close sits at the top right. Updates and Advanced separate the primary update flow from local packages. Feature rows reserve explicit columns for selection, text and actions. Other GPU technologies and support libraries have separate disclosures. All dialog bodies use solid theme surfaces.

Catalog installation labels require matching SHA-256 evidence. The latest marker uses the family catalog target, not the first filtered row. NVIDIA profile persistence and in-game effects are separate states. Windows devices remain browsable from the local inventory, including on Nexus without a network request.

Backups uses a title, an overflow action menu, and the search/grouping toolbar. Do not add a summary dashboard or counts legend. Show missing or unverified file warnings only when applicable.
