# TODOS

## Bugs (P0)

- [ ] **Pre-existing test failure: `render::frame::tests::layer_order_honors_explicit_ids_then_array_order`** — panics with `invalid type: floating point 0.0, expected i32` in `frame.rs`. Not caused by any branch change; exists on main. Fix the test or the schema mismatch it's catching. Noticed on branch `claude/template-creation-text-prompts-o1ow08` during `/ship`.

## Later

- [ ] **Finalize the donation backend for the Fund the App page.** `website/content/fund.json` currently points `donateUrl` at `https://github.com/sponsors/walkersutton` as a **placeholder**. GitHub Sponsors can't be tied to a repo — it targets an account — so to make donations read as "Cyclemetry" not "Walker Sutton" we'd need a `cyclemetry` GitHub org (heavier; org goals are monthly-subscription oriented). Leaning toward **Ko-fi** (`ko-fi.com/cyclemetry`) instead: branded page, built-in one-time goal bar, low fees. Once chosen: update `donateUrl` in `fund.json`, and update the `raised`/`supporters`/`lastUpdated` totals by hand as donations arrive. The page renders its own $99/year progress graphic from that JSON, so the platform only needs to accept money under the Cyclemetry name.

- [x] **Move overlay trim ears to the bottom timeline.** The left sidebar already has draggable handles for `scene.start` / `scene.end`; move or duplicate that interaction on the bottom playback timeline so users can trim the overlay window where they scrub. The ears should bracket the overlay window within the full activity range, similar to the distance-reference dot, and keep the sidebar inputs in sync.

- [ ] **Aspect-ratio template variants.** Non-16:9 output now retargets the
  canvas and uniformly height-scales the authored template, so elements
  authored for 16:9 can fall off the sides on portrait/square (accepted for
  now). Future: let community templates ship per-aspect-ratio layout variants
  (e.g. 16:9 / 9:16 / 1:1) and pick the closest variant for the chosen output.

- [ ] **Composite elements.** A "composite" is a first-class UI concept that wraps a `scene.editor.groups` group behind a simpler interface. The user sees one thing (e.g. "Speed widget") instead of the 2–3 raw elements that make it up.
  - **Architecture (decided 2026-06): composites are built on the anchor primitive, not a new Rust element type.** Anchoring is the mechanism — `anchor: { target, point, self_point, offset_x, offset_y }` on label/value/rect/image pins an element to a point on a box element (plot/meter/gauge/rect/image), resolved by `Template::resolve_anchors()` before every render/preview/measure, with `vertical_align` keeping text optically centered as values change per frame. A composite is then just authoring sugar: a group whose members ship with anchors pre-wired. This avoids a combinatorial set of bespoke Rust composite variants and makes "expand" nearly a no-op (it already IS elements).
  - **Done:** the anchor primitive — schema + Rust resolve pre-pass, vertical text alignment, WYSIWYG support (dragging a target floats anchored followers live; dragging an anchored element commits to its offset; detach/delete bakes resolved x/y), Anchor section in the properties panel with per-point text-alignment presets.
  - **Remaining:** composite presets ("Insert → Gauge with value" creates the group with anchors pre-wired) + the compact composite panel.
  - Composites ARE groups — no new data model needed on disk
  - The composite editor surface is **intentionally smaller than the sum of its parts** — exposes only high-level controls (position, metric, color scheme), not every sub-element property. If "Expand" feels like a power-user escape hatch, the abstraction level is right.
  - "Expand to elements" is non-destructive: removes the `composite` flag, group becomes a normal group, all elements become individually editable
  - **Implementation:** add `composite: true` (or `composite_type: "speed" | "custom"`) flag inside `scene.editor.groups[n]`; editor renders a compact composite panel instead of individual element rows
  - Bundled templates are already halfway there — aaron's `speed_group`, jeff's `power_group` etc. are existing groups that just need the flag
  - **Synergy with color variables:** composite panel can expose `scene.vars` as its "color scheme" controls

- [ ] **Drop `natural_width`/`natural_height` from image elements.** These are captured at insert time and persisted, but are derivable from the image file at load time. Remove from the schema and resolve dimensions lazily in the Rust renderer via `imagesize` or equivalent. Currently present in aaron (1 image) and jeff (4 images).

- [ ] **Element properties audit.**
  1. **Basic vs. advanced categorization** — some properties are in the wrong bucket. Go through each element type and verify every property is correctly classified as basic (visible by default) or advanced (hidden behind toggle).
  2. **Per-element property sets** — some elements expose properties that don't apply to them. Remove anything that shouldn't be there.
  3. **Text properties on non-text elements** — some elements show text-related controls (font, italic, text_align, etc.) that are irrelevant to their type. Remove those panels/fields for element types that don't render text.
