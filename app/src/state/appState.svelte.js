import { open } from '@tauri-apps/plugin-dialog'
import * as backend from '../api/backend.js'
import {
  parseLocalStorage,
  dialogExtensions,
  exportBitsPerPixelSecond,
} from '../lib/utils.js'
import { elementTypeName } from '../lib/elementTypes.js'
import { stripDefaults } from '../lib/stripDefaults.js'
import { normalizeElementUpdates } from '../lib/templateSchema.js'
import {
  offsetForVideoStart,
  wallClockApplicable,
} from '../lib/videoAlignment.js'

// Dev-only: if the reset button set this flag, wipe localStorage before any
// state reads so the persist $effect can't race-restore the old values.
if (import.meta.env.DEV && sessionStorage.getItem('dev_reset') === '1') {
  sessionStorage.removeItem('dev_reset')
  localStorage.clear()
}

// ProRes 4444 bitrate varies wildly with overlay density, so calibration is
// keyed by template. The empty-string slot is the cross-template fallback used
// before a given template has been rendered.

// Timeline seconds rendered by the export-estimate test render — long enough
// to amortize FFmpeg/cache startup, short enough to stay a "quick test".
const CALIBRATION_SECONDS = 4

function loadExportSizeCalibration() {
  const empty = { templates: {} }
  const stored = parseLocalStorage('exportSizeCalibration')
  if (stored && typeof stored === 'object' && stored.templates) {
    // Migrate v1 per-template entries ({ bps, n }) to per-format
    // ({ prores: { bps, n } }) — all pre-format-picker renders were ProRes.
    const templates = {}
    for (const [key, val] of Object.entries(stored.templates)) {
      templates[key] = val?.bps > 0 ? { prores: val } : val
    }
    return { templates }
  }

  // Migrate the pre-per-template single-value key, if present.
  const legacy = parseFloat(
    localStorage.getItem('exportSizeBitsPerPixelSecond') ?? '',
  )
  localStorage.removeItem('exportSizeBitsPerPixelSecond')
  if (legacy > 0)
    return { templates: { '': { prores: { bps: legacy, n: 1 } } } }
  return empty
}

export function createAppState() {
  const storedActivityName = (value) => {
    if (!value || value === 'null' || value === 'undefined') return null
    const basename = value.split(/[\\/]/).pop() || null
    return /\.(gpx|fit|tcx)$/i.test(basename) ? basename : null
  }

  // ── Persistent ──────────────────────────────────────────────────────────────
  // config is the single source of truth: scene settings + all element positions
  let _persisted = parseLocalStorage('editorConfig')
  // Drop a pre-unified persisted config (old labels/values/plots shape); the
  // app reloads a template fresh rather than migrating stale localStorage.
  if (
    _persisted &&
    !Array.isArray(_persisted.elements) &&
    (_persisted.labels || _persisted.values || _persisted.plots)
  ) {
    _persisted = null
  }
  const initialConfig = migrateConfig(_persisted)
  let config = $state(initialConfig)
  const storedGpxFilename = storedActivityName(
    localStorage.getItem('gpxFilename'),
  )
  let gpxFilename = $state(storedGpxFilename)
  // Wall-clock UTC (ISO 8601) of the first GPX sample. `null` for sources
  // without timestamps. Set by the GPX load flow; used by the alignment
  // timeline to position the video clip on the activity's real-time axis.
  let gpxStartTime = $state(localStorage.getItem('gpxStartTime') || null)
  // Reference video for timeline alignment + (Phase 4) live overlay preview.
  // Stored by absolute path — file stays in place on disk; we never copy it.
  // `missing: true` means the path persisted from a prior session but the file
  // is no longer there, so the UI can prompt for relink.
  let video = $state(parseLocalStorage('projectVideo'))
  let videoUnderlayVisible = $state(
    localStorage.getItem('videoUnderlayVisible') !== 'false',
  )
  const storedActivityDuration = parseFloat(
    localStorage.getItem('activityDuration') ?? '',
  )
  const initialActivityDuration =
    storedGpxFilename && storedActivityDuration > 0 ? storedActivityDuration : 0
  let activityDuration = $state(initialActivityDuration)
  let activityMetrics = $state(
    storedGpxFilename ? (parseLocalStorage('activityMetrics') ?? null) : null,
  )
  let selectedSecond = $state(
    parseInt(localStorage.getItem('selectedSecond') ?? '0'),
  )
  let loadedTemplateFilename = $state(
    localStorage.getItem('loadedTemplateFilename') ?? null,
  )
  // Rider weight for the W/kg (power_to_weight) metric. Deliberately kept out of
  // the template/scene config so it is never saved into or shared via a template
  // (weight is sensitive personal data). Stored only on this device, and passed
  // to the renderer as a separate argument.
  const storedRiderWeight = parseFloat(
    localStorage.getItem('riderWeight') ?? '',
  )
  let riderWeight = $state(
    Number.isFinite(storedRiderWeight) ? storedRiderWeight : null,
  )
  let riderWeightUnit = $state(localStorage.getItem('riderWeightUnit') ?? 'kg')
  // Weight in kilograms for the renderer, or null when unset/invalid.
  function resolvedRiderWeightKg() {
    if (
      riderWeight == null ||
      !Number.isFinite(riderWeight) ||
      riderWeight <= 0
    )
      return null
    return riderWeightUnit === 'lb' ? riderWeight * 0.45359237 : riderWeight
  }
  // Whether the loaded template renders W/kg anywhere.
  function templateUsesRiderWeight() {
    return (config?.elements ?? []).some((e) => e.value === 'power_to_weight')
  }
  // Unit system ("metric" | "imperial") for every readout left on Auto —
  // per-element unit overrides still win. An app-level preference like rider
  // weight: configured once in Settings, stored on this device, injected into
  // the config at render time (withAppSettings) and never saved into the
  // template. Seeded from a pre-existing scene.units so upgrading keeps the
  // user's current choice.
  let units = $state(
    localStorage.getItem('units') ??
      (_persisted?.scene?.units === 'imperial' ? 'imperial' : 'metric'),
  )
  // Compound-unit style for compound speed units: false → "km/h" (default),
  // true → "kmh". App-level like `units`: stored on this device, injected into
  // scene.compact_units at render, never saved into the template.
  let compactUnits = $state(
    (localStorage.getItem('compactUnits') ??
      String(_persisted?.scene?.compact_units === true)) === 'true',
  )
  let outputDir = $state(localStorage.getItem('outputDir') ?? null)
  let defaultOutputDir = $state(null)
  let outputWidth = $state(
    parseInt(localStorage.getItem('outputWidth') ?? '3840'),
  )
  let outputHeight = $state(
    parseInt(localStorage.getItem('outputHeight') ?? '2160'),
  )
  // Export format: 'prores' (ProRes 4444, default) or 'qtrle' (QuickTime
  // Animation), both transparent-overlay .mov files; or 'stitched' — an H.264
  // .mp4 with the overlay composited onto the reference video.
  let exportFormat = $state(localStorage.getItem('exportFormat') ?? 'prores')
  // For transparent overlay exports: emit the full canvas (transparent dead
  // space around the overlay, drop-in at 0,0) instead of cropping to the
  // visible elements. Defaults to false (crop + placement-offset sidecar).
  let exportFullFrame = $state(
    localStorage.getItem('exportFullFrame') === 'true',
  )
  // Snapshot of `config` as it was last loaded/saved. Used to detect unsaved
  // template edits. Output resolution lives outside `config`, so switching a
  // 1080p template into a 4K view never marks it modified.
  let pristineConfig = $state(
    localStorage.getItem('pristineConfig') ??
      (initialConfig ? JSON.stringify(initialConfig) : null),
  )

  // ── Transient ────────────────────────────────────────────────────────────────
  let copiedElement = $state(null) // the copied element object — in-memory clipboard
  // Preview frame rate is fully automatic — the CenterCanvas tuner adapts it to
  // measured render throughput (balancing preview smoothness against resource
  // use). Persisted only as a sensible starting point; it's re-tuned each session.
  let previewFps = $state(parseInt(localStorage.getItem('previewFps') ?? '5'))
  let benchmarking = $state(false)
  let lastRenderFps = $state(
    parseFloat(localStorage.getItem('lastRenderFps') ?? '') || null,
  )
  // Wall-clock render throughput from real exports, keyed by codec — qtrle's
  // software encode can pace differently than hardware ProRes. `lastRenderFps`
  // stays the codec-agnostic fallback (benchmarks measure render-only).
  let renderFpsByFormat = $state(parseLocalStorage('renderFpsByFormat') ?? {})
  let exportSizeCalibration = $state(loadExportSizeCalibration())
  let renderingVideo = $state(false)
  let currentPreviewImage = $state(null) // data:image/png;base64,... from latest preview frame
  let errorMessage = $state(null)
  let successMessage = $state(null)
  let successTimer = null
  let templates = $state([])
  let fonts = $state([])
  let showTemplatePicker = $state(false)
  let selectedElementId = $state(null)
  let selectedElementIds = $state([])
  let selectedGroupId = $state(null)
  let selectedCourseMarkerId = $state(null)
  let renderProgress = $state({
    current: 0,
    total: 0,
    percent: 0,
    status: 'idle',
    estimatedSecondsRemaining: null,
    overlaySecondsRendered: 0,
    overlayTotalSeconds: 0,
  })

  // ── Persistence effects ───────────────────────────────────────────────────
  $effect(() => {
    if (config) localStorage.setItem('editorConfig', JSON.stringify(config))
  })
  $effect(() => {
    if (gpxFilename) localStorage.setItem('gpxFilename', gpxFilename)
    else localStorage.removeItem('gpxFilename')
  })
  $effect(() => {
    if (video) localStorage.setItem('projectVideo', JSON.stringify(video))
    else localStorage.removeItem('projectVideo')
  })
  $effect(() => {
    localStorage.setItem('videoUnderlayVisible', String(videoUnderlayVisible))
  })
  $effect(() => {
    if (gpxStartTime) localStorage.setItem('gpxStartTime', gpxStartTime)
    else localStorage.removeItem('gpxStartTime')
  })
  $effect(() => {
    localStorage.setItem('activityDuration', String(activityDuration))
  })
  $effect(() => {
    if (activityMetrics)
      localStorage.setItem('activityMetrics', JSON.stringify(activityMetrics))
    else localStorage.removeItem('activityMetrics')
  })
  $effect(() => {
    localStorage.setItem('selectedSecond', String(selectedSecond))
  })
  $effect(() => {
    if (loadedTemplateFilename)
      localStorage.setItem('loadedTemplateFilename', loadedTemplateFilename)
  })
  $effect(() => {
    if (outputDir) localStorage.setItem('outputDir', outputDir)
    else localStorage.removeItem('outputDir')
  })
  $effect(() => {
    if (riderWeight != null && Number.isFinite(riderWeight))
      localStorage.setItem('riderWeight', String(riderWeight))
    else localStorage.removeItem('riderWeight')
  })
  $effect(() => {
    localStorage.setItem('riderWeightUnit', riderWeightUnit)
  })
  $effect(() => {
    localStorage.setItem('units', units)
  })
  $effect(() => {
    localStorage.setItem('compactUnits', String(compactUnits))
  })
  $effect(() => {
    localStorage.setItem('outputWidth', String(outputWidth))
  })
  $effect(() => {
    localStorage.setItem('outputHeight', String(outputHeight))
  })
  $effect(() => {
    localStorage.setItem('exportFormat', exportFormat)
    localStorage.setItem('exportFullFrame', String(exportFullFrame))
  })
  $effect(() => {
    if (pristineConfig != null)
      localStorage.setItem('pristineConfig', pristineConfig)
    else localStorage.removeItem('pristineConfig')
  })
  $effect(() => {
    localStorage.setItem('previewFps', String(previewFps))
  })
  $effect(() => {
    localStorage.setItem(
      'exportSizeCalibration',
      JSON.stringify(exportSizeCalibration),
    )
  })
  $effect(() => {
    localStorage.setItem('renderFpsByFormat', JSON.stringify(renderFpsByFormat))
  })

  function templateSnapshot(value) {
    return value ? JSON.stringify(stripDefaults(toEditorFormat(value))) : null
  }

  function markPristine() {
    pristineConfig = templateSnapshot(config)
  }

  function templateModified() {
    return (
      !!config &&
      pristineConfig != null &&
      templateSnapshot(config) !== pristineConfig
    )
  }

  // Pending "discard unsaved edits?" action. When set, app.svelte shows a
  // ConfirmDialog; running or clearing it resolves the gate.
  let pendingDiscard = $state(null)

  function confirmIfModified(run) {
    if (templateModified()) pendingDiscard = run
    else run()
  }

  // ── Reference video ──────────────────────────────────────────────────────
  // Probe the file, capture container metadata, store by absolute path.
  // The video stays where the user put it on disk; we never copy.
  async function loadVideo(absolutePath) {
    if (!absolutePath) return
    try {
      const probe = await backend.probeVideo(absolutePath)
      // Normalize probe → in-memory shape before the wall-clock check, since
      // the helper expects creationTime (camelCase) + duration on a single
      // object.
      const draft = {
        path: probe.path,
        duration: probe.duration,
        creationTime: probe.creation_time,
        creationTimeNote: probe.creation_time_note ?? null,
        codec: probe.codec,
        width: probe.width,
        height: probe.height,
        userOffsetSec: 0,
        missing: false,
      }
      // Auto-align: if the camera's wall clock places the video inside the
      // activity period, trust it (offset = 0); otherwise snap the video's
      // first frame to the current overlay window start, so the user sees
      // it immediately on the alignment bar.
      const initialOffset = wallClockApplicable(
        gpxStartTime,
        draft,
        activityDuration,
      )
        ? 0
        : offsetForVideoStart(gpxStartTime, draft, config?.scene?.start ?? 0)
      video = { ...draft, userOffsetSec: initialOffset }
      videoUnderlayVisible = true
      // Preview FPS is fully automatic — the render-throughput tuner manages it,
      // so there's nothing to bump here when a video underlay loads.
    } catch (e) {
      errorMessage = `Could not read video: ${e?.message ?? e}`
    }
  }

  async function pickAndLoadVideo() {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: 'Video',
          extensions: dialogExtensions([
            'mp4',
            'mov',
            'm4v',
            'mkv',
            'webm',
            'avi',
            '360',
            'insv',
          ]),
        },
      ],
      title: 'Select reference video',
    })
    if (!selected) return
    await loadVideo(selected)
  }

  function clearVideo() {
    video = null
  }

  function setVideoUnderlayVisible(visible) {
    videoUnderlayVisible = visible
  }

  function setVideoOffset(seconds) {
    if (!video) return
    video = { ...video, userOffsetSec: seconds }
  }

  // Mark the video missing if its file disappeared between sessions. Cheap
  // re-probe is fine — ffmpeg refuses fast on a missing path.
  async function verifyVideo() {
    if (!video?.path) return
    try {
      await backend.probeVideo(video.path)
      if (video.missing) video = { ...video, missing: false }
    } catch {
      if (!video.missing) video = { ...video, missing: true }
    }
  }

  async function fetchDefaultOutputDir() {
    try {
      defaultOutputDir = await backend.defaultOutputDir()
    } catch {
      defaultOutputDir = null
    }
  }

  async function pickOutputDir() {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: outputDir ?? defaultOutputDir ?? undefined,
      title: 'Choose exports folder',
    })
    if (!selected) return false
    outputDir = selected
    showSuccess('Exports folder updated')
    return true
  }

  function resetOutputDir() {
    outputDir = null
    showSuccess('Using default exports folder')
  }

  // ProRes 4444 lives roughly in [0.1, 8] bits/pixel/second. Anything outside
  // [0.05, 10] is almost certainly a partial/corrupt file, not real signal.
  const MIN_VALID_BPS = 0.05
  const MAX_VALID_BPS = 10
  // Once we have enough samples, cap the new-sample weight so a single odd
  // render can't whiplash the calibration; below the cap, equal-weight running
  // mean (α = 1/n) gives the cleanest convergence.
  const MIN_SAMPLE_WEIGHT = 0.15

  function blendCalibration(prev, sample) {
    const nextN = (prev?.n ?? 0) + 1
    if (!prev?.bps || !(prev.n > 0)) return { bps: sample, n: 1 }
    const alpha = Math.max(MIN_SAMPLE_WEIGHT, 1 / nextN)
    return { bps: prev.bps * (1 - alpha) + sample * alpha, n: nextN }
  }

  function calibrationFormat(format) {
    if (format === 'qtrle' || format === 'stitched') return format
    return 'prores'
  }

  function recordExportSizeEstimate(actualBitsPerPixelSecond, format) {
    if (!(actualBitsPerPixelSecond >= MIN_VALID_BPS)) return
    if (!(actualBitsPerPixelSecond <= MAX_VALID_BPS)) return
    const fmt = calibrationFormat(format ?? exportFormat)
    const key = loadedTemplateFilename ?? ''
    const templates = exportSizeCalibration.templates ?? {}
    const blend = (slot) => ({
      ...slot,
      [fmt]: blendCalibration(slot?.[fmt], actualBitsPerPixelSecond),
    })
    exportSizeCalibration = {
      templates: {
        ...templates,
        [key]: blend(templates[key]),
        // The empty-key slot doubles as the cross-template fallback used for
        // never-rendered templates.
        ...(key === '' ? {} : { '': blend(templates['']) }),
      },
    }
  }

  // Size calibration comes only from real exports (or the quick test render)
  // in the same codec — the qtrle/ProRes ratio varies too much with overlay
  // content and machine to hardcode a cross-codec conversion.
  function currentExportSizeCalibration(format = exportFormat) {
    const templates = exportSizeCalibration.templates ?? {}
    const key = loadedTemplateFilename ?? ''
    const fmt = calibrationFormat(format)
    return templates[key]?.[fmt] ?? templates['']?.[fmt] ?? null
  }

  // Real renders record wall throughput per codec. `lastRenderFps` is left to
  // the benchmark (render-only, codec-agnostic) and legacy sessions — it
  // approximates ProRes wall speed, where hardware encode overlaps rendering.
  function recordRenderFps(format, fps) {
    if (!(fps > 0)) return
    renderFpsByFormat = {
      ...renderFpsByFormat,
      [calibrationFormat(format)]: fps,
    }
  }

  // Best throughput guess for `format`: its own recorded renders (real
  // exports or the quick test render). The render-only benchmark
  // (`lastRenderFps`) approximates ProRes wall speed, where hardware encode
  // overlaps rendering — it says nothing about qtrle's software encode, so an
  // unmeasured qtrle returns null and the export dialog offers a test render
  // instead of a made-up number.
  function renderFpsFor(format) {
    const fmt = calibrationFormat(format)
    const own = renderFpsByFormat[fmt]
    if (own > 0) return own
    // ProRes and stitched both bottleneck on frame rendering, with hardware
    // encode (ProRes / videotoolbox H.264) overlapping — so the render-only
    // benchmark and any measured ProRes throughput approximate both. qtrle's
    // software encode is unrelated, so it stays null and offers a test render.
    if (fmt === 'prores' || fmt === 'stitched') {
      if (lastRenderFps > 0) return lastRenderFps
      if (renderFpsByFormat.prores > 0) return renderFpsByFormat.prores
    }
    return null
  }

  // Which codec the quick test render is currently measuring, or null. The
  // export dialog shows progress on the matching option card.
  let calibratingFormat = $state(null)

  // Short real export — CALIBRATION_SECONDS of timeline, render + encode to a
  // throwaway file — measuring wall speed and encoded size for `format` on
  // this machine. Records both so the export dialog shows real numbers.
  async function calibrateExportEstimate(format) {
    if (renderingVideo || calibratingFormat) return
    // Stitched can't be test-rendered — it would need the source footage too.
    // Its numbers come from real exports instead.
    if (format === 'stitched') return
    if (!config?.scene || !gpxFilename || activityDuration <= 0) return
    calibratingFormat = calibrationFormat(format)
    try {
      const r = await backend.nativeCalibrateExport(
        withAppSettings(config),
        gpxFilename,
        calibratingFormat,
        CALIBRATION_SECONDS,
        outputWidth,
        outputHeight,
      )
      if (r.frames > 0 && r.elapsed_ms > 0)
        recordRenderFps(format, (r.frames / r.elapsed_ms) * 1000)
      const fps = config.scene.fps ?? 30
      const bps = exportBitsPerPixelSecond(
        r.bytes,
        outputWidth,
        outputHeight,
        fps,
        r.frames / fps,
      )
      if (bps) recordExportSizeEstimate(bps, format)
    } catch (e) {
      errorMessage = `Test render failed: ${e?.message ?? e}`
    } finally {
      calibratingFormat = null
    }
  }

  function resolvePendingDiscard(ok) {
    const run = pendingDiscard
    pendingDiscard = null
    showTemplatePicker = false
    if (ok && run) run()
  }

  // ── Selection ─────────────────────────────────────────────────────────────
  // selectedElementId is the "primary" element (drives the properties panel);
  // selectedElementIds is the full set for shift-click multi-select + group drag.

  function selectOnly(id) {
    selectedGroupId = null
    selectedCourseMarkerId = null
    selectedElementId = id
    selectedElementIds = id ? [id] : []
  }

  function setSelectedElements(ids) {
    selectedGroupId = null
    selectedCourseMarkerId = null
    selectedElementIds = [...ids]
    selectedElementId = ids.length ? ids[ids.length - 1] : null
  }

  function toggleElementSelection(id) {
    if (selectedElementIds.includes(id)) {
      selectedElementIds = selectedElementIds.filter((x) => x !== id)
      if (selectedElementId === id) {
        selectedElementId =
          selectedElementIds[selectedElementIds.length - 1] ?? null
      }
    } else {
      selectedElementIds = [...selectedElementIds, id]
      selectedElementId = id
    }
    selectedGroupId = null
    selectedCourseMarkerId = null
  }

  // ── Undo history ──────────────────────────────────────────────────────────
  // Snapshots of `config` taken just before each edit. Template load/new and
  // wholesale config replacement clear it (you can't undo across a switch).
  const HISTORY_LIMIT = 50
  let history = $state([])
  let redoStack = $state([])
  let editBatch = null

  // Apply an edit, recording the pre-edit config so it can be undone.
  // A fresh edit invalidates the redo stack.
  function commitConfig(next) {
    if (editBatch) {
      if (!editBatch.before && config) editBatch.before = JSON.stringify(config)
      config = next
      return
    }
    if (config) {
      history = [...history.slice(-(HISTORY_LIMIT - 1)), JSON.stringify(config)]
    }
    redoStack = []
    config = next
  }

  function resetHistory() {
    history = []
    redoStack = []
    editBatch = null
  }

  function beginEditBatch() {
    if (!editBatch) editBatch = { before: null }
  }

  function endEditBatch() {
    if (!editBatch) return
    const before = editBatch.before
    editBatch = null
    if (!before || !config || before === JSON.stringify(config)) return
    history = [...history.slice(-(HISTORY_LIMIT - 1)), before]
    redoStack = []
  }

  function undo() {
    endEditBatch()
    if (history.length === 0) return
    const prev = history[history.length - 1]
    history = history.slice(0, -1)
    if (config)
      redoStack = [
        ...redoStack.slice(-(HISTORY_LIMIT - 1)),
        JSON.stringify(config),
      ]
    config = JSON.parse(prev)
  }

  function redo() {
    endEditBatch()
    if (redoStack.length === 0) return
    const next = redoStack[redoStack.length - 1]
    redoStack = redoStack.slice(0, -1)
    if (config)
      history = [...history.slice(-(HISTORY_LIMIT - 1)), JSON.stringify(config)]
    config = JSON.parse(next)
  }

  // ── Config mutation helpers ───────────────────────────────────────────────

  // The config as sent to the renderer: the working template plus app-level
  // settings that live outside the template so they are never saved into or
  // shared via it. Units slot into scene.units, which the Rust side fans out
  // to every element left on Auto; rider weight rides as a separate render
  // argument (riderWeightKg).
  function withAppSettings(cfg) {
    return cfg
      ? { ...cfg, scene: { ...cfg.scene, units, compact_units: compactUnits } }
      : cfg
  }

  function updateScene(updates) {
    if (!config?.scene) return
    commitConfig({ ...config, scene: { ...config.scene, ...updates } })
  }

  // Merge into the scene's start/finish gate (scene.lap_gate), creating it on
  // first touch with the race assumed to start at the overlay window start.
  function updateLapGate(updates) {
    if (!config?.scene) return
    updateScene({
      lap_gate: {
        start: config.scene.start ?? 0,
        ...config.scene.lap_gate,
        ...updates,
      },
    })
  }

  // Find an element by stable id. Returns { idx, el } or null.
  function findElement(id, src = config) {
    const idx = src?.elements?.findIndex((e) => e.id === id) ?? -1
    return idx < 0 ? null : { idx, el: src.elements[idx] }
  }

  function updateElement(id, updates) {
    const found = findElement(id)
    if (!found) return
    const elements = [...config.elements]
    elements[found.idx] = { ...found.el, ...normalizeElementUpdates(updates) }
    commitConfig({ ...config, elements })
  }

  function updateElementPos(id, x, y) {
    updateElement(id, { x: Math.round(x), y: Math.round(y) })
  }

  // Apply position moves to a config, returning the next config (same ref if
  // nothing changed). Shared by the three drag entry points. A move is either
  // { id, x, y } for free elements or { id, anchor } for anchored ones, whose
  // position lives in the anchor offset (Rust derives x/y from the target).
  function applyMoves(base, moves) {
    if (!base?.elements || moves.length === 0) return base
    const elements = [...base.elements]
    let touched = false
    for (const m of moves) {
      const i = elements.findIndex((e) => e.id === m.id)
      if (i < 0) continue
      elements[i] = m.anchor
        ? { ...elements[i], anchor: m.anchor }
        : { ...elements[i], x: Math.round(m.x), y: Math.round(m.y) }
      touched = true
    }
    return touched ? { ...base, elements } : base
  }

  // Apply several position changes as ONE edit (one undo step) — used by
  // group drag so the whole move reverts together.
  function updateElementPositions(moves) {
    if (!config || moves.length === 0) return
    commitConfig(applyMoves(config, moves))
  }

  // Update positions live during a drag without touching undo history.
  // Call commitElementPositions on drop to persist the pre-drag snapshot.
  function moveElementPositionsLive(moves) {
    if (!config || moves.length === 0) return
    config = applyMoves(config, moves)
  }

  // Commit a drag: push the pre-drag snapshot to history so Ctrl+Z reverts
  // the whole drag in one step, then apply the final positions.
  function commitElementPositions(preDragConfigJson, moves) {
    if (!config) return
    if (preDragConfigJson) {
      history = [...history.slice(-(HISTORY_LIMIT - 1)), preDragConfigJson]
      redoStack = []
    }
    if (moves.length === 0) return
    config = applyMoves(config, moves)
  }

  // Live element update without touching undo history — use during an
  // in-progress resize/drag, then call commitElementUpdate on release.
  function updateElementLive(id, updates) {
    const found = findElement(id)
    if (!found) return
    const elements = [...config.elements]
    elements[found.idx] = { ...found.el, ...normalizeElementUpdates(updates) }
    config = { ...config, elements }
  }

  // Commit an arbitrary element update as one undo step.
  function commitElementUpdate(preConfigJson, id, updates) {
    if (!config) return
    if (preConfigJson) {
      history = [...history.slice(-(HISTORY_LIMIT - 1)), preConfigJson]
      redoStack = []
    }
    const found = findElement(id)
    if (!found) return
    const elements = [...config.elements]
    elements[found.idx] = { ...found.el, ...normalizeElementUpdates(updates) }
    config = { ...config, elements }
  }

  // Canvas-centering hook. The WYSIWYG overlay owns the measured pixel bounds
  // and author→output scale, so it registers the concrete centering fn; the
  // properties panel calls alignSelected() without knowing that geometry.
  // Null whenever no editable canvas is mounted.
  let alignHandler = null
  function setAlignHandler(fn) {
    alignHandler = fn
  }
  // Center the primary-selected element on the canvas. axis: 'h' | 'v' | 'both'.
  function alignSelected(axis) {
    if (selectedElementId) alignHandler?.(selectedElementId, axis)
  }

  // Stable, collision-free id within the config. Keeps the readable
  // `type-N` scheme (matches converted templates / Rust's opaque ids).
  function newElementId(type, elements) {
    const taken = (elements ?? []).map((e) => e.id)
    let n = 0
    let id
    do {
      id = `${type}-${n++}`
    } while (taken.includes(id))
    return id
  }

  function allElementIds(nextConfig = config) {
    return (nextConfig?.elements ?? []).map((e) => e.id)
  }

  // Element draw order is elements array order — no separate layers list needed.
  function normalizedElementLayerIds(nextConfig = config) {
    return allElementIds(nextConfig)
  }

  // Normalize a raw config (from disk or localStorage) to the canonical in-memory
  // format. Handles every legacy disk format so the rest of the app never has to.
  function migrateConfig(raw) {
    if (!raw) return raw
    let config = raw

    // 1. Legacy scene.layers → elements array order
    if (config.scene?.layers && Array.isArray(config.scene.layers)) {
      const layers = config.scene.layers
      const byId = Object.fromEntries(
        (config.elements ?? []).map((e) => [e.id, e]),
      )
      const ordered = layers.map((id) => byId[id]).filter(Boolean)
      const rest = (config.elements ?? []).filter((e) => !layers.includes(e.id))
      const sceneWithout = Object.fromEntries(
        Object.entries(config.scene).filter(([k]) => k !== 'layers'),
      )
      config = {
        ...config,
        scene: sceneWithout,
        elements: [...ordered, ...rest],
      }
    }

    // 2. Editor state: normalize into in-memory format.
    //    New format: scene.editor.{ groups, locked }
    //    Legacy format: scene.groups + element.locked
    const editorState = config.scene?.editor ?? {}
    const groups = editorState.groups ?? config.scene?.groups ?? []
    const lockedIdsList = editorState.locked ?? []

    const elements = (config.elements ?? []).map((e) => {
      // Remove bbox (dead field — never used in rendering or editor)
      const withoutBbox = Object.fromEntries(
        Object.entries(e).filter(([k]) => k !== 'bbox'),
      )
      const isLocked = e.locked === true || lockedIdsList.includes(e.id)
      const withoutLocked = Object.fromEntries(
        Object.entries(withoutBbox).filter(([k]) => k !== 'locked'),
      )
      const base = isLocked ? { ...withoutLocked, locked: true } : withoutLocked
      // Migrate legacy points: [{...}] → point: {...} for plot elements.
      // Only the first entry is ever used; the array form was pure nesting noise.
      if (
        base.type === 'plot' &&
        Array.isArray(base.points) &&
        base.points.length > 0
      ) {
        // eslint-disable-next-line no-unused-vars
        const { points: _pts, ...rest } = base
        return { ...rest, point: base.point ?? base.points[0] }
      }
      return base
    })

    // units and compact_units moved to app-level preferences (see `units` /
    // `compactUnits` state) — strip the scene fields so they never ride in
    // saved templates again.
    const sceneBase = Object.fromEntries(
      Object.entries(config.scene ?? {}).filter(
        ([k]) =>
          k !== 'editor' &&
          k !== 'groups' &&
          k !== 'units' &&
          k !== 'compact_units',
      ),
    )
    return { ...config, scene: { ...sceneBase, groups }, elements }
  }

  // Prepare a config for writing to disk: move editor-only state (locked IDs,
  // groups) into scene.editor and strip it from elements / scene root.
  function toEditorFormat(config) {
    if (!config) return config
    const lockedIds = (config.elements ?? [])
      .filter((e) => e.locked === true)
      .map((e) => e.id)
    const groups = config.scene?.groups ?? []

    const editor = {}
    if (groups.length > 0) editor.groups = groups
    if (lockedIds.length > 0) editor.locked = lockedIds

    const elements = (config.elements ?? []).map((e) =>
      Object.fromEntries(Object.entries(e).filter(([k]) => k !== 'locked')),
    )
    const sceneBase = Object.fromEntries(
      Object.entries(config.scene ?? {}).filter(([k]) => k !== 'groups'),
    )
    const scene =
      Object.keys(editor).length > 0
        ? { ...sceneBase, editor }
        : { ...sceneBase }

    return { ...config, scene, elements }
  }

  function addElement(type, defaults) {
    if (!config) return null
    const id = newElementId(type, config.elements ?? [])
    const el = { type, id, ...defaults }
    commitConfig({ ...config, elements: [...(config.elements ?? []), el] })
    return id
  }

  // Deleting an anchor target would leave its dependents dangling (Rust falls
  // back to their stale authored x/y) — bake the resolved position into x/y
  // and drop the anchor instead, so survivors stay where they were drawn.
  const ANCHOR_POINT_FRACS = {
    'top-left': [0, 0],
    top: [0.5, 0],
    'top-right': [1, 0],
    left: [0, 0.5],
    center: [0.5, 0.5],
    right: [1, 0.5],
    'bottom-left': [0, 1],
    bottom: [0.5, 1],
    'bottom-right': [1, 1],
  }
  function detachAnchorsToRemoved(elements, removedIds) {
    const all = config?.elements ?? []
    return elements.map((el) => {
      const a = el.anchor
      if (!a?.target || !removedIds.includes(a.target)) return el
      const t = all.find((e) => e.id === a.target)
      const rest = { ...el }
      delete rest.anchor
      if (!t || t.width == null || t.height == null) return rest
      const [fx, fy] = ANCHOR_POINT_FRACS[a.point ?? 'center'] ?? [0.5, 0.5]
      let x = (t.x ?? 0) + t.width * fx + (a.offset_x ?? 0)
      let y = (t.y ?? 0) + t.height * fy + (a.offset_y ?? 0)
      if (el.type === 'rect' || el.type === 'image') {
        const [sfx, sfy] = ANCHOR_POINT_FRACS[a.self_point ?? 'center'] ?? [
          0.5, 0.5,
        ]
        x -= (el.width ?? 0) * sfx
        y -= (el.height ?? 0) * sfy
      }
      return { ...rest, x: Math.round(x), y: Math.round(y) }
    })
  }

  function removeElement(id) {
    removeElements([id])
  }

  function removeElements(ids) {
    if (!config?.elements || ids.length === 0) return
    let elements = config.elements.filter((e) => !ids.includes(e.id))
    if (elements.length === config.elements.length) return
    elements = detachAnchorsToRemoved(elements, ids)

    const groups = (config.scene?.groups ?? []).map((g) => ({
      ...g,
      element_ids: g.element_ids.filter((eid) => !ids.includes(eid)),
    }))
    commitConfig({ ...config, elements, scene: { ...config.scene, groups } })

    if (selectedElementIds.some((id) => ids.includes(id))) {
      selectOnly(null)
    }
  }

  function moveElementLayer(id, delta) {
    if (!config?.elements) return
    const elements = [...config.elements]
    const from = elements.findIndex((e) => e.id === id)
    if (from < 0) return
    const to = Math.max(0, Math.min(elements.length - 1, from + delta))
    if (to === from) return
    const [moved] = elements.splice(from, 1)
    elements.splice(to, 0, moved)
    commitConfig({ ...config, elements })
  }

  function setElementLayerOrder(ids) {
    if (!config?.elements) return
    const elements = config.elements
    if (ids.length !== elements.length) return
    if (!ids.every((id) => elements.some((e) => e.id === id))) return
    const byId = Object.fromEntries(elements.map((e) => [e.id, e]))
    commitConfig({ ...config, elements: ids.map((id) => byId[id]) })
  }

  // ── Group management ──────────────────────────────────────────────────────

  function selectGroup(groupId) {
    const group = (config?.scene?.groups ?? []).find((g) => g.id === groupId)
    if (!group) return
    selectedGroupId = groupId
    selectedCourseMarkerId = null
    selectedElementIds = [...group.element_ids]
    selectedElementId = group.element_ids[0] ?? null
  }

  function newGroupId() {
    const existing = (config?.scene?.groups ?? []).map((g) => g.id)
    let n = 0
    let id
    do {
      id = `group-${n++}`
    } while (existing.includes(id))
    return id
  }

  function createGroup(name, elementIds) {
    if (!config?.scene) return null
    const id = newGroupId()
    const groups = (config.scene.groups ?? [])
      .map((g) => ({
        ...g,
        element_ids: g.element_ids.filter((eid) => !elementIds.includes(eid)),
      }))
      .concat({ id, name, element_ids: [...elementIds] })
    commitConfig({ ...config, scene: { ...config.scene, groups } })
    return id
  }

  function deleteGroup(groupId) {
    if (!config?.scene) return
    const groups = (config.scene.groups ?? []).filter((g) => g.id !== groupId)
    commitConfig({ ...config, scene: { ...config.scene, groups } })
    if (selectedGroupId === groupId) selectOnly(null)
  }

  function reorderGroupElements(groupId, newFrontToBack) {
    if (!config?.scene?.groups) return
    const groups = config.scene.groups.map((g) =>
      g.id === groupId
        ? { ...g, element_ids: [...newFrontToBack].reverse() }
        : g,
    )
    commitConfig({ ...config, scene: { ...config.scene, groups } })
  }

  function renameGroup(groupId, name) {
    if (!config?.scene?.groups) return
    const groups = config.scene.groups.map((g) =>
      g.id === groupId ? { ...g, name } : g,
    )
    commitConfig({ ...config, scene: { ...config.scene, groups } })
  }

  function removeElementFromGroups(elementId) {
    if (!config?.scene?.groups) return
    const groups = config.scene.groups.map((g) => ({
      ...g,
      element_ids: g.element_ids.filter((id) => id !== elementId),
    }))
    commitConfig({ ...config, scene: { ...config.scene, groups } })
  }

  function removeFromGroupAndReorder(elementId, newLayerOrder) {
    if (!config?.elements) return
    const groups = (config.scene?.groups ?? []).map((g) => ({
      ...g,
      element_ids: g.element_ids.filter((id) => id !== elementId),
    }))
    const elements = config.elements
    if (
      newLayerOrder.length !== elements.length ||
      !newLayerOrder.every((id) => elements.some((e) => e.id === id))
    )
      return
    const byId = Object.fromEntries(elements.map((e) => [e.id, e]))
    commitConfig({
      ...config,
      elements: newLayerOrder.map((id) => byId[id]),
      scene: { ...config.scene, groups },
    })
  }

  function addElementToGroup(elementId, groupId) {
    if (!config?.scene?.groups) return
    const groups = config.scene.groups.map((g) => {
      if (g.id === groupId) {
        if (g.element_ids.includes(elementId)) return g
        return { ...g, element_ids: [...g.element_ids, elementId] }
      }
      return {
        ...g,
        element_ids: g.element_ids.filter((id) => id !== elementId),
      }
    })
    commitConfig({ ...config, scene: { ...config.scene, groups } })
  }

  function parseSelectedElement() {
    if (!selectedElementId || !config) return null
    const found = findElement(selectedElementId)
    return found
      ? { id: found.el.id, item: found.el, type: found.el.type }
      : null
  }

  function selectedElementLabel() {
    const s = parseSelectedElement()
    if (!s) return null
    const name = elementTypeName(s.item)
    const text = s.item.text || s.item.value || ''
    return text ? `${name} "${text}"` : name
  }

  function deleteSelectedElement() {
    const ids = selectedElementIds.length
      ? selectedElementIds
      : selectedElementId
        ? [selectedElementId]
        : []
    removeElements(ids)
  }

  function copyElement() {
    const s = parseSelectedElement()
    if (!s) return
    copiedElement = s.item
  }

  function pasteElement() {
    if (!copiedElement) return
    const rest = { ...copiedElement }
    const type = rest.type
    delete rest.id
    delete rest.type
    addElement(type, {
      ...rest,
      x: (rest.x ?? 0) + 20,
      y: (rest.y ?? 0) + 20,
    })
  }

  // ── Template actions ─────────────────────────────────────────────────────

  function toFilename(raw) {
    const stem = raw
      .trim()
      .toLowerCase()
      .replace(/\.json$/, '')
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '')
    return stem ? `${stem}.json` : null
  }

  function templateDisplayName(filename) {
    return filename
      .replace(/\.json$/, '')
      .replace(/_/g, ' ')
      .replace(/\b\w/g, (c) => c.toUpperCase())
  }

  function blankTemplate(name) {
    return {
      scene: {
        width: outputWidth,
        height: outputHeight,
        fps: 30,
        start: 0,
        end: Math.max(1, Math.floor(activityDuration || 1)),
        color: '#ffffff',
        opacity: 1,
        font_size: 64,
        overlay_filename: name.replace(/\.json$/, ''),
      },
      elements: [],
    }
  }

  function showSuccess(msg) {
    successMessage = msg
    clearTimeout(successTimer)
    successTimer = setTimeout(() => {
      successMessage = null
    }, 2500)
  }

  async function saveTemplate() {
    if (!config) return
    let filename = loadedTemplateFilename
    const tpl = templates.find((t) => t.id === filename)
    if (!filename || tpl?.type === 'built-in') {
      const name = prompt(
        'Template name:',
        filename?.replace('.json', '') ?? 'my_overlay',
      )
      if (!name) return
      filename = toFilename(name)
      if (!filename) return
    }
    await backend.saveTemplate(filename, stripDefaults(toEditorFormat(config)))
    loadedTemplateFilename = filename
    markPristine()
    if (currentPreviewImage) {
      backend.saveTemplatePreview(filename, currentPreviewImage).catch(() => {})
    }
    await fetchTemplates()
    showSuccess(`Saved "${filename}"`)
  }

  async function saveTemplateAs() {
    if (!config) return
    const name = prompt(
      'Save as:',
      loadedTemplateFilename?.replace('.json', '') ?? 'my_overlay',
    )
    if (!name) return
    const filename = toFilename(name)
    if (!filename) return
    await backend.saveTemplate(filename, stripDefaults(toEditorFormat(config)))
    loadedTemplateFilename = filename
    markPristine()
    if (currentPreviewImage) {
      backend.saveTemplatePreview(filename, currentPreviewImage).catch(() => {})
    }
    await fetchTemplates()
    showSuccess(`Saved "${filename}"`)
  }

  async function newTemplate(name) {
    if (!name) return
    const filename = toFilename(name)
    if (!filename) return
    const base = blankTemplate(filename)
    await backend.saveTemplate(filename, stripDefaults(toEditorFormat(base)))
    config = base
    loadedTemplateFilename = filename
    selectOnly(null)
    resetHistory()
    markPristine()
    await fetchTemplates()
    showSuccess(`Created "${templateDisplayName(filename)}"`)
  }

  // Generate a template from a text prompt (or edit the current one). The
  // result loads into the editor as an unsaved draft — the user saves it
  // normally afterward. `edit: true` sends the current template so the model
  // modifies it in place.
  async function generateTemplate(prompt, { edit = false } = {}) {
    const trimmed = (prompt ?? '').trim()
    if (!trimmed) return
    const current =
      edit && config ? stripDefaults(toEditorFormat(config)) : null
    const result = await backend.generateTemplate(trimmed, current)
    const next = migrateConfig(result)
    // start/end are activity-specific timeline bounds, not template config —
    // preserve the user's current window (same as loadTemplate).
    if (next?.scene) {
      const currentStart = config?.scene?.start ?? 0
      const currentEnd = config?.scene?.end ?? activityDuration
      next.scene = {
        ...next.scene,
        start: currentStart,
        end: currentEnd > currentStart ? currentEnd : activityDuration,
      }
    }
    if (edit && config && loadedTemplateFilename) {
      // Undoable edit of the loaded template; leaves it modified for save.
      commitConfig(next)
    } else {
      // Fresh draft — unsaved until the user names and saves it.
      config = next
      loadedTemplateFilename = null
      selectOnly(null)
      resetHistory()
      pristineConfig = null
    }
    showSuccess(edit ? 'Template updated' : 'Template generated')
  }

  async function fetchTemplates() {
    try {
      templates = await backend.listTemplates()
    } catch (err) {
      console.error('Failed to fetch templates:', err)
    }
  }

  async function fetchFonts() {
    try {
      fonts = await backend.listFonts()
    } catch (err) {
      console.error('Failed to fetch fonts:', err)
    }
  }

  // Pick a .ttf/.otf, copy it into the user fonts dir, refresh the list, and
  // return the new font's filename so the caller can select it.
  async function addCustomFont() {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: 'Fonts',
          extensions: dialogExtensions(['ttf', 'otf', 'woff', 'woff2']),
        },
      ],
    })
    if (!selected) return null
    try {
      fonts = await backend.importFont(selected)
      const filename = selected.split(/[\\/]/).pop()
      updateScene({ font: filename })
      return filename
    } catch (err) {
      errorMessage = `Could not add font: ${err?.message ?? err}`
      return null
    }
  }

  async function runBenchmark() {
    if (
      renderingVideo ||
      benchmarking ||
      !config ||
      !gpxFilename ||
      activityDuration <= 0
    )
      return
    benchmarking = true
    try {
      const result = await backend.nativeBenchmark(
        withAppSettings(config),
        gpxFilename,
        90,
        outputWidth,
        outputHeight,
      )
      if (result.frames > 0 && result.elapsed_ms > 0) {
        const fps = (result.frames / result.elapsed_ms) * 1000
        lastRenderFps = fps
        localStorage.setItem('lastRenderFps', fps.toFixed(4))
      }
    } catch (e) {
      console.debug('Benchmark failed:', e)
    } finally {
      benchmarking = false
    }
  }

  // Re-benchmark whenever the template or GPX changes (debounced so rapid
  // config edits don't flood the Rust thread pool).
  $effect(() => {
    void loadedTemplateFilename
    void gpxFilename
    void outputWidth
    void outputHeight
    if (!config || !gpxFilename || activityDuration <= 0) return
    const timer = setTimeout(runBenchmark, 800)
    return () => clearTimeout(timer)
  })

  // Retarget the output resolution to a template's authored aspect ratio.
  // The renderer scales templates by height and keeps whatever output shape is
  // set globally, so a vertical template loaded into a 16:9 output renders with
  // everything crammed to one side. When a template's aspect differs from the
  // current output aspect, adopt the template's aspect while preserving the
  // resolution tier (long edge stays put). Matching aspects are left alone so a
  // user's specific resolution choice within an aspect is respected.
  function syncOutputToTemplateAspect(scene) {
    const w = scene?.width
    const h = scene?.height
    if (!w || !h) return
    const templateAspect = w / h
    const currentAspect = outputHeight ? outputWidth / outputHeight : 1
    if (Math.abs(templateAspect - currentAspect) < 0.01) return
    const longEdge = Math.max(outputWidth, outputHeight)
    let nw
    let nh
    if (templateAspect >= 1) {
      nw = longEdge
      nh = Math.round(longEdge / templateAspect)
    } else {
      nh = longEdge
      nw = Math.round(longEdge * templateAspect)
    }
    // Encoders require even dimensions.
    outputWidth = nw - (nw % 2)
    outputHeight = nh - (nh % 2)
  }

  async function loadTemplate(filename) {
    const data = await backend.getTemplate(filename)
    const loaded = migrateConfig(data)
    // start/end are activity-specific timeline bounds, not template config.
    // Preserve the user's current timeline window when switching templates.
    if (loaded?.scene) {
      const currentStart = config?.scene?.start ?? 0
      const currentEnd = config?.scene?.end ?? activityDuration
      loaded.scene = {
        ...loaded.scene,
        start: currentStart,
        end: currentEnd > currentStart ? currentEnd : activityDuration,
      }
      syncOutputToTemplateAspect(loaded.scene)
    }
    config = loaded
    loadedTemplateFilename = filename
    selectOnly(null)
    resetHistory()
    markPristine()
  }

  // Reload the current template from disk, discarding any unsaved changes.
  async function revertTemplate() {
    if (!loadedTemplateFilename) return
    await loadTemplate(loadedTemplateFilename)
  }

  function clearTemplate() {
    config = null
    loadedTemplateFilename = null
    pristineConfig = null
    localStorage.removeItem('loadedTemplateFilename')
    localStorage.removeItem('editorConfig')
    selectOnly(null)
    resetHistory()
  }

  return {
    get config() {
      return config
    },
    set config(v) {
      config = migrateConfig(v)
      resetHistory()
    },
    get gpxFilename() {
      return gpxFilename
    },
    set gpxFilename(v) {
      gpxFilename = v
    },
    get gpxStartTime() {
      return gpxStartTime
    },
    set gpxStartTime(v) {
      gpxStartTime = v
    },
    get video() {
      return video
    },
    get videoUnderlayVisible() {
      return videoUnderlayVisible
    },
    setVideoUnderlayVisible,
    loadVideo,
    pickAndLoadVideo,
    clearVideo,
    setVideoOffset,
    verifyVideo,
    get activityDuration() {
      return activityDuration
    },
    set activityDuration(v) {
      activityDuration = v
    },
    get activityMetrics() {
      return activityMetrics
    },
    set activityMetrics(v) {
      activityMetrics = v
    },
    get hasActivity() {
      return !!gpxFilename && activityDuration > 0
    },
    get timelineDuration() {
      return activityDuration
    },
    get selectedSecond() {
      return selectedSecond
    },
    set selectedSecond(v) {
      selectedSecond = v
    },
    get isTemplateModified() {
      return templateModified()
    },
    get pendingDiscard() {
      return pendingDiscard
    },
    confirmIfModified,
    resolvePendingDiscard,
    fetchDefaultOutputDir,
    get loadedTemplateFilename() {
      return loadedTemplateFilename
    },
    set loadedTemplateFilename(v) {
      loadedTemplateFilename = v
    },
    get outputDir() {
      return outputDir
    },
    set outputDir(v) {
      outputDir = v
    },
    get defaultOutputDir() {
      return defaultOutputDir
    },
    get effectiveOutputDir() {
      return outputDir ?? defaultOutputDir ?? ''
    },
    pickOutputDir,
    resetOutputDir,
    // Rider weight (local-only; feeds the W/kg metric, never saved to templates).
    get units() {
      return units
    },
    set units(v) {
      units = v === 'imperial' ? 'imperial' : 'metric'
    },
    // Compact compound-unit style ("km/h" vs "kmh"); app-level, injected into
    // scene.compact_units, never saved to templates.
    get compactUnits() {
      return compactUnits
    },
    set compactUnits(v) {
      compactUnits = v === true || v === 'true' || v === 'compact'
    },
    // Config with app-level settings (unit system) injected — what every
    // render/preview call sends; template saves keep using `config`.
    get renderConfig() {
      return withAppSettings(config)
    },
    get riderWeight() {
      return riderWeight
    },
    set riderWeight(v) {
      riderWeight =
        v == null || v === '' || !Number.isFinite(Number(v)) ? null : Number(v)
    },
    get riderWeightUnit() {
      return riderWeightUnit
    },
    set riderWeightUnit(v) {
      riderWeightUnit = v === 'lb' ? 'lb' : 'kg'
    },
    // Weight resolved to kilograms for the renderer, or null when unset.
    get riderWeightKg() {
      return resolvedRiderWeightKg()
    },
    // True when the template renders W/kg anywhere — where the weight prompts
    // belong, whether or not a weight is currently set.
    get usesRiderWeight() {
      return templateUsesRiderWeight()
    },
    // True when the template renders W/kg but no weight is set — every such
    // readout comes out 0.0 until the user enters one, so callers prompt.
    get needsRiderWeight() {
      return resolvedRiderWeightKg() == null && templateUsesRiderWeight()
    },
    get outputWidth() {
      return outputWidth
    },
    set outputWidth(v) {
      outputWidth = v
    },
    get outputHeight() {
      return outputHeight
    },
    set outputHeight(v) {
      outputHeight = v
    },
    get exportFormat() {
      return exportFormat
    },
    set exportFormat(v) {
      exportFormat = v
    },
    get exportFullFrame() {
      return exportFullFrame
    },
    set exportFullFrame(v) {
      exportFullFrame = v
    },
    get previewFps() {
      return previewFps
    },
    set previewFps(v) {
      previewFps = v
    },
    get currentPreviewImage() {
      return currentPreviewImage
    },
    set currentPreviewImage(v) {
      currentPreviewImage = v
    },
    get renderingVideo() {
      return renderingVideo
    },
    set renderingVideo(v) {
      renderingVideo = v
    },
    get errorMessage() {
      return errorMessage
    },
    set errorMessage(v) {
      errorMessage = v
    },
    get successMessage() {
      return successMessage
    },
    get templates() {
      return templates
    },
    set templates(v) {
      templates = v
    },
    get fonts() {
      return fonts
    },
    fetchFonts,
    addCustomFont,
    get showTemplatePicker() {
      return showTemplatePicker
    },
    set showTemplatePicker(v) {
      showTemplatePicker = v
    },
    get selectedElementId() {
      return selectedElementId
    },
    set selectedElementId(v) {
      selectOnly(v)
    },
    get selectedElementIds() {
      return selectedElementIds
    },
    toggleElementSelection,
    setSelectedElements,
    get selectedGroupId() {
      return selectedGroupId
    },
    get selectedCourseMarkerId() {
      return selectedCourseMarkerId
    },
    set selectedCourseMarkerId(v) {
      selectedCourseMarkerId = v
    },
    selectGroup,
    createGroup,
    deleteGroup,
    renameGroup,
    reorderGroupElements,
    removeElementFromGroups,
    removeFromGroupAndReorder,
    addElementToGroup,
    get canUndo() {
      return history.length > 0 || !!editBatch?.before
    },
    get canRedo() {
      return redoStack.length > 0
    },
    beginEditBatch,
    endEditBatch,
    undo,
    redo,
    get elementLayerOrder() {
      return normalizedElementLayerIds()
    },
    moveElementLayer,
    setElementLayerOrder,
    get renderProgress() {
      return renderProgress
    },
    set renderProgress(v) {
      renderProgress = v
    },
    clearError() {
      errorMessage = null
    },
    updateScene,
    updateLapGate,
    updateElement,
    updateElementPos,
    updateElementPositions,
    moveElementPositionsLive,
    commitElementPositions,
    updateElementLive,
    commitElementUpdate,
    setAlignHandler,
    alignSelected,
    addElement,
    removeElement,
    deleteSelectedElement,
    get copiedElement() {
      return copiedElement
    },
    copyElement,
    pasteElement,
    get lastRenderFps() {
      return lastRenderFps
    },
    set lastRenderFps(v) {
      lastRenderFps = v
    },
    exportSizeCalibrationFor: currentExportSizeCalibration,
    recordExportSizeEstimate,
    recordRenderFps,
    renderFpsFor,
    get calibratingFormat() {
      return calibratingFormat
    },
    calibrateExportEstimate,
    get benchmarking() {
      return benchmarking
    },
    selectedElementLabel,
    fetchTemplates,
    loadTemplate,
    clearTemplate,
    saveTemplate,
    saveTemplateAs,
    newTemplate,
    generateTemplate,
    revertTemplate,
  }
}
