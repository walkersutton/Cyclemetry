<script>
  import { setContext, onMount } from 'svelte'
  import { listen } from '@tauri-apps/api/event'
  import { windowDrag } from './lib/windowDrag.js'
  import { createAppState } from './state/appState.svelte.js'
  import * as backend from './api/backend.js'
  import loadGpx from './api/gpxUtils.js'
  import renderVideo from './api/renderVideo.js'
  import { AI_ASSISTANT_ENABLED } from './lib/featureFlags.js'

  import LeftSidebar from './components/layout/LeftSidebar.svelte'
  import CenterCanvas from './components/layout/CenterCanvas.svelte'
  import RightPanel from './components/layout/RightPanel.svelte'
  import RenderProgressOverlay from './components/overlays/RenderProgressOverlay.svelte'
  import RenderStatusChip from './components/overlays/RenderStatusChip.svelte'
  import ErrorToast from './components/overlays/ErrorToast.svelte'
  import UpdateBanner from './components/overlays/UpdateBanner.svelte'
  import Settings from './components/overlays/Settings.svelte'
  import AboutModal from './components/overlays/AboutModal.svelte'
  import TemplatePickerModal from './components/overlays/TemplatePickerModal.svelte'
  import ActivityPickerModal from './components/overlays/ActivityPickerModal.svelte'
  import ConfirmDialog from './components/overlays/ConfirmDialog.svelte'
  import NewTemplateDialog from './components/overlays/NewTemplateDialog.svelte'
  import ExportFormatDialog from './components/overlays/ExportFormatDialog.svelte'
  import Button from './components/ui/Button.svelte'
  import Tooltip from './components/ui/Tooltip.svelte'

  import {
    Activity,
    AlertTriangle,
    ChevronDown,
    Clock,
    Eye,
    EyeOff,
    Film,
    LayoutGrid,
    Monitor,
    Play,
    RotateCcw,
    Save,
    Sparkles,
    X,
  } from 'lucide-svelte'
  import {
    formatTime,
    formatFileSize,
    estimateExportBytes,
    DEFAULT_BITS_PER_PIXEL_SECOND,
    DEFAULT_STITCHED_BITS_PER_PIXEL_SECOND,
    TOOLTIP_DELAY,
  } from './lib/utils.js'
  import { wallClockDeltaSec } from './lib/videoAlignment.js'

  // ── State ──────────────────────────────────────────────────────────────────
  const app = createAppState()
  setContext('app', app)
  const isDev = import.meta.env.DEV

  async function overwriteRepoTemplate() {
    try {
      await backend.overwriteCommunityTemplate(app.loadedTemplateFilename)
      await app.fetchTemplates()
    } catch (e) {
      app.errorMessage = e?.message ?? String(e)
    }
  }

  // ── Scene toolbar helpers ──────────────────────────────────────────────────
  const RES_PRESETS = [
    { label: '4K', w: 3840, h: 2160 },
    { label: '1080p', w: 1920, h: 1080 },
    { label: '4K Vertical', w: 2160, h: 3840 },
    { label: '1080p Vertical', w: 1080, h: 1920 },
    { label: 'Square', w: 1080, h: 1080 },
  ]

  // Export options: two transparent-overlay codecs (both .mov — ProRes 4444
  // is smaller and hardware-accelerated but consumer editors like Premiere
  // Elements on Windows can't decode it; Animation/qtrle is the compatible
  // fallback), plus a stitched export that burns the overlay onto the
  // reference video for a finished, shareable MP4. The stitched option only
  // appears once a video is loaded — with no footage there's nothing to burn
  // onto, so offering it would just dead-end into a file picker.
  let hasVideoLoaded = $derived(!!app.video?.path && !app.video.missing)
  let EXPORT_FORMATS = $derived([
    {
      value: 'prores',
      label: 'ProRes 4444',
      container: '.mov',
      transparent: true,
      bestFor: 'Pro editors',
      summary: 'Transparent overlay for pro editors.',
      desc: 'Overlay only, on a transparent background — layer it onto your own edit. Smaller files, but needs a pro editor: Premiere Pro, Final Cut, or DaVinci Resolve.',
    },
    {
      value: 'qtrle',
      label: 'Animation (RLE)',
      container: '.mov',
      transparent: true,
      bestFor: 'Any editor',
      summary: 'Transparent overlay that plays in any editor.',
      desc: 'Overlay only, on a transparent background — plays in any editor, including ones without ProRes support like Premiere Elements on Windows. Slightly larger files.',
    },
    ...(hasVideoLoaded
      ? [
          {
            value: 'stitched',
            label: 'Video with overlay',
            container: '.mp4',
            transparent: false,
            bestFor: 'Sharing',
            summary: 'Overlay burned onto your video — ready to share.',
            desc: `Burns the overlay directly onto ${videoBasename(app.video.path)} — a finished video, ready to share. No editor needed.`,
          },
        ]
      : []),
  ])

  // True when the user is in custom-resolution mode. Initialised to true when
  // the persisted dims don't match any preset (e.g. loaded from a prior session).
  let customResActive = $state(
    !RES_PRESETS.some(
      (p) => p.w === app.outputWidth && p.h === app.outputHeight,
    ),
  )
  let showResolutionMenu = $state(false)

  let resolutionLabel = $derived.by(() => {
    const preset = RES_PRESETS.find(
      (p) => p.w === app.outputWidth && p.h === app.outputHeight,
    )
    return preset ? preset.label : `${app.outputWidth}×${app.outputHeight}`
  })

  function videoBasename(path) {
    if (!path) return ''
    return path.split(/[\\/]/).pop()
  }

  function humanGap(sec) {
    const s = Math.round(Math.abs(sec))
    if (s < 120) return `${s}s`
    if (s < 7200) return `${Math.round(s / 60)}m`
    const h = Math.floor(s / 3600)
    const m = Math.round((s % 3600) / 60)
    return m > 0 ? `${h}h ${m}m` : `${h}h`
  }

  // Never fail silently: when the camera's recorded timestamp puts the video
  // entirely outside the activity, explain the mismatch instead of moving the
  // band somewhere useless. Common causes: re-encoded/stitched exports carry
  // the export time, or the camera clock/timezone is wrong.
  function moveVideoToRecordingTime() {
    const delta = wallClockDeltaSec(app.gpxStartTime, app.video)
    if (delta == null) return
    const videoEnd = delta + (app.video?.duration ?? 0)
    if (videoEnd <= 0 || delta >= app.timelineDuration) {
      const gap = videoEnd <= 0 ? videoEnd : delta - app.timelineDuration
      app.errorMessage =
        `Can't align: the video's recorded timestamp (${app.video.creationTime}) is ` +
        `${humanGap(gap)} ${videoEnd <= 0 ? 'before the activity starts' : 'after the activity ends'}. ` +
        `The file's creation time is probably not the recording time — stitched or re-encoded exports ` +
        `carry the export time, and a wrong camera clock or timezone shifts it. Drag the video band to align manually.`
      return
    }
    app.setVideoOffset(0)
  }

  function errorText(err, fallback = 'Unknown error') {
    if (typeof err === 'string') return err
    return err?.message ?? String(err ?? fallback)
  }

  function chooseResolution(preset) {
    app.outputWidth = preset.w
    app.outputHeight = preset.h
    customResActive = false
    showResolutionMenu = false
  }

  // Offer the align button whenever both timestamps exist — even when the
  // wall-clock placement would land outside the activity, so pressing it can
  // explain the metadata problem instead of the button silently vanishing.
  let canUseRecordingTime = $derived(
    app.hasActivity && wallClockDeltaSec(app.gpxStartTime, app.video) != null,
  )

  // ── Template toolbar helpers ───────────────────────────────────────────────
  let templateLabel = $derived.by(() => {
    if (!app.loadedTemplateFilename) return null
    const t = (app.templates ?? []).find(
      (t) => t.id === app.loadedTemplateFilename,
    )
    return t?.name ?? app.loadedTemplateFilename.replace('.json', '')
  })

  let rendering = $state(false)
  let showSettings = $state(false)
  let showAbout = $state(false)
  let showNewTemplateDialog = $state(false)
  let showAiChat = $state(false)
  let showActivityPicker = $state(false)

  // Auto-switch to properties when user clicks an element while AI chat is open.
  $effect(() => {
    if ((app.selectedElementId || app.selectedGroupId) && showAiChat) {
      showAiChat = false
    }
  })

  function closeDialogs() {
    showActivityPicker = false
    showSettings = false
    showAbout = false
    showNewTemplateDialog = false
    showAiChat = false
    app.showTemplatePicker = false
  }

  function openTemplatePicker() {
    closeDialogs()
    app.showTemplatePicker = true
  }

  function openActivityPicker() {
    closeDialogs()
    showActivityPicker = true
  }

  function openSettings() {
    closeDialogs()
    showSettings = true
  }

  function openAbout() {
    closeDialogs()
    showAbout = true
  }

  function openNewTemplateDialog() {
    closeDialogs()
    showNewTemplateDialog = true
  }

  function toggleAiChat() {
    if (showAiChat) {
      showAiChat = false
    } else {
      closeDialogs()
      showAiChat = true
    }
  }

  // Enforce mutual exclusion: any code path that sets showTemplatePicker = true
  // (including child components that bypass openTemplatePicker) must close other dialogs.
  $effect(() => {
    if (app.showTemplatePicker) {
      showActivityPicker = false
      showSettings = false
      showAbout = false
      showNewTemplateDialog = false
      showAiChat = false
    }
  })

  let showRevertConfirm = $state(false)

  const REVERT_SKIP_KEY = 'confirm_skip_revert_template'
  const RENDERED_ONCE_KEY = 'has_rendered_once'
  let hasRenderedOnce = $state(
    localStorage.getItem(RENDERED_ONCE_KEY) === 'true',
  )

  function handleRevertClick() {
    if (localStorage.getItem(REVERT_SKIP_KEY) === 'true') {
      app.revertTemplate().catch((e) => {
        app.errorMessage = e?.message ?? String(e)
      })
    } else {
      showRevertConfirm = true
    }
  }
  function onWindowKeydown(e) {
    const t = e.target
    const inField =
      t?.tagName === 'INPUT' ||
      t?.tagName === 'TEXTAREA' ||
      t?.tagName === 'SELECT' ||
      t?.isContentEditable
    const blocked =
      showSettings ||
      app.showTemplatePicker ||
      showNewTemplateDialog ||
      showActivityPicker ||
      showExportFormatDialog

    if (e.key === 'Escape' && showResolutionMenu) {
      showResolutionMenu = false
      return
    }

    // Escape minimizes the render dialog back to the header status chip
    // (the render keeps running).
    if (e.key === 'Escape' && app.renderingVideo && renderExpanded) {
      renderExpanded = false
      return
    }

    // Undo (⌘/Ctrl+Z). Skip when typing in a field so native text undo works.
    if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey &&
      (e.key === 'z' || e.key === 'Z')
    ) {
      if (inField || blocked || !app.canUndo) return
      e.preventDefault()
      app.undo()
      return
    }

    // Redo (⌘/Ctrl+Shift+Z or Ctrl+Y). Same in-field skip as undo.
    if (
      (e.metaKey || e.ctrlKey) &&
      ((e.shiftKey && (e.key === 'z' || e.key === 'Z')) ||
        e.key === 'y' ||
        e.key === 'Y')
    ) {
      if (inField || blocked || !app.canRedo) return
      e.preventDefault()
      app.redo()
      return
    }

    // Copy element (⌘/Ctrl+C). Only when an element is selected and not in a text field.
    if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey &&
      (e.key === 'c' || e.key === 'C')
    ) {
      if (inField || blocked || !app.selectedElementId) return
      e.preventDefault()
      app.copyElement()
      return
    }

    // Paste element clone (⌘/Ctrl+V).
    if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey &&
      (e.key === 'v' || e.key === 'V')
    ) {
      if (inField || blocked || !app.copiedElement) return
      e.preventDefault()
      app.pasteElement()
      return
    }

    if (e.key !== 'Delete' && e.key !== 'Backspace') return
    if (blocked || inField) return
    if (!app.selectedElementId) return
    e.preventDefault()
    app.deleteSelectedElement()
  }

  onMount(() => {
    app.fetchTemplates()
    app.fetchFonts()
    app.fetchDefaultOutputDir()
    app.verifyVideo()
    if (typeof window.__TAURI__ !== 'undefined') {
      const unlisteners = [
        listen('menu_open_gpx', () => handleOpenGpx()),
        listen('menu_open_recent_gpx', (e) => handleOpenRecentGpx(e.payload)),
        listen('menu_save_template', () =>
          app.saveTemplate().catch((e) => {
            app.errorMessage = e.message
          }),
        ),
        listen('menu_new_template', () =>
          app.confirmIfModified(() => {
            openNewTemplateDialog()
          }),
        ),
        listen('menu_show_downloads', () => handleOpenDownloads()),
        listen('menu_show_activities', () =>
          backend.openActivitiesFolder().catch(() => {}),
        ),
        listen('menu_open_templates_folder', () =>
          backend.openTemplatesFolder().catch(() => {}),
        ),
        listen('menu_settings', () => {
          openSettings()
        }),
        listen('menu_about', () => {
          openAbout()
        }),
        listen('menu_undo', () => {
          if (app.canUndo) app.undo()
        }),
        listen('menu_redo', () => {
          if (app.canRedo) app.redo()
        }),
        listen('menu_copy', () => {
          if (app.selectedElementId) app.copyElement()
        }),
        listen('menu_paste', () => {
          if (app.copiedElement) app.pasteElement()
        }),
        listen('menu_show_template_dialog', () => {
          openTemplatePicker()
        }),
        listen('menu_add_custom_font', () =>
          app.addCustomFont().catch((e) => {
            app.errorMessage = e.message
          }),
        ),
        listen('menu_add_video', () =>
          app.pickAndLoadVideo().catch((e) => {
            app.errorMessage = e.message
          }),
        ),
        listen('menu_dev_reset', async () => {
          await backend.devClearCache().catch(() => {})
          sessionStorage.setItem('dev_reset', '1')
          window.location.reload()
        }),
      ]
      return () => unlisteners.forEach((p) => p.then((fn) => fn()))
    }
  })

  // ── Actions ────────────────────────────────────────────────────────────────
  // Open the activity picker. The picker owns native file selection and saved
  // activity selection, so every app UI path should go through it.
  async function handleOpenGpx() {
    openActivityPicker()
  }

  /**
   * Load an activity from either an absolute path (string, from the native
   * dialog) or a previously saved uploads-dir entry ({ savedFilename }).
   * Called by ActivityPickerModal and by the macOS recent menu.
   */
  async function loadActivityFromPickerOrMenu(source) {
    if (typeof source === 'string') {
      await loadGpx(source, app)
      backend.recordGpxOpened(source).catch(() => {})
    } else {
      const filename = source.savedFilename
      const stored = await backend.loadSavedActivity(filename)
      const result = typeof stored === 'string' ? JSON.parse(stored) : stored
      if (result.error) throw new Error(result.error)
      app.gpxFilename = result.filename ?? filename
      app.gpxStartTime = result.start_time ?? null
      app.activityDuration = result.duration_seconds
      app.activityMetrics = result.valid_attributes ?? null
      app.selectedSecond = 0
      if (app.config?.scene) {
        app.updateScene({ start: 0, end: app.timelineDuration })
      }
    }
    if (!app.config) {
      try {
        const def = await backend.getTemplate('default.json')
        app.config = def
        app.loadedTemplateFilename = 'default.json'
        app.updateScene({ start: 0, end: app.timelineDuration })
      } catch {
        /* use existing config */
      }
    }
  }

  async function handleOpenRecentGpx(path) {
    try {
      await loadActivityFromPickerOrMenu(path)
    } catch (err) {
      app.errorMessage = `Could not open ${path.split('/').pop()}: ${errorText(err)}`
    }
  }

  // Render is a two-step flow: the button opens the export-format dialog,
  // confirming there starts the actual render.
  let showExportFormatDialog = $state(false)
  // Free space on the output volume, fetched when the export dialog opens so
  // it can warn when the estimated file won't fit. null = unknown/not loaded.
  let exportDiskFreeBytes = $state(null)
  $effect(() => {
    if (!showExportFormatDialog) return
    exportDiskFreeBytes = null
    backend.diskFree(app.outputDir).then(
      (r) => (exportDiskFreeBytes = r?.free_bytes ?? null),
      () => {},
    )
  })
  // Whether the full render-progress dialog is open. Renders show the ambient
  // header chip by default; expanding opens the dialog on demand.
  let renderExpanded = $state(false)

  function handleRender() {
    if (rendering || !app.hasActivity) return
    showExportFormatDialog = true
  }

  // Measure any codec that has no same-machine numbers yet, one at a time
  // (the native calibrator is single-flight). Bails if a real render starts.
  async function warmUpExportEstimates() {
    for (const f of EXPORT_FORMATS) {
      if (app.renderingVideo) return
      if (exportTestAvailableFor(f.value)) {
        await app.calibrateExportEstimate(f.value)
      }
    }
  }

  // Warm up export estimates in the background, well before the user is likely
  // to open the export dialog, so its cards already show real time & size
  // numbers with no waiting. Re-arms whenever a new activity or template is
  // loaded; exportTestAvailableFor goes false after a successful measurement,
  // so an already-measured codec is skipped and this won't repeat needlessly.
  const EXPORT_WARMUP_DELAY_MS = 2500
  $effect(() => {
    // Coarse triggers only — read the identity of what's loaded, not the
    // deeply-reactive config, so template edits don't re-arm on every keystroke.
    const ready = app.hasActivity
    void app.loadedTemplateFilename
    if (!ready) return
    const timer = setTimeout(warmUpExportEstimates, EXPORT_WARMUP_DELAY_MS)
    return () => clearTimeout(timer)
  })

  async function startRender(format, fullFrame = false) {
    showExportFormatDialog = false
    if (rendering || !app.hasActivity) return
    if (format === 'stitched' && (!app.video?.path || app.video.missing)) {
      // Stitching needs footage — pick it now; bail if the user cancels.
      await app.pickAndLoadVideo()
      if (!app.video?.path || app.video.missing) return
    }
    app.exportFormat = format
    // Only the transparent formats carry a sizing choice — don't let a stitched
    // export (always full-frame) clobber the saved overlay preference.
    if (format !== 'stitched') app.exportFullFrame = fullFrame
    if (!hasRenderedOnce) {
      hasRenderedOnce = true
      localStorage.setItem(RENDERED_ONCE_KEY, 'true')
    }
    rendering = true
    renderExpanded = false
    try {
      const result = await renderVideo(app)
      if (result?.cancelled) console.log('Render cancelled')
    } catch (err) {
      app.errorMessage = err.message ?? 'Render failed'
    } finally {
      rendering = false
    }
  }

  async function handleOpenDownloads() {
    try {
      await backend.openDownloads(app.outputDir)
    } catch (e) {
      app.errorMessage = `Could not open output folder: ${e.message}`
    }
  }

  // Estimate render wall-clock time from the last recorded render FPS.
  // Re-evaluates whenever renderingVideo or config changes, so it picks up
  // the freshly-stored FPS right after a render finishes.
  // Output video length in seconds for a given format. A time-lapse
  // (scene.target_duration) compresses the whole window into that many seconds;
  // stitched exports ignore it (the footage can't be sped up to match, mirroring
  // the Rust guard), so they use the real window length.
  function outputDurationFor(format) {
    const start = app.config.scene.start ?? 0
    const end = app.config.scene.end ?? app.timelineDuration
    const window = end - start
    const td = app.config.scene.target_duration
    return format !== 'stitched' && td > 0 ? td : window
  }

  function renderEstimateSecsFor(format) {
    if (app.renderingVideo || !app.config?.scene || !app.hasActivity)
      return null
    const fps = app.config.scene.fps ?? 30
    const start = app.config.scene.start ?? 0
    const end = app.config.scene.end ?? app.timelineDuration
    if (start >= end) return null
    const renderFps = app.renderFpsFor(format)
    if (!renderFps || renderFps <= 0) return null
    return Math.round((outputDurationFor(format) * fps) / renderFps)
  }

  function renderFileSizeBytesFor(format) {
    if (!app.config?.scene || !app.hasActivity) return null
    const fps = app.config.scene.fps ?? 30
    const duration = outputDurationFor(format)
    if (duration <= 0) return null
    const calibration = app.exportSizeCalibrationFor(format)
    // qtrle (lossless RLE) has no reliable prior — its size swings wildly with
    // overlay content, so it shows nothing and offers a quick test render
    // instead. ProRes and stitched (H.264) both have solid built-in priors.
    if (!calibration && format === 'qtrle') return null
    const fallbackBps =
      format === 'stitched'
        ? DEFAULT_STITCHED_BITS_PER_PIXEL_SECOND
        : DEFAULT_BITS_PER_PIXEL_SECOND
    return estimateExportBytes(
      app.outputWidth,
      app.outputHeight,
      fps,
      duration,
      calibration,
      fallbackBps,
    )
  }

  function renderFileSizeEstFor(format) {
    const bytes = renderFileSizeBytesFor(format)
    return bytes == null ? null : formatFileSize(bytes)
  }

  let renderEstimateSecs = $derived(renderEstimateSecsFor(app.exportFormat))
  let renderFileSizeEst = $derived(renderFileSizeEstFor(app.exportFormat))

  let renderTooltip = $derived.by(() => {
    if (!app.config) return 'Load a template first'
    if (!app.hasActivity) return 'Load an activity first'
    if (app.renderingVideo) return 'Render in progress'
    const lines = []
    if (renderEstimateSecs != null)
      lines.push(`~${formatTime(renderEstimateSecs)} to render`)
    if (renderFileSizeEst != null) lines.push(`~${renderFileSizeEst} output`)
    return lines.length > 0 ? lines.join('\n') : null
  })

  // Per-card estimates for the export dialog — render time and file size as
  // separate labeled fields, so the codecs compare at a glance without
  // clicking between them.
  function exportTimeEstimateFor(format) {
    const secs = renderEstimateSecsFor(format)
    return secs != null ? `~${formatTime(secs)}` : null
  }

  function exportSizeEstimateFor(format) {
    const size = renderFileSizeEstFor(format)
    return size != null ? `~${size}` : null
  }

  // A codec with no same-machine measurement gets a "quick test" offer on its
  // card instead of estimates derived from the other codec. Stitched is
  // excluded: a test render can't include the source footage, so its numbers
  // only come from real exports.
  function exportTestAvailableFor(format) {
    if (format === 'stitched') return false
    if (!app.hasActivity || !app.config?.scene || app.renderingVideo)
      return false
    return (
      renderEstimateSecsFor(format) == null ||
      renderFileSizeEstFor(format) == null
    )
  }

  let gpxLabel = $derived.by(() => {
    if (!app.gpxFilename) return 'Load Activity'
    return app.gpxFilename.split(/[\\/]/).pop()
  })

  // 1 = no template, 2 = template but no activity, 0 = ready
  let onboardingStep = $derived.by(() => {
    if (!app.config) return 1
    if (!app.hasActivity) return 2
    return 0
  })
</script>

<svelte:window
  onkeydown={onWindowKeydown}
  onclick={() => {
    showResolutionMenu = false
  }}
/>

<div
  class="h-screen flex flex-col gap-2 bg-black p-2 text-foreground overflow-hidden select-none"
>
  <ErrorToast />
  <UpdateBanner />
  <RenderProgressOverlay
    expanded={renderExpanded}
    format={EXPORT_FORMATS.find((f) => f.value === app.exportFormat) ?? null}
    onminimize={() => (renderExpanded = false)}
  />
  {#if showSettings}
    <Settings
      onclose={() => {
        showSettings = false
      }}
    />
  {/if}
  {#if showAbout}
    <AboutModal onclose={() => (showAbout = false)} />
  {/if}
  {#if app.showTemplatePicker}
    <TemplatePickerModal
      onclose={() => {
        app.showTemplatePicker = false
      }}
    />
  {/if}
  {#if showActivityPicker}
    <ActivityPickerModal
      onload={loadActivityFromPickerOrMenu}
      onclose={() => {
        showActivityPicker = false
      }}
    />
  {/if}
  {#if showExportFormatDialog}
    <ExportFormatDialog
      formats={EXPORT_FORMATS}
      initial={EXPORT_FORMATS.some((f) => f.value === app.exportFormat)
        ? app.exportFormat
        : EXPORT_FORMATS[0].value}
      initialFullFrame={app.exportFullFrame}
      timeFor={exportTimeEstimateFor}
      sizeFor={exportSizeEstimateFor}
      bytesFor={renderFileSizeBytesFor}
      diskFreeBytes={exportDiskFreeBytes}
      usesRiderWeight={app.usesRiderWeight}
      testAvailableFor={exportTestAvailableFor}
      calibrating={app.calibratingFormat}
      oncalibrate={(fmt) => app.calibrateExportEstimate(fmt)}
      onconfirm={startRender}
      oncancel={() => {
        showExportFormatDialog = false
      }}
    />
  {/if}
  {#if showNewTemplateDialog}
    <NewTemplateDialog
      oncreate={async (name) => {
        showNewTemplateDialog = false
        await app.newTemplate(name).catch((e) => {
          app.errorMessage = e.message
        })
      }}
      oncancel={() => {
        showNewTemplateDialog = false
      }}
    />
  {/if}
  {#if app.pendingDiscard}
    <ConfirmDialog
      title="Discard unsaved changes?"
      message="This template has unsaved edits. Switching will lose them. Save the template first if you want to reuse it."
      confirmText="Discard"
      cancelText="Keep editing"
      onconfirm={() => app.resolvePendingDiscard(true)}
      oncancel={() => app.resolvePendingDiscard(false)}
    />
  {/if}
  {#if showRevertConfirm}
    <ConfirmDialog
      title="Revert template?"
      message="This will discard all unsaved changes and reload the last saved version. This cannot be undone."
      confirmText="Revert"
      cancelText="Keep editing"
      dontShowAgainLabel="Don't ask again"
      onconfirm={(skip) => {
        if (skip) localStorage.setItem(REVERT_SKIP_KEY, 'true')
        showRevertConfirm = false
        app.revertTemplate().catch((e) => {
          app.errorMessage = e?.message ?? String(e)
        })
      }}
      oncancel={() => {
        showRevertConfirm = false
      }}
    />
  {/if}

  <!-- ── Header ─────────────────────────────────────────────────────────────── -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header
    use:windowDrag
    class="h-[52px] shrink-0 bg-[var(--panel)] rounded-[10px] flex items-center gap-2 pr-3 pl-[88px] z-50"
  >
    <!-- ── Template toolbar ─────────────────────────────────────────────────── -->
    <div class="flex items-center gap-1 shrink-0">
      <!-- Template picker -->
      <Tooltip content="Choose a template" side="bottom" delay={TOOLTIP_DELAY}>
        <button
          onclick={() => {
            openTemplatePicker()
          }}
          class="hdr-btn px-2.5 gap-1.5 max-w-[170px] min-w-0 {onboardingStep ===
          1
            ? 'onboarding-glow'
            : ''}"
        >
          <LayoutGrid size={12} class="text-zinc-500 shrink-0" />
          <span
            class="truncate {templateLabel ? 'text-zinc-200' : 'text-zinc-500'}"
          >
            {templateLabel ?? 'Templates…'}
          </span>
          {#if app.isTemplateModified}
            <span class="text-amber-400 shrink-0" title="Unsaved changes"
              >•</span
            >
          {/if}
        </button>
      </Tooltip>

      {#if AI_ASSISTANT_ENABLED}
        <!-- AI assistant (sidebar toggle) -->
        <Tooltip
          content={showAiChat ? 'Close AI assistant' : 'Open AI assistant'}
          side="bottom"
          delay={TOOLTIP_DELAY}
        >
          <button
            onclick={toggleAiChat}
            class="hdr-btn hdr-btn-icon shrink-0 cursor-pointer transition-colors
                 {showAiChat
              ? 'bg-primary/20 text-primary hover:bg-primary/25'
              : 'bg-primary/10 text-primary hover:bg-primary/20 hover:text-primary'}"
            aria-label={showAiChat ? 'Close AI assistant' : 'Open AI assistant'}
            aria-pressed={showAiChat}><Sparkles size={12} /></button
          >
        </Tooltip>
      {/if}

      <!-- Save — amber when modified -->
      {#if app.isTemplateModified}
        <Tooltip content="Save template" side="bottom" delay={TOOLTIP_DELAY}>
          <button
            onclick={() =>
              app.saveTemplate().catch((e) => {
                app.errorMessage = e?.message ?? String(e)
              })}
            class="hdr-btn hdr-btn-icon shrink-0 bg-amber-400/10 text-amber-400
                   hover:bg-amber-400/20 hover:text-amber-300"
            ><Save size={12} /></button
          >
        </Tooltip>

        <!-- Revert to last saved -->
        <Tooltip
          content="Revert to last saved"
          side="bottom"
          delay={TOOLTIP_DELAY}
        >
          <button
            onclick={handleRevertClick}
            class="hdr-btn hdr-btn-icon shrink-0"
            ><RotateCcw size={12} /></button
          >
        </Tooltip>
      {/if}

      {#if isDev && app.templates.find((t) => t.id === app.loadedTemplateFilename)?.type === 'community-modified'}
        <Tooltip content="Push to repo" side="bottom" delay={TOOLTIP_DELAY}>
          <button
            onclick={overwriteRepoTemplate}
            class="hdr-btn hdr-btn-icon shrink-0 border-sky-500/60 bg-sky-500/10 text-sky-400
                   hover:border-sky-400 hover:bg-sky-500/20 hover:text-sky-300 cursor-pointer"
            >⬆</button
          >
        </Tooltip>
      {/if}
    </div>

    <div class="h-5 w-px bg-white/[0.07] shrink-0"></div>

    <!-- Activity file picker -->
    <Tooltip content="Choose an activity" side="bottom" delay={TOOLTIP_DELAY}>
      <button
        onclick={handleOpenGpx}
        class="hdr-btn px-2.5 gap-1.5 max-w-[160px] {onboardingStep === 2
          ? 'onboarding-glow'
          : ''}"
      >
        <Activity size={12} class="shrink-0" />
        <span class="truncate">{gpxLabel}</span>
      </button>
    </Tooltip>

    <!-- Video — always shown, dashed border signals optional -->
    <div class="h-5 w-px bg-white/[0.07] shrink-0"></div>
    {#if !app.video}
      <Tooltip
        content="Adding video is only for preview. The exported overlay will not include the video."
        side="bottom"
        class="shrink-0"
      >
        <button
          onclick={() => app.pickAndLoadVideo()}
          class="hdr-btn px-2.5 gap-1.5 border-dashed border-[var(--panel3)] bg-transparent hover:bg-[var(--panel2)]"
        >
          <Film size={12} class="shrink-0" />
          Add video…
        </button>
      </Tooltip>
    {:else if app.video.missing}
      <div class="flex items-center gap-1 shrink-0">
        <Tooltip
          content="Locate replacement video"
          side="bottom"
          delay={TOOLTIP_DELAY}
        >
          <button
            onclick={() => app.pickAndLoadVideo()}
            class="hdr-btn px-2.5 gap-1.5 max-w-[160px] border-red-800/60 text-red-300 hover:border-red-600/60 hover:bg-red-900/30 hover:text-red-200"
            title={app.video.path}
          >
            <AlertTriangle size={12} class="text-red-400 shrink-0" />
            <span class="truncate">{videoBasename(app.video.path)}</span>
          </button>
        </Tooltip>
        <button
          onclick={() => app.clearVideo()}
          class="hdr-btn hdr-btn-icon shrink-0"><X size={12} /></button
        >
      </div>
    {:else}
      <div class="flex items-center gap-1 shrink-0">
        <Tooltip content="Replace video" side="bottom" delay={TOOLTIP_DELAY}>
          <button
            onclick={() => app.pickAndLoadVideo()}
            class="hdr-btn px-2.5 gap-1.5 max-w-[170px]"
            title={app.video.path}
          >
            <Film size={12} class="text-zinc-500 shrink-0" />
            <span class="truncate text-zinc-200"
              >{videoBasename(app.video.path)}</span
            >
          </button>
        </Tooltip>
        {#if canUseRecordingTime}
          <button
            onclick={moveVideoToRecordingTime}
            title={app.video.creationTimeNote
              ? `Align to camera recording time — ${app.video.creationTimeNote}`
              : 'Align to camera recording time'}
            class="hdr-btn hdr-btn-icon shrink-0"><Clock size={12} /></button
          >
        {/if}
        <Tooltip
          content={app.videoUnderlayVisible
            ? 'Hide video underlay'
            : 'Show video underlay'}
          side="bottom"
          delay={TOOLTIP_DELAY}
        >
          <button
            onclick={() =>
              app.setVideoUnderlayVisible(!app.videoUnderlayVisible)}
            class="hdr-btn hdr-btn-icon hdr-btn-ghost shrink-0 {app.videoUnderlayVisible
              ? ''
              : 'text-[var(--dim)]'}"
            aria-label={app.videoUnderlayVisible
              ? 'Hide video underlay'
              : 'Show video underlay'}
            aria-pressed={app.videoUnderlayVisible}
          >
            {#if app.videoUnderlayVisible}
              <Eye size={12} />
            {:else}
              <EyeOff size={12} />
            {/if}
          </button>
        </Tooltip>
        <button
          onclick={() => app.clearVideo()}
          class="hdr-btn hdr-btn-icon shrink-0"><X size={12} /></button
        >
      </div>
    {/if}

    <!-- ── Scene toolbar: resolution + FPS (shown when a template is loaded) ── -->
    {#if app.config?.scene}
      <div class="h-5 w-px bg-white/[0.07] shrink-0"></div>

      <!-- Resolution popover -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div class="relative shrink-0" onclick={(e) => e.stopPropagation()}>
        <Tooltip
          content="Choose output resolution"
          side="bottom"
          delay={TOOLTIP_DELAY}
        >
          <button
            type="button"
            onclick={() => {
              showResolutionMenu = !showResolutionMenu
            }}
            class="hdr-btn px-2.5 gap-1.5 min-w-[112px] justify-between {showResolutionMenu
              ? 'bg-[var(--panel3)] text-zinc-100'
              : ''}"
            aria-haspopup="menu"
            aria-expanded={showResolutionMenu}
          >
            <span class="inline-flex items-center gap-1.5 min-w-0">
              <Monitor size={12} class="text-zinc-500 shrink-0" />
              <span class="truncate">{resolutionLabel}</span>
            </span>
            <ChevronDown size={12} class="text-zinc-600 shrink-0" />
          </button>
        </Tooltip>

        {#if showResolutionMenu}
          <div
            class="resolution-popover absolute left-0 top-[calc(100%+6px)] z-[80] w-56 rounded-[10px] border border-white/[0.08] bg-[#181818] p-2 shadow-2xl shadow-black/60"
            role="menu"
          >
            <div class="grid grid-cols-2 gap-1">
              {#each RES_PRESETS as p (p.label)}
                {@const active =
                  !customResActive &&
                  app.outputWidth === p.w &&
                  app.outputHeight === p.h}
                <button
                  type="button"
                  onclick={() => chooseResolution(p)}
                  class="resolution-option {active
                    ? 'resolution-option--active'
                    : ''}"
                  role="menuitem"
                >
                  <span class="font-medium">{p.label}</span>
                  <span class="font-mono text-[10px] text-zinc-500"
                    >{p.w}×{p.h}</span
                  >
                </button>
              {/each}
            </div>

            <div class="mt-2 border-t border-white/[0.06] pt-2">
              <button
                type="button"
                onclick={() => {
                  customResActive = true
                }}
                class="resolution-option w-full {customResActive
                  ? 'resolution-option--active'
                  : ''}"
              >
                <span class="font-medium">Custom</span>
                <span class="font-mono text-[10px] text-zinc-500"
                  >{app.outputWidth}×{app.outputHeight}</span
                >
              </button>

              <div class="mt-2 flex items-center gap-1.5">
                <input
                  type="number"
                  value={app.outputWidth}
                  min={1}
                  onfocus={() => {
                    customResActive = true
                  }}
                  oninput={(e) => {
                    customResActive = true
                    const v = parseInt(e.target.value)
                    if (v > 0) app.outputWidth = v
                  }}
                  class="h-7 min-w-0 flex-1 rounded-[6px] border px-2 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-ring {customResActive
                    ? 'border-transparent bg-[var(--panel3)] text-foreground'
                    : 'border-transparent bg-[var(--panel2)] text-zinc-500'}"
                  aria-label="Output width"
                />
                <span class="text-zinc-600 text-xs">×</span>
                <input
                  type="number"
                  value={app.outputHeight}
                  min={1}
                  onfocus={() => {
                    customResActive = true
                  }}
                  oninput={(e) => {
                    customResActive = true
                    const v = parseInt(e.target.value)
                    if (v > 0) app.outputHeight = v
                  }}
                  class="h-7 min-w-0 flex-1 rounded-[6px] border px-2 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-ring {customResActive
                    ? 'border-transparent bg-[var(--panel3)] text-foreground'
                    : 'border-transparent bg-[var(--panel2)] text-zinc-500'}"
                  aria-label="Output height"
                />
              </div>
            </div>
          </div>
        {/if}
      </div>

      {#if app.hasActivity}
        <div class="h-5 w-px bg-white/[0.07] shrink-0"></div>

        <!-- FPS -->
        <div class="flex items-center gap-2 shrink-0 pl-1">
          <span class="text-xs text-[var(--dim)]">FPS</span>
          <input
            type="number"
            min="1"
            max="240"
            value={app.config.scene.fps ?? 30}
            oninput={(e) => {
              const v = parseInt(e.target.value)
              if (v > 0) app.updateScene({ fps: v })
            }}
            class="h-[30px] w-[52px] rounded-lg border-0 bg-[var(--panel2)] px-2 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-ring font-mono"
          />
        </div>
      {/if}
    {/if}

    <div class="flex-1"></div>

    <!-- Render button / ambient render status -->
    <div class="flex items-center gap-2">
      {#if app.renderingVideo}
        <RenderStatusChip onexpand={() => (renderExpanded = true)} />
      {:else}
        <Tooltip content={renderTooltip} side="bottom" align="end">
          <Button
            onclick={handleRender}
            disabled={!app.config || !app.hasActivity}
            class="h-[34px] gap-1.5 min-w-[104px] rounded-lg px-4 font-semibold text-white
                   shadow-[0_4px_14px_-4px_rgba(220,20,60,0.6)] hover:bg-[var(--accent-hover)]
                   disabled:bg-primary/25 disabled:text-white/60 disabled:opacity-100 disabled:shadow-none
                   {onboardingStep === 0 && !hasRenderedOnce
              ? 'onboarding-glow'
              : ''}"
            size="sm"
          >
            <Play size={13} fill="currentColor" strokeWidth={0} />
            Render Video
          </Button>
        </Tooltip>
      {/if}
    </div>
  </header>

  <!-- ── Three-panel layout — panels float on the black canvas with 8px gaps ── -->
  <div class="flex-1 flex gap-2 overflow-hidden min-h-0">
    {#if app.config && app.hasActivity}
      <LeftSidebar />
    {/if}
    <CenterCanvas onopenactivity={handleOpenGpx} />
    {#if (app.config && app.hasActivity) || showAiChat}
      <RightPanel
        aiChatOpen={showAiChat}
        onreopenAiChat={() => (showAiChat = true)}
        aiAssistantEnabled={AI_ASSISTANT_ENABLED}
      />
    {/if}
  </div>
</div>
