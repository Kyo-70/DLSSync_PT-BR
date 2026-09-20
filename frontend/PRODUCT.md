# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Existing Svelte and TypeScript interface in a Tauri v2 Windows desktop application. Rust owns update, recovery and device workflows.

## Users

PC players maintaining game DLLs and device drivers. This audience follows from the existing application; it is not a new product requirement.

## Product Purpose

Make available game component updates understandable and actionable. Keep verified backups and report actual outcomes. The primary update stays one click.

## Operating Context

Windows 10/11 x64. Seven existing views: Library, Catalog, Drivers, Backups, Journal, Settings and About. The minimum window is 900 by 560 pixels. Both light and dark themes must remain readable.

## Capabilities and Constraints

The backend owns eligibility, health, update actions, hashes and recovery results. Preserve these contracts and partial failures. Do not invent compatibility, performance, signature or successful-restore claims. Nexus requires explicit action before network access. No commits, publication or driver installation are part of the visual work.

## Brand Commitments

Keep the DLSSync name, existing logo, Geist and JetBrains Mono fonts, and current color identity. Light mode uses a pure #FFFFFF canvas; dark mode uses a pure #000000 canvas. Tony explicitly requested a complete, more beautiful, consistent and legible frontend. The current card-heavy layouts and weak light-theme contrast are rejected visual references.

## Evidence on Hand

Current source, seven runnable views and six user-supplied screenshots. Isolated fixtures support native acceptance. Fixture game images are unavailable and must retain an honest fallback.

## Product Principles

- Show the next useful action before secondary metadata.
- Keep advanced controls available without making the main task dense.
- Distinguish verified state from unknown state.
- Use one consistent control and typography system across both themes.

## Accessibility & Inclusion

Keep keyboard access and visible focus. Respect reduced motion. Support all eight registered locales. Maintain readable small text, semantic color labels and layouts at the minimum window size.
