<script>
  /**
   * Center panel: manages the frame buffer, canvas preview, WYSIWYG SVG overlay,
   * and playback controls. This is the heart of the new UI.
   */
  import { getContext, untrack } from 'svelte'
  import { SvelteMap, SvelteSet } from 'svelte/reactivity'
  import PreviewCanvas from '../canvas/PreviewCanvas.svelte'
  import VideoBackdrop from '../canvas/VideoBackdrop.svelte'
  import WysiwygLayer from '../canvas/WysiwygLayer.svelte'
  import PlaybackControls from '../canvas/PlaybackControls.svelte'
  import VideoAlignmentBar from '../canvas/VideoAlignmentBar.svelte'
  import { videoStartOnAxis } from '@/lib/videoAlignment.js'
  import { isLapMetric } from '@/lib/elementTypes.js'
  import * as backend from '@/api/backend.js'
  import { Github } from 'lucide-svelte'

  let { onopenactivity } = $props()
  const app = getContext('app')

  // ── Frame buffer ─────────────────────────────────────────────────────────────
  // Cache keyed by frame index (integer at previewFps resolution).
  // frame index 0 = scene start, increases by 1 per (1/previewFps) seconds.
  let cache = new SvelteMap()
  let pending = new SvelteSet()
  let shownWarnings = new SvelteSet()

  let currentFrameData = $state(null) // { image, elements }
  let fetchError = $state(null)
  let previewNotice = $state(null)
  let playing = $state(false)
  let rafHandle = null
  let lastTick = null
  let stallTimer = null
  let configDebounce = null
  let previewGeneration = 0

  // Rolling render-latency samples (ms) used by the Auto frame-rate tuner.
  // Plain array — intentionally non-reactive; we don't want UI re-runs per sample.
  let renderLatencies = []
  let autoFpsCooldownUntil = 0

  const PREFETCH_AHEAD = 5
  const MAX_CACHE = 30
  const MAX_CONCURRENT = 3
  const PREVIEW_SLOW_MS = 8000
  const PREVIEW_HARD_TIMEOUT_MS = 45000
  const MAX_VIEWER_ZOOM = 10
  // Auto preview-rate bounds. Capped well below 30fps: the preview only needs to
  // read smoothly, and a higher rate burns CPU/GPU re-rendering frames the eye
  // can't distinguish on sparse telemetry. Floor keeps it from feeling choppy.
  const MIN_PREVIEW_FPS = 2
  const MAX_PREVIEW_FPS = 15

  let previewFps = $derived(app.previewFps ?? 1)

  // Convert absolute second → frame index relative to scene start
  function secToFrameIdx(sec, fps, start) {
    return Math.round((sec - start) * fps)
  }

  // Buffered positions as seconds (for the scrub bar indicators)
  let bufferedSeconds = $derived(
    [...cache.keys()]
      .map((idx) => sceneStart + idx / previewFps)
      .sort((a, b) => a - b),
  )

  function clearBuffer() {
    previewGeneration += 1
    cache.clear()
    pending.clear()
    fetchError = null
    previewNotice = null
    if (stallTimer) {
      clearTimeout(stallTimer)
      stallTimer = null
    }
  }

  async function fetchFrame(frameIdx) {
    const config = app.renderConfig
    if (!config) return console.debug('[tpl-diag] fetchFrame bail: no config')
    if (!app.hasActivity)
      return console.debug('[tpl-diag] fetchFrame bail: no activity')
    const fps = app.previewFps ?? 1
    const start = config.scene?.start ?? 0
    const end = config.scene?.end ?? app.timelineDuration
    const maxFrameIdx = Math.round((end - start) * fps)
    if (frameIdx < 0 || frameIdx > maxFrameIdx)
      return console.debug(
        '[tpl-diag] fetchFrame bail: frameIdx out of range',
        { frameIdx, maxFrameIdx, start, end },
      )
    if (cache.has(frameIdx) || pending.has(frameIdx))
      return console.debug(
        '[tpl-diag] fetchFrame bail: cache/pending has',
        frameIdx,
      )
    if (pending.size >= MAX_CONCURRENT)
      return console.debug(
        '[tpl-diag] fetchFrame bail: MAX_CONCURRENT',
        pending.size,
      )
    console.debug('[tpl-diag] fetchFrame start', { frameIdx, fps, start, end })
    const gpx = app.gpxFilename

    pending.add(frameIdx)
    const generation = previewGeneration
    let slowTimer = null
    let hardTimer = null
    try {
      slowTimer = setTimeout(() => {
        if (generation !== previewGeneration || currentFrameData) return
        previewNotice =
          'Preparing preview is taking a little longer than usual…'
      }, PREVIEW_SLOW_MS)
      const timeout = new Promise((_, reject) => {
        hardTimer = setTimeout(
          () =>
            reject(
              new Error(
                'Preview is taking too long to generate. Choose an activity and template again, or retry if this keeps happening.',
              ),
            ),
          PREVIEW_HARD_TIMEOUT_MS,
        )
      })
      const renderStart = performance.now()
      let data = await Promise.race([
        backend.nativeGenerateDemo(
          config,
          gpx,
          frameIdx,
          fps,
          previewW,
          previewH,
          app.riderWeightKg,
        ),
        timeout,
      ])
      if (generation !== previewGeneration) return
      recordRenderLatency(performance.now() - renderStart)
      if (data?.image) {
        console.debug('[tpl-diag] fetchFrame got image', {
          frameIdx,
          elements: data.elements?.length,
        })
        fetchError = null
        previewNotice = null
        if (stallTimer) {
          clearTimeout(stallTimer)
          stallTimer = null
        }
        // If the saved GPX file no longer exists, clear the stale state and
        // open the activity picker so the user can reselect.
        if (data.warning && !shownWarnings.has(data.warning)) {
          shownWarnings.add(data.warning)
          if (data.warning.includes('not found')) {
            app.gpxFilename = null
            app.activityDuration = 0
            onopenactivity?.()
          } else {
            app.errorMessage = data.warning
          }
        }
        cache.set(frameIdx, data)
        // Evict oldest frames beyond MAX_CACHE
        if (cache.size > MAX_CACHE) {
          const oldest = cache.keys().next().value
          cache.delete(oldest)
        }
        // Show this frame if it's current, or if we don't have the current frame yet
        const currentIdx = secToFrameIdx(app.selectedSecond, fps, start)
        if (
          frameIdx === currentIdx ||
          (frameIdx < currentIdx && !cache.has(currentIdx))
        ) {
          console.debug('[tpl-diag] currentFrameData <- frame', frameIdx)
          currentFrameData = data
        } else {
          console.debug('[tpl-diag] frame NOT shown (not current)', {
            frameIdx,
            currentIdx,
          })
        }
      }
    } catch (e) {
      if (generation !== previewGeneration) return
      console.warn('Frame fetch failed for frame', frameIdx, e)
      const fps2 = app.previewFps ?? 1
      const start2 = app.config?.scene?.start ?? 0
      const currentIdx = secToFrameIdx(app.selectedSecond, fps2, start2)
      const isCurrent = frameIdx === currentIdx
      const wasNeeded = !currentFrameData
      if (isCurrent || wasNeeded) {
        fetchError = e?.message ?? String(e)
      }
    } finally {
      if (slowTimer) clearTimeout(slowTimer)
      if (hardTimer) clearTimeout(hardTimer)
      pending.delete(frameIdx)
    }
  }

  function recordRenderLatency(ms) {
    if (!(ms > 0)) return
    renderLatencies.push(ms)
    if (renderLatencies.length > 10) renderLatencies.shift()
    maybeAutoTuneFps()
  }

  function median(arr) {
    if (!arr.length) return 0
    const s = [...arr].sort((a, b) => a - b)
    const m = Math.floor(s.length / 2)
    return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2
  }

  // Derive a sustainable preview frame rate from measured render latency. With
  // up to MAX_CONCURRENT renders in flight, throughput ≈ MAX_CONCURRENT / latency;
  // we keep an 80% margin and clamp to MIN_PREVIEW_FPS–MAX_PREVIEW_FPS to balance
  // smoothness against resource use. Hysteresis + a cooldown stop it flapping
  // (each fps change rebuilds the frame buffer). While playing we only step DOWN —
  // raising fps mid-play would clear the buffer and stutter; we step up only when
  // idle, where the rebuild is invisible.
  function maybeAutoTuneFps() {
    if (renderLatencies.length < 3) return
    const med = median(renderLatencies)
    if (med <= 0) return
    const sustainable = (MAX_CONCURRENT * 1000) / med
    const target = Math.max(
      MIN_PREVIEW_FPS,
      Math.min(MAX_PREVIEW_FPS, Math.round(sustainable * 0.8)),
    )
    const current = app.previewFps ?? 1
    if (target === current) return
    const now = performance.now()
    if (now < autoFpsCooldownUntil) return
    if (Math.abs(target - current) / current < 0.25) return
    if (playing && target > current) return
    app.previewFps = target
    autoFpsCooldownUntil = now + 1500
  }

  // Re-fetch when config, gpx, or previewFps changes — debounced so a burst of
  // edits (e.g. dragging the color-picker wheel, which fires an input event per
  // hovered color) coalesces into a single render once the user pauses. The old
  // frame stays visible during the debounce window, so there's no flicker.
  $effect(() => {
    const _config = app.config
    const _fps = app.previewFps ?? 1
    const _hasActivity = app.hasActivity
    void app.gpxFilename // reactive dep: re-run when GPX changes
    void app.units // reactive dep: the unit-system preference flips readouts
    void previewW // reactive dep: re-render when the base target size changes
    void previewH // (export resolution, preview box size, or display pixel ratio)

    if (configDebounce) clearTimeout(configDebounce)
    configDebounce = setTimeout(() => {
      configDebounce = null
      clearBuffer()
      if (!_config || !_hasActivity) {
        currentFrameData = null
        app.currentPreviewImage = null
        return
      }
      const start = _config.scene?.start ?? 0
      const end = _config.scene?.end ?? app.timelineDuration
      // Don't attempt to fetch when the timeline range is invalid — the sidebar
      // already shows a validation error; no point spinning here too.
      if (end <= start) {
        currentFrameData = null
        return
      }
      const s = Math.max(start, app.selectedSecond)
      const frameIdx = secToFrameIdx(s, _fps, start)
      fetchFrame(frameIdx)
      if (stallTimer) clearTimeout(stallTimer)
      stallTimer = setTimeout(() => {
        stallTimer = null
      }, 5000)
    }, 160)

    return () => {
      if (configDebounce) {
        clearTimeout(configDebounce)
        configDebounce = null
      }
    }
  })

  // Prefetch around playhead. cache.get(frameIdx) is the only intended reactive dep —
  // it re-runs when that frame arrives. fetchFrame calls are untracked to avoid
  // reads of pending/cache inside fetchFrame triggering re-runs.
  $effect(() => {
    const fps = previewFps
    const start = sceneStart
    const frameIdx = secToFrameIdx(app.selectedSecond, fps, start)
    const frame = cache.get(frameIdx)
    if (frame) currentFrameData = frame
    // When the renderer falls behind during playback the previous frame stays
    // on screen; on sparse telemetry that hold is imperceptible, and the auto
    // tuner backs off the frame rate on its own — so there's nothing to surface.
    untrack(() => {
      for (let i = 0; i < PREFETCH_AHEAD; i++) fetchFrame(frameIdx + i)
    })
  })

  // Keep app.currentPreviewImage in sync so saveTemplate can use the latest frame.
  $effect(() => {
    if (currentFrameData?.image)
      app.currentPreviewImage = currentFrameData.image
    else if (!app.hasActivity) app.currentPreviewImage = null
  })

  // ── Playback RAF loop ────────────────────────────────────────────────────────
  $effect(() => {
    if (playing) {
      lastTick = performance.now()
      rafHandle = requestAnimationFrame(tick)
    } else {
      if (rafHandle) cancelAnimationFrame(rafHandle)
    }
    return () => {
      if (rafHandle) cancelAnimationFrame(rafHandle)
    }
  })

  // While the video backdrop is in range and playing, the video element is
  // the master clock — it emits `timeupdate` events that drive
  // selectedSecond directly. RAF skips its own advancement in that mode so
  // we're not racing two clocks (and so the sync effect doesn't have to
  // fight RAF with constant seeks, which was the source of stutter).
  let videoIsMaster = $derived.by(() => {
    if (!playing) return false
    if (!app.videoUnderlayVisible) return false
    const v = app.video
    if (!v || v.missing || !(v.duration > 0)) return false
    const startAbs = videoStartOnAxis(app.gpxStartTime, v)
    return (
      app.selectedSecond >= startAbs &&
      app.selectedSecond <= startAbs + v.duration
    )
  })

  function tick(now) {
    if (!playing) return
    const dt = (now - lastTick) / 1000
    lastTick = now
    if (!videoIsMaster) {
      const next = Math.min(app.selectedSecond + dt * playbackSpeedup, sceneEnd)
      app.selectedSecond = next
    }
    // Always check the end condition — works for both master modes.
    if (app.selectedSecond >= sceneEnd) {
      app.selectedSecond = sceneEnd
      playing = false
      return
    }
    rafHandle = requestAnimationFrame(tick)
  }

  function seek(s) {
    app.selectedSecond = s
  }

  async function reportPreviewIssue() {
    if (!fetchError) return
    const scene = app.config?.scene ?? {}
    const body = [
      'A preview render failed in Cyclemetry.',
      '',
      '### Error',
      '```text',
      fetchError,
      '```',
      '',
      '### Context',
      `- Template: ${app.loadedTemplateFilename ?? 'unsaved/current template'}`,
      `- Activity: ${app.gpxFilename ?? 'none'}`,
      `- Scene range: ${scene.start ?? 0}s to ${scene.end ?? app.timelineDuration}s`,
      `- Output: ${app.outputWidth ?? 1920}x${app.outputHeight ?? 1080}`,
      `- Preview FPS: ${app.previewFps ?? 1}`,
      '',
      '### What I was doing',
      'Describe what you clicked or changed right before this appeared.',
    ].join('\n')
    try {
      const summary = fetchError.replace(/\s+/g, ' ').slice(0, 90)
      await backend.reportIssue(`Preview render error: ${summary}`, body)
    } catch (e) {
      app.errorMessage = `Could not open GitHub issue: ${e?.message ?? e}`
    }
  }

  let sceneStart = $derived(app.config?.scene?.start ?? 0)
  let sceneEnd = $derived(app.config?.scene?.end ?? app.timelineDuration)

  // Time-lapse: the whole ride window plays back in `target_duration` seconds,
  // so the editor preview matches the exported clip length. The scrubber still
  // maps to ride-time (the preview is sampled at real time); we just advance the
  // playhead `speedup×` faster during playback. 1 = real time (no time-lapse).
  let playbackSpeedup = $derived.by(() => {
    const td = app.config?.scene?.target_duration
    const window = sceneEnd - sceneStart
    return td > 0 && window > 0 ? window / td : 1
  })
  let quickStartStep = $derived(!app.config ? 1 : !app.hasActivity ? 2 : 0)
  let quickStartStep1Complete = $derived(!!app.config)
  let quickStartStep2Complete = $derived(app.hasActivity)

  // ── Distance reference slider ─────────────────────────────────────────────────
  // Show an amber dot on a second bar when a distance element with reference='until_custom' is selected.
  let selectedDistanceEl = $derived.by(() => {
    const id = app.selectedElementId
    const config = app.config
    if (!id || !config?.elements) return null
    const el = config.elements.find((e) => e.id === id)
    return el && el.type === 'value' ? el : null
  })

  let selectedCourseMarker = $derived.by(() => {
    const id = app.selectedElementId
    const config = app.config
    if (!id || !config?.elements) return null
    const el = config.elements.find((e) => e.id === id)
    if (
      !el ||
      el.type !== 'plot' ||
      el.value !== 'course' ||
      !el.markers?.length
    )
      return null
    return (
      el.markers.find((m) => m.id === app.selectedCourseMarkerId) ??
      el.markers[0]
    )
  })

  let showDistanceBar = $derived(
    selectedDistanceEl?.value === 'distance' &&
      (selectedDistanceEl?.distance_reference === 'until_custom' ||
        selectedDistanceEl?.distance_reference === 'since_custom'),
  )
  let showTimeBar = $derived(
    selectedDistanceEl?.value === 'time' &&
      (selectedDistanceEl?.time_reference === 'until_custom' ||
        selectedDistanceEl?.time_reference === 'since_custom'),
  )
  let showCourseMarkerBar = $derived(!!selectedCourseMarker)

  let customDistanceM = $derived.by(() => {
    if (!showDistanceBar || !selectedDistanceEl) return null
    const t = selectedDistanceEl.distance_target ?? 0
    const u = selectedDistanceEl.unit ?? 'km'
    if (u === 'm') return t
    if (u === 'mi') return t * 1609.34
    return t * 1000
  })
  let courseMarkerDistanceM = $derived(
    showCourseMarkerBar ? (selectedCourseMarker?.distance ?? 0) : null,
  )

  let distanceInfo = $state(null)

  $effect(() => {
    if ((!showDistanceBar && !showCourseMarkerBar) || !app.config) {
      distanceInfo = null
      return
    }
    if (!app.hasActivity) {
      distanceInfo = null
      return
    }
    const gpx = app.gpxFilename
    const start = sceneStart
    const end = sceneEnd
    backend
      .getActivityDistanceInfo(gpx, start, end)
      .then((info) => {
        distanceInfo = info
      })
      .catch(() => {
        distanceInfo = null
      })
  })

  function onCustomDistanceChange(newM) {
    const id = app.selectedElementId
    const el = app.config?.elements?.find((e) => e.id === id)
    if (!el || el.type !== 'value') return
    const unit = el.unit ?? 'km'
    let displayVal
    if (unit === 'm') displayVal = newM
    else if (unit === 'mi')
      displayVal = Math.round((newM / 1609.34) * 100) / 100
    else displayVal = Math.round((newM / 1000) * 100) / 100
    app.updateElement(id, { distance_target: displayVal })
  }

  let customTimeS = $derived(
    showTimeBar ? (selectedDistanceEl?.time_target ?? 0) : null,
  )

  function onCustomTimeChange(newS) {
    const id = app.selectedElementId
    const el = app.config?.elements?.find((e) => e.id === id)
    if (!el || el.type !== 'value') return
    app.updateElement(id, { time_target: Math.round(newS) })
  }

  // ── Race bar (lap start/finish gate) ────────────────────────────────────
  // Dual-handle bar shown while a lap-metric value element is selected. The
  // handles are the race-start and race-finish moments (scene.lap_gate);
  // dragging one also scrubs the preview there so the user can see themselves
  // cross the line. Defaults span the overlay window until the gate exists.
  let lapGateForBar = $derived.by(() => {
    if (!selectedDistanceEl || !isLapMetric(selectedDistanceEl.value))
      return null
    const gate = app.config?.scene?.lap_gate
    return {
      start: gate?.start ?? sceneStart,
      end: gate?.end ?? sceneEnd,
    }
  })

  function onLapGateChange(field, seconds) {
    const rounded = Math.round(seconds * 10) / 10
    // First touch materialises the whole gate at the bar's displayed defaults,
    // so the stored config always matches what the handles show.
    const creating = !app.config?.scene?.lap_gate
    app.updateLapGate(
      creating
        ? { start: sceneStart, end: sceneEnd, [field]: rounded }
        : { [field]: rounded },
    )
  }

  function onCourseMarkerDistanceChange(newM) {
    const id = app.selectedElementId
    const el = app.config?.elements?.find((e) => e.id === id)
    const marker = selectedCourseMarker
    if (!el || el.type !== 'plot' || !marker) return
    app.updateElement(id, {
      markers: (el.markers ?? []).map((m) =>
        m === marker || (marker.id && m.id === marker.id)
          ? { ...m, distance: newM }
          : m,
      ),
    })
  }
  // Base preview render size. The frame is shown in a viewport-sized box that's
  // usually far smaller than the export resolution, so rendering the full output
  // res wastes pixels the screen can't show — while rendering *below* the box's
  // device pixels looks soft on HiDPI displays. We render at the box's CSS width
  // × devicePixelRatio, capped at the export resolution. Zoom is intentionally
  // NOT a factor here: the base stays put and provides gap-free coverage, while
  // the separate crop layer (below) supersamples the visible window for crisp
  // zoomed text. Aspect ratio always follows the output.
  let zoom = $state(1)
  let dpr = $state(
    typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1,
  )
  let boxCssW = $state(0)
  let previewW = $derived.by(() => {
    const ow = app.outputWidth ?? 1920
    if (!boxCssW) return ow
    return Math.max(1, Math.min(ow, Math.round(boxCssW * dpr)))
  })
  let previewH = $derived.by(() => {
    const ow = app.outputWidth ?? 1920
    const oh = app.outputHeight ?? 1080
    return Math.max(1, Math.round(previewW * (oh / ow)))
  })
  // Canvas buffer + overlay coordinate space track the *currently displayed*
  // frame's render dims (returned by the backend), not the in-flight target.
  // While a zoom/resize re-render is in flight the old frame stays up, so using
  // the live target here would mis-size the canvas and misalign the overlay.
  // Falls back to the target for the very first paint, before any frame lands.
  let sceneW = $derived(currentFrameData?.width ?? previewW)
  let sceneH = $derived(currentFrameData?.height ?? previewH)
  // The box geometry follows the export aspect ratio so it never jumps while
  // the rendered frame size changes (frames always preserve the output aspect).
  let aspectRatio = $derived(
    (app.outputHeight ?? 1080) / (app.outputWidth ?? 1920),
  )
  let sceneInvalid = $derived(sceneEnd <= sceneStart)

  // Preview zoom/pan. Pinch or Ctrl+wheel zooms toward the cursor; two-finger
  // scroll pans while zoomed (trackpad-native, no conflict with the element-
  // drag pointer layer). transform-origin is 0 0 so the focal math is a clean
  // closed form. Safe for WYSIWYG editing — drag reads svg.getScreenCTM(),
  // which already accounts for ancestor transforms. Double-click resets.
  // (`zoom` is declared above, since the preview render size depends on it.)
  let panX = $state(0)
  let panY = $state(0)
  let stageEl = $state()

  // Track the preview box's on-screen CSS width so previewW/previewH reflect what
  // the screen can actually show. ResizeObserver reports layout (pre-transform)
  // size, so zoom doesn't perturb it. Quantized to 8px to avoid a SceneCache
  // rebuild on every sub-pixel of a window-resize drag.
  $effect(() => {
    if (!stageEl) return
    const apply = (w) => {
      if (w) boxCssW = Math.round(w / 8) * 8
    }
    const ro = new ResizeObserver((entries) => {
      for (const e of entries)
        apply(e.contentRect?.width ?? stageEl.offsetWidth)
    })
    ro.observe(stageEl)
    apply(stageEl.offsetWidth)
    return () => ro.disconnect()
  })

  // devicePixelRatio changes when the window moves between displays of different
  // density. matchMedia fires once per change, so re-register after each one.
  $effect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return
    let mql
    const update = () => {
      dpr = window.devicePixelRatio || 1
      mql?.removeEventListener('change', update)
      mql = window.matchMedia(`(resolution: ${dpr}dppx)`)
      mql.addEventListener('change', update)
    }
    update()
    return () => mql?.removeEventListener('change', update)
  })

  // ── Crisp-zoom crop layer ───────────────────────────────────────────────────
  // The full-frame base preview is CSS-upscaled when zoomed, so text softens past
  // the base render resolution. We additionally render just the *visible window*
  // of the scene, supersampled to device resolution, and lay it crisply over the
  // base. The base always covers the whole scene (gap-free during gestures); the
  // crop sharpens in once it lands. Reuses the base render cache (same target
  // dims) so zooming/panning never rebuilds the SceneCache.
  let clipEl = $state()
  let cropImage = $state(null)
  let cropBox = $state(null) // { left, top, width, height } as % of the stage box
  let cropVisible = $state(false)
  let cropGen = 0
  let cropDebounce = null
  const CROP_DEBOUNCE_MS = 140

  // Visible window of the scene → a render request, in base-scene px (for the
  // supersample source) + device px (for the output surface) + box-% (placement).
  function computeCropView() {
    if (!stageEl || !clipEl) return null
    const baseW = currentFrameData?.width
    const baseH = currentFrameData?.height
    if (!baseW || !baseH) return null
    const box = stageEl.getBoundingClientRect() // post-transform (zoom + pan)
    const clip = clipEl.getBoundingClientRect() // the clipping viewport
    if (box.width <= 0 || box.height <= 0) return null
    const left = Math.max(box.left, clip.left)
    const top = Math.max(box.top, clip.top)
    const right = Math.min(box.right, clip.right)
    const bottom = Math.min(box.bottom, clip.bottom)
    if (right <= left || bottom <= top) return null // nothing visible
    const nx0 = (left - box.left) / box.width
    const ny0 = (top - box.top) / box.height
    const nx1 = (right - box.left) / box.width
    const ny1 = (bottom - box.top) / box.height
    const outW = Math.round((right - left) * dpr)
    const outH = Math.round((bottom - top) * dpr)
    if (outW < 2 || outH < 2) return null
    return {
      baseW,
      baseH,
      view: {
        vx: nx0 * baseW,
        vy: ny0 * baseH,
        vw: (nx1 - nx0) * baseW,
        vh: (ny1 - ny0) * baseH,
        outW,
        outH,
      },
      box: {
        left: nx0 * 100,
        top: ny0 * 100,
        width: (nx1 - nx0) * 100,
        height: (ny1 - ny0) * 100,
      },
    }
  }

  async function fetchCrop() {
    const v = computeCropView()
    if (!v) {
      cropVisible = false
      return
    }
    const gen = ++cropGen
    const config = app.renderConfig
    const gpx = app.gpxFilename
    const fps = app.previewFps ?? 1
    const start = config?.scene?.start ?? 0
    const frameIdx = secToFrameIdx(
      Math.max(start, app.selectedSecond),
      fps,
      start,
    )
    try {
      const data = await backend.nativeGenerateDemoCrop(
        config,
        gpx,
        frameIdx,
        fps,
        v.baseW,
        v.baseH,
        v.view.vx,
        v.view.vy,
        v.view.vw,
        v.view.vh,
        v.view.outW,
        v.view.outH,
        app.riderWeightKg,
      )
      if (gen !== cropGen) return // superseded by a newer view
      if (data?.image) {
        cropImage = data.image
        cropBox = v.box
        cropVisible = true
      }
    } catch (e) {
      if (gen === cropGen) cropVisible = false
      console.debug('[crop] fetch failed', e)
    }
  }

  // Re-render the crop on zoom/pan (and when the base frame changes). Hide the
  // stale crop immediately so the gesture shows the gap-free base, then debounce
  // a fresh crop. Only active while zoomed in and paused.
  $effect(() => {
    const z = zoom
    void panX
    void panY
    const base = currentFrameData
    const active =
      z > 1 && !playing && !!base?.image && !sceneInvalid && app.hasActivity
    cropVisible = false // hide stale crop instantly → base (gap-free) shows through
    cropGen++ // invalidate any in-flight fetch
    if (cropDebounce) {
      clearTimeout(cropDebounce)
      cropDebounce = null
    }
    if (!active) return
    cropDebounce = setTimeout(() => {
      cropDebounce = null
      fetchCrop()
    }, CROP_DEBOUNCE_MS)
    return () => {
      if (cropDebounce) {
        clearTimeout(cropDebounce)
        cropDebounce = null
      }
    }
  })

  // Keep the scaled content overlapping the viewport so it can't be lost.
  function clampPan() {
    if (zoom <= 1) {
      panX = 0
      panY = 0
      return
    }
    const w = stageEl?.offsetWidth ?? 0
    const h = stageEl?.offsetHeight ?? 0
    const maxX = (zoom - 1) * w
    const maxY = (zoom - 1) * h
    panX = Math.min(0, Math.max(-maxX, panX))
    panY = Math.min(0, Math.max(-maxY, panY))
  }

  function onCanvasWheel(e) {
    if (!stageEl) return
    if (e.ctrlKey) {
      // Pinch / Ctrl+wheel → zoom toward the cursor.
      e.preventDefault()
      const next = Math.min(
        MAX_VIEWER_ZOOM,
        Math.max(1, zoom * Math.exp(-e.deltaY * 0.01)),
      )
      if (next === zoom) return
      const rect = stageEl.getBoundingClientRect()
      const ratio = 1 - next / zoom
      panX += (e.clientX - rect.left) * ratio
      panY += (e.clientY - rect.top) * ratio
      zoom = next
      clampPan()
    } else if (zoom > 1) {
      // Two-finger scroll → pan the zoomed view.
      e.preventDefault()
      panX -= e.deltaX
      panY -= e.deltaY
      clampPan()
    }
  }

  function resetZoom() {
    zoom = 1
    panX = 0
    panY = 0
  }

  // Clamp playhead when scene start/end changes.
  // Skip when the range is invalid — clamping to a negative sceneEnd would corrupt selectedSecond
  // and cause fetchFrame to silently bail out (frameIdx < 0 guard) even after the user fixes values.
  $effect(() => {
    const s = sceneStart
    const e = sceneEnd
    if (e <= s) return
    if (app.selectedSecond < s) app.selectedSecond = s
    else if (app.selectedSecond > e) app.selectedSecond = e
  })

  // Spacebar toggles playback (unless typing or focused on a button).
  function onKeydown(e) {
    if (e.code !== 'Space') return
    const t = e.target
    if (
      t?.tagName === 'INPUT' ||
      t?.tagName === 'TEXTAREA' ||
      t?.tagName === 'SELECT' ||
      t?.tagName === 'BUTTON' ||
      t?.isContentEditable
    )
      return
    if (!app.config || !app.hasActivity || sceneInvalid) return
    e.preventDefault()
    if (!playing && app.selectedSecond >= sceneEnd)
      app.selectedSecond = sceneStart
    playing = !playing
  }
</script>

<svelte:window onkeydown={onKeydown} />

<main
  class="flex-1 flex flex-col overflow-hidden bg-[var(--panel)] rounded-[10px] min-w-0"
>
  <!-- Zoom indicator — pinned above the canvas, visible regardless of pan position -->
  {#if app.config && zoom !== 1}
    <div
      class="shrink-0 flex items-center justify-end px-3 py-1 border-b border-white/[0.06]"
    >
      <button
        onclick={resetZoom}
        class="cursor-pointer text-[10px] font-mono text-zinc-400 hover:text-primary transition-colors"
        title="Reset zoom (or double-click the canvas)"
      >
        {Math.round(zoom * 100)}% · reset
      </button>
    </div>
  {/if}

  <!-- Canvas area (flexible height) -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={clipEl}
    class="flex-1 flex items-center justify-center p-6 overflow-hidden"
    style="background: radial-gradient(120% 90% at 50% 0%, rgba(220, 20, 60, 0.09), transparent 58%);"
    onwheel={onCanvasWheel}
    ondblclick={resetZoom}
  >
    {#if app.config && app.hasActivity}
      <!-- Aspect-ratio wrapper — always shown when a template is loaded -->
      <div
        bind:this={stageEl}
        class="relative shadow-[0_30px_60px_-20px_rgba(0,0,0,0.8)]"
        style={`width: min(100%, calc((100vh - 180px) / ${aspectRatio})); aspect-ratio: ${app.outputWidth ?? 1920} / ${app.outputHeight ?? 1080}; transform-origin: 0 0; transform: translate(${panX}px, ${panY}px) scale(${zoom});`}
      >
        <!-- Background -->
        <div
          class="absolute inset-0 rounded-lg overflow-hidden bg-[#0a0a0a]"
          style={currentFrameData?.image
            ? `background-image:
            linear-gradient(45deg, #1a1a1a 25%, transparent 25%),
            linear-gradient(-45deg, #1a1a1a 25%, transparent 25%),
            linear-gradient(45deg, transparent 75%, #1a1a1a 75%),
            linear-gradient(-45deg, transparent 75%, #1a1a1a 75%);
            background-size: 16px 16px;
            background-position: 0 0, 0 8px, 8px -8px, -8px 0px;`
            : ''}
        ></div>

        <!-- Reference video backdrop — driven by selectedSecond, hidden when out of range -->
        {#if app.videoUnderlayVisible}
          <VideoBackdrop {playing} />
        {/if}

        <!-- Rendered frame -->
        {#if sceneInvalid}
          <!-- Invalid timeline range — shown first so it always wins over stale frame/spinner -->
          <div
            class="absolute inset-0 flex flex-col items-center justify-center gap-2 px-6"
          >
            <p class="text-xs text-red-500 text-center">
              Fix the timeline range — start must be less than end
            </p>
          </div>
        {:else if currentFrameData?.image}
          <div class="absolute inset-0 rounded-lg overflow-hidden">
            <PreviewCanvas
              frameDataUrl={currentFrameData.image}
              sceneWidth={sceneW}
              sceneHeight={sceneH}
            />
          </div>
        {:else if fetchError}
          <!-- Error state -->
          <div
            class="absolute inset-0 flex flex-col items-center justify-center gap-3 px-6"
          >
            <svg
              class="h-5 w-5 text-red-500"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M12 9v2m0 4h.01M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"
              />
            </svg>
            <p class="text-xs text-red-400 text-center leading-relaxed">
              {fetchError}
            </p>
            <button
              class="inline-flex cursor-pointer items-center gap-1.5 text-xs text-zinc-200 bg-zinc-800/80 hover:bg-zinc-700 border border-zinc-600 hover:border-zinc-500 rounded px-3 py-1.5 transition-colors"
              onclick={reportPreviewIssue}
            >
              <Github size={13} />
              Report issue
            </button>
          </div>
        {:else}
          <!-- Generating preview -->
          <div
            class="absolute inset-0 flex flex-col items-center justify-center gap-2"
          >
            <svg
              class="h-5 w-5 text-zinc-600 animate-spin"
              viewBox="0 0 24 24"
              fill="none"
            >
              <circle
                class="opacity-25"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                stroke-width="4"
              />
              <path
                class="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
              />
            </svg>
            <p class="text-xs text-zinc-600">
              {previewNotice ?? 'Generating preview…'}
            </p>
          </div>
        {/if}

        <!-- Crisp-zoom crop: a supersampled render of just the visible window,
             laid over the base frame so text stays sharp while zoomed in. Sits
             above the base canvas and below the WYSIWYG overlay. Positioned by
             % of the stage box, so the box's own transform places + scales it. -->
        {#if cropVisible && cropImage && cropBox && !sceneInvalid}
          <img
            src={cropImage}
            alt=""
            draggable="false"
            class="absolute pointer-events-none select-none"
            style={`left:${cropBox.left}%; top:${cropBox.top}%; width:${cropBox.width}%; height:${cropBox.height}%;`}
          />
        {/if}

        {#if !fetchError && !sceneInvalid}
          <!-- WYSIWYG drag layer — only active while the preview is editable. -->
          <WysiwygLayer
            measuredElements={currentFrameData?.elements ?? []}
            frameImage={currentFrameData?.image ?? null}
            sceneWidth={sceneW}
            sceneHeight={sceneH}
            {zoom}
          />
        {/if}
      </div>
    {:else}
      <!-- Onboarding quick-start guide -->
      <div
        class="flex flex-col items-center justify-center gap-8 px-6 select-none"
      >
        <p
          class="text-[11px] font-semibold tracking-[0.22em] uppercase text-[var(--dim)]"
        >
          Quick start
        </p>
        <div class="flex items-stretch gap-[18px]">
          <!-- Step 1 — Choose a template -->
          <button
            onclick={() => {
              app.showTemplatePicker = true
            }}
            class="onboarding-card {quickStartStep === 1
              ? 'onboarding-card--active'
              : quickStartStep1Complete
                ? 'onboarding-card--complete'
                : 'border-white/[0.06] opacity-40'} w-[196px] rounded-2xl border bg-[var(--panel2)] p-[22px]
                   flex flex-col items-center gap-3.5 text-center transition-transform duration-200
                   hover:-translate-y-1 cursor-pointer"
          >
            <span
              class="onboarding-step-badge {quickStartStep === 1
                ? 'onboarding-step-badge--active'
                : quickStartStep1Complete
                  ? 'onboarding-step-badge--complete'
                  : 'border border-white/[0.06] bg-[var(--panel3)] text-zinc-500'}
                         w-[30px] h-[30px] rounded-full flex items-center justify-center text-[13px] font-bold"
            >
              {#if quickStartStep1Complete}
                <svg
                  class="w-3.5 h-3.5"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  stroke-width="3"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M5 13l4 4L19 7"
                  />
                </svg>
              {:else}
                1
              {/if}
            </span>
            <div class="flex flex-col gap-1">
              <p
                class="text-[15px] font-semibold {quickStartStep === 1 ||
                quickStartStep1Complete
                  ? 'text-zinc-100'
                  : 'text-zinc-400'}"
              >
                Choose a Template
              </p>
              <p
                class="text-xs {quickStartStep === 1 || quickStartStep1Complete
                  ? 'text-[var(--dim)]'
                  : 'text-zinc-600'} leading-relaxed"
              >
                Pick a layout for your overlay
              </p>
            </div>
            <!-- Grid icon -->
            <svg
              class="w-[30px] h-[30px] {quickStartStep === 1
                ? 'text-primary opacity-80'
                : quickStartStep1Complete
                  ? 'text-emerald-400/70'
                  : 'text-zinc-600'} mt-1"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="1.5"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                d="M3.75 6A2.25 2.25 0 016 3.75h2.25A2.25 2.25 0 0110.5 6v2.25a2.25 2.25 0 01-2.25 2.25H6a2.25 2.25 0 01-2.25-2.25V6zM3.75 15.75A2.25 2.25 0 016 13.5h2.25a2.25 2.25 0 012.25 2.25V18a2.25 2.25 0 01-2.25 2.25H6A2.25 2.25 0 013.75 18v-2.25zM13.5 6a2.25 2.25 0 012.25-2.25H18A2.25 2.25 0 0120.25 6v2.25A2.25 2.25 0 0118 10.5h-2.25a2.25 2.25 0 01-2.25-2.25V6zM13.5 15.75a2.25 2.25 0 012.25-2.25H18a2.25 2.25 0 012.25 2.25V18A2.25 2.25 0 0118 20.25h-2.25A2.25 2.25 0 0113.5 18v-2.25z"
              />
            </svg>
          </button>

          <!-- Step 2 — Load an Activity -->
          <button
            type="button"
            onclick={() => onopenactivity?.()}
            class="onboarding-card {quickStartStep === 2
              ? 'onboarding-card--active'
              : quickStartStep2Complete
                ? 'onboarding-card--complete'
                : 'border-white/[0.06] opacity-70 hover:opacity-100'} w-[196px] rounded-2xl border bg-[var(--panel2)]
                      p-[22px] flex flex-col items-center gap-3.5 text-center transition-[transform,opacity] duration-200
                      hover:-translate-y-1 cursor-pointer"
          >
            <span
              class="onboarding-step-badge {quickStartStep === 2
                ? 'onboarding-step-badge--active'
                : quickStartStep2Complete
                  ? 'onboarding-step-badge--complete'
                  : 'border border-white/[0.06] bg-[var(--panel3)] text-zinc-500'} w-[30px] h-[30px] rounded-full
                         flex items-center justify-center text-[13px] font-bold"
            >
              {#if quickStartStep2Complete}
                <svg
                  class="w-3.5 h-3.5"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  stroke-width="3"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M5 13l4 4L19 7"
                  />
                </svg>
              {:else}
                2
              {/if}
            </span>
            <div class="flex flex-col gap-1">
              <p
                class="text-[15px] font-semibold {quickStartStep === 2 ||
                quickStartStep2Complete
                  ? 'text-zinc-100'
                  : 'text-zinc-400'}"
              >
                Load Activity
              </p>
              <p
                class="text-xs {quickStartStep === 2 || quickStartStep2Complete
                  ? 'text-[var(--dim)]'
                  : 'text-zinc-600'} leading-relaxed"
              >
                Open a GPX, FIT, or TCX file
              </p>
            </div>
            <!-- Activity icon -->
            <svg
              class="w-[30px] h-[30px] {quickStartStep === 2
                ? 'text-primary opacity-80'
                : quickStartStep2Complete
                  ? 'text-emerald-400/70'
                  : 'text-zinc-600'} mt-1"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="1.5"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                d="M3 12h3l3-9 4 18 3-9h5"
              />
            </svg>
          </button>

          <!-- Step 3 — Render (dimmed) -->
          <div
            class="onboarding-card w-[196px] rounded-2xl border border-white/[0.06] bg-[var(--panel2)]
                      p-[22px] flex flex-col items-center gap-3.5 text-center opacity-40 cursor-default"
          >
            <span
              class="onboarding-step-badge w-[30px] h-[30px] rounded-full border border-white/[0.06] bg-[var(--panel3)]
                         flex items-center justify-center text-[13px] font-bold text-zinc-500"
              >3</span
            >
            <div class="flex flex-col gap-1">
              <p class="text-[15px] font-semibold text-zinc-400">
                Render Video
              </p>
              <p class="text-xs text-zinc-600 leading-relaxed">
                Export the overlay to a file
              </p>
            </div>
            <!-- Play icon -->
            <svg
              class="w-[30px] h-[30px] text-zinc-600 mt-1"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="1.5"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                d="M5.25 5.653c0-.856.917-1.398 1.667-.986l11.54 6.347a1.125 1.125 0 010 1.972l-11.54 6.347c-.75.412-1.667-.13-1.667-.986V5.653z"
              />
            </svg>
          </div>
        </div>
      </div>
    {/if}
  </div>

  <!-- Video alignment bar — always shown when a valid video is loaded -->
  {#if app.config && app.hasActivity && app.video && !app.video.missing}
    <VideoAlignmentBar />
  {/if}

  <!-- Playback controls — only shown once a template and activity are loaded -->
  {#if app.config && app.hasActivity}
    <PlaybackControls
      bind:playhead={app.selectedSecond}
      start={sceneStart}
      end={sceneEnd}
      bind:playing
      buffered={bufferedSeconds}
      outputDuration={playbackSpeedup > 1
        ? (app.config?.scene?.target_duration ?? null)
        : null}
      onseek={seek}
      distanceInfo={showDistanceBar || showCourseMarkerBar
        ? distanceInfo
        : null}
      customDistanceM={showDistanceBar ? customDistanceM : null}
      oncustomdistancechange={onCustomDistanceChange}
      {customTimeS}
      oncustomtimechange={onCustomTimeChange}
      markerDistanceM={showCourseMarkerBar ? courseMarkerDistanceM : null}
      markerStyle={selectedCourseMarker?.style ?? 'checkered'}
      markerColor={selectedCourseMarker?.color ?? '#ef4444'}
      onmarkerdistancechange={onCourseMarkerDistanceChange}
      lapGate={lapGateForBar}
      onlapgatechange={onLapGateChange}
    />
  {/if}
</main>
