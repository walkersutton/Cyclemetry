<script>
  import { formatTime, TOOLTIP_DELAY } from '@/lib/utils.js'
  import { Play, Pause, SkipBack, SkipForward } from 'lucide-svelte'
  import Tooltip from '@/components/ui/Tooltip.svelte'

  let {
    playhead = $bindable(0),
    start = 0, // overlay window start (sceneStart)
    end = 1, // overlay window end (sceneEnd)
    playing = $bindable(false),
    buffered = [], // array of seconds that are ready in cache
    // Time-lapse output length in seconds. When set, the readout shows the
    // exported clip's time (the whole window compressed into this many seconds)
    // instead of ride-time, so the preview matches the export.
    outputDuration = null,
    onseek,
    distanceInfo = null, // { total_m, overlay_start_m, overlay_end_m }
    customDistanceM = null, // current custom reference point in metres
    oncustomdistancechange,
    customTimeS = null, // current custom time reference point in overlay-relative seconds
    oncustomtimechange,
    markerDistanceM = null, // selected course marker position in metres
    markerStyle = 'checkered',
    markerColor = '#ef4444',
    onmarkerdistancechange,
    // Race window for lap counting: { start, end } in ride seconds, or null
    // when no lap-metric element is selected. Rendered as a second playback
    // bar with two handles — race start (green) and race finish (checkered).
    lapGate = null,
    onlapgatechange,
  } = $props()

  // Scrubbing is bounded to the overlay render window — the bar's left
  // edge IS sceneStart, right edge IS sceneEnd. Visualizing the overlay's
  // position within the broader activity happens on the LeftSidebar mini
  // GPX track and the VideoAlignmentBar.
  function seek(s) {
    playhead = Math.max(start, Math.min(s, end))
    onseek?.(playhead)
  }

  function stepBack() {
    seek(Math.max(start, Math.floor(playhead) - 1))
  }
  function stepForward() {
    seek(Math.min(end, Math.floor(playhead) + 1))
  }

  function onScrub(e) {
    seek(parseFloat(e.target.value))
  }

  function onDistanceScrub(e) {
    oncustomdistancechange?.(parseFloat(e.target.value))
  }

  function onTimeScrub(e) {
    oncustomtimechange?.(parseFloat(e.target.value))
  }

  function onMarkerScrub(e) {
    onmarkerdistancechange?.(parseFloat(e.target.value))
  }

  // Race handles clamp against each other (start stays before finish) and
  // scrub the preview to the dragged moment so the user can see themselves
  // cross the line while positioning.
  function onLapStartScrub(e) {
    const v = Math.min(parseFloat(e.target.value), (lapGate?.end ?? end) - 1)
    onlapgatechange?.('start', v)
    seek(v)
  }

  function onLapEndScrub(e) {
    const v = Math.max(
      parseFloat(e.target.value),
      (lapGate?.start ?? start) + 1,
    )
    onlapgatechange?.('end', v)
    seek(v)
  }

  let lapStartPct = $derived(
    lapGate && duration > 0 ? ((lapGate.start - start) / duration) * 100 : 0,
  )
  let lapEndPct = $derived(
    lapGate && duration > 0 ? ((lapGate.end - start) / duration) * 100 : 100,
  )

  let duration = $derived(end - start)

  // Timecode readout. On a time-lapse, map the ride-time playhead onto the
  // exported clip's timeline (0 → outputDuration) so it reads e.g. 0:03 / 0:05.
  let readoutTotal = $derived(outputDuration ?? duration)
  let readoutCurrent = $derived(
    outputDuration != null && duration > 0
      ? ((playhead - start) / duration) * outputDuration
      : playhead - start,
  )

  // ── Visual smoothing for the scrub thumb ────────────────────────────────
  // During playback the upstream `playhead` updates in coarse hops:
  // ~4 Hz when the video is master (HTML5 `timeupdate` is sparse), and
  // ~60 Hz otherwise. The frame cache + prefetch want truth-driven
  // `playhead`, but the slider thumb feels jumpy without between-update
  // interpolation. `smoothPlayhead` advances at vsync, clamped to a small
  // window around truth so it never runs ahead during a stall or lags
  // behind during a scrub.
  let smoothPlayhead = $state(playhead)
  $effect(() => {
    if (!playing) {
      smoothPlayhead = playhead
      return
    }
    let lastTick = performance.now()
    let raf = requestAnimationFrame(function step(now) {
      const dt = (now - lastTick) / 1000
      lastTick = now
      let next = smoothPlayhead + dt
      // Snap to truth on big jumps (user scrubbed mid-play).
      if (Math.abs(playhead - next) > 1.0) next = playhead
      // Cap how far ahead/behind of truth the display can wander, so a
      // video stall or burst doesn't desync the visual from the audio.
      next = Math.min(next, playhead + 0.3)
      next = Math.max(next, playhead - 0.5)
      // Don't visually overshoot the end of the overlay window.
      if (next > end) next = end
      smoothPlayhead = next
      raf = requestAnimationFrame(step)
    })
    return () => cancelAnimationFrame(raf)
  })

  let pct = $derived(
    duration > 0 ? ((smoothPlayhead - start) / duration) * 100 : 0,
  )

  // Distance bar derived values. The bar spans the overlay window's
  // distance range — overlay_start_m on the left, overlay_end_m on the
  // right. Visualizing the broader activity context isn't useful here; we
  // only set reference points that the value element renders inside the
  // overlay.
  // Reference-bar bounds. The thumb position itself is driven natively by the
  // input's value/min/max; these only feed the min/max attributes below.
  let distOverlayStart = $derived(distanceInfo?.overlay_start_m ?? 0)
  let distOverlayEnd = $derived(distanceInfo?.overlay_end_m ?? 1)
  let markerShapeClass = $derived(
    markerStyle === 'circle'
      ? 'marker-circle'
      : markerStyle === 'rectangle'
        ? 'marker-rectangle'
        : 'marker-checkered',
  )
  let markerCss = $derived(`--cm-thumb: ${markerColor || '#ef4444'}`)
</script>

<div class="flex flex-col gap-2 px-5 pt-3.5 pb-4 border-t border-white/[0.06]">
  <!-- Scrub bar with buffered indicator -->
  <div class="relative h-5 flex items-center">
    <!-- Buffered ranges (visual only) -->
    <div
      class="absolute inset-x-0 h-1 rounded-full bg-[var(--panel3)] overflow-hidden"
    >
      {#each buffered as s (s)}
        <div
          class="absolute h-full bg-zinc-600/50 w-[2px]"
          style={`left: ${duration > 0 ? ((s - start) / duration) * 100 : 0}%`}
        ></div>
      {/each}
    </div>
    <!-- Range input. `value` is bound to the smoothed display playhead so
         the thumb moves at vsync rather than in 250 ms timeupdate hops.
         `step` is sub-second so the thumb can sit anywhere on the bar —
         step=1 would re-quantize it back to integer-second hops, defeating
         the smoothing. The timecode display still rounds to whole seconds
         via secToTimecode/formatTime, so display granularity is unaffected.
         Scrubs still write through to `playhead` via onScrub. -->
    <input
      type="range"
      min={start}
      max={end}
      step={0.01}
      value={smoothPlayhead}
      oninput={onScrub}
      style="--pct: {pct}%"
      class="cm-slider cm-slider--filled absolute inset-x-0 h-full w-full"
    />
  </div>

  <!-- Distance reference bar — visible only when a distance element with reference='until_custom' is selected.
       Bar spans the overlay window's distance range; the whole bar IS the overlay. -->
  {#if distanceInfo && customDistanceM !== null}
    <div class="relative h-5 flex items-center">
      <div class="relative w-full h-full flex items-center">
        <div
          class="absolute inset-x-0 h-1 rounded-full bg-[var(--panel3)]"
        ></div>
        <input
          type="range"
          min={distOverlayStart}
          max={distOverlayEnd}
          step={10}
          value={customDistanceM}
          oninput={onDistanceScrub}
          style="--cm-thumb: #f59e0b"
          class="cm-slider absolute inset-x-0 h-full w-full"
          title="Custom distance reference: {customDistanceM >= 1000
            ? (customDistanceM / 1000).toFixed(1) + ' km'
            : Math.round(customDistanceM) + ' m'}"
        />
      </div>
    </div>
  {/if}

  <!-- Time reference bar — visible when a time element with reference='until_custom'/'since_custom' is selected.
       Bar spans 0 → overlay duration in seconds. -->
  {#if customTimeS !== null}
    <div class="relative h-5 flex items-center">
      <div class="relative w-full h-full flex items-center">
        <div
          class="absolute inset-x-0 h-1 rounded-full bg-[var(--panel3)]"
        ></div>
        <input
          type="range"
          min={0}
          max={duration}
          step={1}
          value={customTimeS}
          oninput={onTimeScrub}
          style="--cm-thumb: #10b981"
          class="cm-slider absolute inset-x-0 h-full w-full"
          title="Custom time reference: {formatTime(customTimeS)}"
        />
      </div>
    </div>
  {/if}

  <!-- Race bar — visible when a lap-metric element is selected. Two handles
       on one track: race start (green, defines the start/finish line's
       position) and race finish (checkered). The inputs are stacked with
       pointer-events routed to the thumbs only, so each handle drags
       independently; dragging also scrubs the preview to that moment. -->
  {#if lapGate}
    <div class="relative h-5 flex items-center">
      <div class="relative w-full h-full flex items-center">
        <div
          class="absolute inset-x-0 h-1 rounded-full bg-[var(--panel3)]"
        ></div>
        <!-- Race window highlight between the handles -->
        <div
          class="absolute h-1 rounded-full bg-emerald-500/25"
          style="left: {Math.max(
            0,
            Math.min(lapStartPct, 100),
          )}%; width: {Math.max(
            0,
            Math.min(lapEndPct, 100) - Math.max(0, Math.min(lapStartPct, 100)),
          )}%"
        ></div>
        <input
          type="range"
          min={start}
          max={end}
          step={0.1}
          value={Math.max(start, Math.min(lapGate.start, end))}
          oninput={onLapStartScrub}
          style="--cm-thumb: #22c55e"
          class="cm-slider lap-handle absolute inset-x-0 h-full w-full"
          title="Race start: {formatTime(
            lapGate.start - start,
          )} — drag to the moment you first cross the line"
          aria-label="Race start"
        />
        <input
          type="range"
          min={start}
          max={end}
          step={0.1}
          value={Math.max(start, Math.min(lapGate.end, end))}
          oninput={onLapEndScrub}
          class="cm-slider lap-handle marker-checkered absolute inset-x-0 h-full w-full"
          title="Race finish: {formatTime(
            lapGate.end - start,
          )} — drag to the final crossing"
          aria-label="Race finish"
        />
      </div>
    </div>
  {/if}

  <!-- Course marker bar — visible when editing a selected map marker.
       Same axis as the distance bar. -->
  {#if distanceInfo && markerDistanceM !== null}
    <div class="relative h-5 flex items-center">
      <div class="relative w-full h-full flex items-center">
        <div
          class="absolute inset-x-0 h-1 rounded-full bg-[var(--panel3)]"
        ></div>
        <input
          type="range"
          min={distOverlayStart}
          max={distOverlayEnd}
          step={10}
          value={markerDistanceM}
          oninput={onMarkerScrub}
          style={markerCss}
          class="cm-slider {markerShapeClass} absolute inset-x-0 h-full w-full"
          title="Course marker: {markerDistanceM >= 1000
            ? (markerDistanceM / 1000).toFixed(1) + ' km'
            : Math.round(markerDistanceM) + ' m'}"
        />
      </div>
    </div>
  {/if}

  <!-- Controls row -->
  <div class="relative flex items-center justify-center">
    <div class="flex items-center gap-2">
      <Tooltip content="−1 second" side="top" delay={TOOLTIP_DELAY}>
        <button
          onclick={stepBack}
          class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-full text-zinc-400 transition-all duration-[80ms] hover:bg-[var(--panel2)] hover:text-zinc-100 active:scale-90"
          aria-label="Step back 1 second"
        >
          <SkipBack size={14} />
        </button>
      </Tooltip>

      <button
        onclick={() => (playing = !playing)}
        class="flex h-[38px] w-[38px] shrink-0 cursor-pointer items-center justify-center rounded-full bg-white text-[#050505] transition-transform duration-[80ms] hover:scale-105 active:scale-95"
        aria-label={playing ? 'Pause' : 'Play'}
      >
        {#if playing}
          <Pause size={17} fill="currentColor" strokeWidth={0} />
        {:else}
          <Play
            size={17}
            fill="currentColor"
            strokeWidth={0}
            class="translate-x-[1px]"
          />
        {/if}
      </button>

      <Tooltip content="+1 second" side="top" delay={TOOLTIP_DELAY}>
        <button
          onclick={stepForward}
          class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-full text-zinc-400 transition-all duration-[80ms] hover:bg-[var(--panel2)] hover:text-zinc-100 active:scale-90"
          aria-label="Step forward 1 second"
        >
          <SkipForward size={14} />
        </button>
      </Tooltip>
    </div>

    <span
      class="absolute right-0 font-mono text-[11px] text-[var(--dim)] tabular-nums"
    >
      {formatTime(readoutCurrent)} / {formatTime(readoutTotal)}
    </span>
  </div>
</div>

<style>
  /* Course-marker thumb shape variants. These ride on top of the shared
     `.cm-slider` grip (global index.css) and only override its shape so the
     thumb previews the actual on-screen marker style. Color and border come
     from the shared grip via `--cm-thumb`. The Svelte scope class raises
     specificity above the global single-class rule, so these win. */
  .marker-circle::-webkit-slider-thumb {
    width: 14px;
    height: 14px;
    margin-top: -6px;
    border-radius: 50%;
  }
  .marker-checkered::-webkit-slider-thumb {
    width: 16px;
    height: 12px;
    margin-top: -5px;
    border-radius: 2px;
    background-color: #fff;
    background-image:
      linear-gradient(45deg, #111 25%, transparent 25%),
      linear-gradient(-45deg, #111 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, #111 75%),
      linear-gradient(-45deg, transparent 75%, #111 75%);
    background-size: 8px 8px;
    background-position:
      0 0,
      0 4px,
      4px -4px,
      -4px 0;
  }
  .marker-rectangle::-webkit-slider-thumb {
    width: 16px;
    height: 12px;
    margin-top: -5px;
    border-radius: 2px;
  }

  /* Dual-handle race bar: two range inputs share one track, so the inputs
     themselves must not swallow pointer events — only their thumbs do.
     Otherwise the top input would capture every click on the bar. */
  .lap-handle {
    pointer-events: none;
  }
  .lap-handle::-webkit-slider-thumb {
    pointer-events: auto;
  }
</style>
