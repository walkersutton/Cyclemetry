<script>
  import { getContext } from 'svelte'
  import { ChevronDown, Plus, X } from 'lucide-svelte'
  import ElementList from '../panels/ElementList.svelte'
  import OpacityControl from '../ui/OpacityControl.svelte'
  import Select from '../ui/Select.svelte'

  const app = getContext('app')

  const fontGroup = (font) =>
    font.source === 'system' ? 'System fonts' : 'Font files'

  function fontOpts() {
    return app.fonts.map((font) => ({
      value: font.value,
      label: font.label,
      group: fontGroup(font),
    }))
  }

  function onSceneFont(v) {
    app.updateScene({ font: v })
  }

  // h:mm:ss when >= 1 hour, m:ss otherwise. Always whole seconds — overlay
  // bounds are scene-second granularity, sub-second precision is noise.
  function secToTimecode(s) {
    const whole = Math.round(s)
    const h = Math.floor(whole / 3600)
    const m = Math.floor((whole % 3600) / 60)
    const sec = whole % 60
    const ss = String(sec).padStart(2, '0')
    if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${ss}`
    return `${m}:${ss}`
  }

  // Accepts h:mm:ss, m:ss, or plain seconds. Always rounds to a whole
  // second to match secToTimecode's display granularity.
  function timecodeToSec(str) {
    str = str.trim()
    let raw = NaN
    if (/^\d+:\d{1,2}:\d{1,2}(?:\.\d+)?$/.test(str)) {
      const [h, m, s] = str.split(':').map(Number)
      raw = h * 3600 + m * 60 + s
    } else if (/^\d+:\d{1,2}(?:\.\d+)?$/.test(str)) {
      const [m, s] = str.split(':').map(Number)
      raw = m * 60 + s
    } else {
      const n = Number(str)
      raw = !isNaN(n) && n >= 0 ? n : NaN
    }
    return isNaN(raw) ? NaN : Math.round(raw)
  }

  let timelineCollapsed = $state(false)
  let sceneCollapsed = $state(false)

  // ── Time-lapse (compress the whole range into a short clip) ──────────────
  const TIMELAPSE_DEFAULT_SECS = 5

  let timelapseOn = $derived((app.config?.scene?.target_duration ?? 0) > 0)

  // Speed-up factor: how much faster than real time the clip plays.
  let timelapseSpeedup = $derived.by(() => {
    const s = app.config?.scene
    const td = s?.target_duration
    if (!(td > 0)) return null
    const start = s.start ?? 0
    const end = s.end ?? app.timelineDuration
    const win = end - start
    return win > 0 ? win / td : null
  })

  function toggleTimelapse(on) {
    app.updateScene({
      target_duration: on ? TIMELAPSE_DEFAULT_SECS : undefined,
    })
  }

  function setTargetDuration(v) {
    const n = Number(v)
    app.updateScene({
      target_duration: !isNaN(n) && n > 0 ? n : undefined,
    })
  }

  let timelineError = $derived.by(() => {
    const s = app.config?.scene
    if (!s) return null
    const start = s.start ?? 0
    const end = s.end ?? app.timelineDuration
    if (start >= end)
      return `Start must be before end (${secToTimecode(start)} ≥ ${secToTimecode(end)})`
    return null
  })

  // ── Mini GPX track (overlay-start / overlay-end handles) ────────────────
  let trackEl = $state(null)
  let drag = $state(null)

  function clamp(v, lo, hi) {
    return Math.min(hi, Math.max(lo, v))
  }

  function beginDrag(handle, e) {
    if (!trackEl) return
    e.preventDefault()
    trackEl.setPointerCapture(e.pointerId)
    app.beginEditBatch?.()
    const initial =
      handle === 'start'
        ? (app.config?.scene?.start ?? 0)
        : (app.config?.scene?.end ?? app.timelineDuration)
    drag = { handle, pointerId: e.pointerId, startX: e.clientX, initial }
  }

  function onTrackPointerMove(e) {
    if (!drag || e.pointerId !== drag.pointerId) return
    const total = Math.max(0.0001, app.timelineDuration ?? 0)
    const w = trackEl?.offsetWidth ?? 1
    const dxSec = ((e.clientX - drag.startX) / w) * total
    const next = Math.round(drag.initial + dxSec)
    const start = app.config?.scene?.start ?? 0
    const end = app.config?.scene?.end ?? total
    if (drag.handle === 'start') {
      app.updateScene({ start: clamp(next, 0, end - 1) })
    } else {
      app.updateScene({ end: clamp(next, start + 1, total) })
    }
  }

  function endTrackDrag(e) {
    if (!drag) return
    if (trackEl?.hasPointerCapture(e.pointerId)) {
      trackEl.releasePointerCapture(e.pointerId)
    }
    app.endEditBatch?.()
    drag = null
  }
</script>

<aside
  class="w-[284px] shrink-0 flex flex-col bg-[var(--panel)] rounded-[10px] overflow-hidden"
>
  {#if app.config?.scene}
    {#if app.hasActivity}
      <!-- Overlay timeline -->
      <section class="border-b border-white/[0.06]">
        <button
          onclick={() => (timelineCollapsed = !timelineCollapsed)}
          class="w-full flex items-center justify-between px-4 py-3 cursor-pointer group"
        >
          <span
            class="text-[10px] font-semibold uppercase tracking-wider text-zinc-500 group-hover:text-zinc-400 transition-colors duration-[150ms]"
          >
            Overlay Timeline
          </span>
          <ChevronDown
            size={12}
            class="text-zinc-600 group-hover:text-zinc-400 transition-all duration-[150ms] {timelineCollapsed
              ? '-rotate-90'
              : ''}"
          />
        </button>

        {#if !timelineCollapsed}
          <div class="px-4 pb-3 space-y-3">
            <div class="space-y-1.5">
              <div class="flex items-baseline justify-between">
                <span class="text-[11px] text-zinc-500">Range</span>
                <button
                  onclick={() => app.updateScene({ end: app.timelineDuration })}
                  title="Set end to activity duration"
                  class="text-[11px] text-zinc-600 hover:text-zinc-300 transition-colors duration-[150ms] tabular-nums"
                  >{secToTimecode(app.timelineDuration)} total</button
                >
              </div>

              <!-- Mini GPX track with overlay-start / overlay-end handles -->
              {#if app.timelineDuration > 0}
                {@const total = app.timelineDuration}
                {@const start = app.config.scene.start ?? 0}
                {@const end = app.config.scene.end ?? total}
                {@const startPct = (start / total) * 100}
                {@const endPct = (end / total) * 100}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  bind:this={trackEl}
                  class="relative h-5 select-none"
                  onpointermove={onTrackPointerMove}
                  onpointerup={endTrackDrag}
                  onpointercancel={endTrackDrag}
                >
                  <div
                    class="absolute inset-x-0 top-2 h-1 rounded-full bg-[var(--panel3)]"
                  ></div>
                  <div
                    class="absolute top-2 h-1 bg-primary/70 rounded-full"
                    style="left: {startPct}%; width: {endPct - startPct}%"
                  ></div>
                  <button
                    type="button"
                    aria-label="Overlay start"
                    onpointerdown={(e) => beginDrag('start', e)}
                    class="absolute top-0 -translate-x-1/2 h-5 w-3 rounded-sm
                     bg-primary border border-zinc-950 cursor-ew-resize
                     hover:scale-110 transition-transform"
                    style="left: {startPct}%"
                  ></button>
                  <button
                    type="button"
                    aria-label="Overlay end"
                    onpointerdown={(e) => beginDrag('end', e)}
                    class="absolute top-0 -translate-x-1/2 h-5 w-3 rounded-sm
                     bg-primary border border-zinc-950 cursor-ew-resize
                     hover:scale-110 transition-transform"
                    style="left: {endPct}%"
                  ></button>
                </div>
              {/if}

              <div class="flex gap-2 items-center">
                <input
                  type="text"
                  value={secToTimecode(app.config.scene.start ?? 0)}
                  placeholder="0:00"
                  onkeydown={(e) => {
                    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
                      e.preventDefault()
                      const next = Math.min(
                        Math.max(
                          0,
                          (app.config.scene.start ?? 0) +
                            (e.key === 'ArrowUp' ? 1 : -1),
                        ),
                        app.timelineDuration,
                      )
                      app.updateScene({ start: next })
                      e.target.value = secToTimecode(next)
                    }
                  }}
                  onchange={(e) => {
                    const v = timecodeToSec(e.target.value)
                    if (!isNaN(v))
                      app.updateScene({
                        start: Math.min(Math.max(0, v), app.timelineDuration),
                      })
                    else
                      e.target.value = secToTimecode(
                        app.config.scene.start ?? 0,
                      )
                  }}
                  class="h-7 w-full rounded-[6px] border bg-[var(--panel2)] px-2 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-ring font-mono
              {timelineError ? 'border-red-500' : 'border-transparent'}"
                />
                <span class="text-zinc-600 text-xs shrink-0">→</span>
                <input
                  type="text"
                  value={secToTimecode(
                    app.config.scene.end ?? app.timelineDuration,
                  )}
                  placeholder={secToTimecode(app.timelineDuration)}
                  onkeydown={(e) => {
                    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
                      e.preventDefault()
                      const next = Math.min(
                        Math.max(
                          0,
                          (app.config.scene.end ?? app.timelineDuration) +
                            (e.key === 'ArrowUp' ? 1 : -1),
                        ),
                        app.timelineDuration,
                      )
                      app.updateScene({ end: next })
                      e.target.value = secToTimecode(next)
                    }
                  }}
                  onchange={(e) => {
                    if (e.target.value.trim().toLowerCase() === 'end') {
                      app.updateScene({ end: app.timelineDuration })
                      e.target.value = secToTimecode(app.timelineDuration)
                      return
                    }
                    const v = timecodeToSec(e.target.value)
                    if (!isNaN(v))
                      app.updateScene({
                        end: Math.min(Math.max(0, v), app.timelineDuration),
                      })
                    else
                      e.target.value = secToTimecode(
                        app.config.scene.end ?? app.timelineDuration,
                      )
                  }}
                  class="h-7 w-full rounded-[6px] border bg-[var(--panel2)] px-2 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-ring font-mono
              {timelineError ? 'border-red-500' : 'border-transparent'}"
                />
              </div>
              {#if timelineError}
                <p class="text-[11px] text-red-500">{timelineError}</p>
              {/if}
            </div>

            <!-- Time-lapse: sweep the whole range into a short summary clip -->
            <div class="space-y-1.5 border-t border-white/[0.06] pt-3">
              <label
                class="flex items-center justify-between cursor-pointer group"
              >
                <span
                  class="text-[11px] text-zinc-500 group-hover:text-zinc-400 transition-colors duration-[150ms]"
                  >Time-lapse</span
                >
                <input
                  type="checkbox"
                  checked={timelapseOn}
                  onchange={(e) => toggleTimelapse(e.target.checked)}
                  class="h-3.5 w-3.5 rounded-sm accent-primary cursor-pointer"
                />
              </label>
              {#if timelapseOn}
                <div class="flex items-center gap-2">
                  <input
                    type="number"
                    min="0.5"
                    step="0.5"
                    inputmode="decimal"
                    value={app.config.scene.target_duration ??
                      TIMELAPSE_DEFAULT_SECS}
                    onchange={(e) => setTargetDuration(e.target.value)}
                    class="h-7 w-full rounded-[6px] border-0 bg-[var(--panel2)] px-2 text-xs
                       text-foreground font-mono focus:outline-none focus:ring-1 focus:ring-ring"
                  />
                  <span class="text-zinc-600 text-xs shrink-0">sec</span>
                </div>
                <p class="text-[10px] text-zinc-600">
                  {#if timelapseSpeedup}
                    Whole range compressed to {app.config.scene
                      .target_duration ?? TIMELAPSE_DEFAULT_SECS}s (≈{Math.round(
                      timelapseSpeedup,
                    )}× speed). Ignored for stitched exports.
                  {:else}
                    Compresses the whole range into a short clip.
                  {/if}
                </p>
              {/if}
            </div>
          </div>
        {/if}
      </section>
    {/if}

    <!-- Scene settings -->
    <section class="border-b border-white/[0.06]">
      <button
        onclick={() => (sceneCollapsed = !sceneCollapsed)}
        class="w-full flex items-center justify-between px-4 py-3 cursor-pointer group"
      >
        <span
          class="text-[10px] font-semibold uppercase tracking-wider text-zinc-500 group-hover:text-zinc-400 transition-colors duration-[150ms]"
        >
          Scene
        </span>
        <ChevronDown
          size={12}
          class="text-zinc-600 group-hover:text-zinc-400 transition-all duration-[150ms] {sceneCollapsed
            ? '-rotate-90'
            : ''}"
        />
      </button>

      {#if !sceneCollapsed}
        <div class="px-4 pb-3 space-y-3">
          <!-- Font (scene default — elements inherit unless overridden) -->
          <div class="space-y-1">
            <span class="text-[11px] text-zinc-500">Font</span>
            <Select
              value={app.config.scene.font ?? 'Arial.ttf'}
              options={fontOpts()}
              onchange={onSceneFont}
            />
          </div>

          <div class="space-y-1">
            <span class="text-[11px] text-zinc-500">Opacity</span>
            <OpacityControl
              value={app.config.scene.opacity ?? 1}
              oninput={(e) =>
                app.updateScene({ opacity: Number(e.target.value) })}
            />
          </div>

          <!-- Color variables -->
          <div class="space-y-1.5">
            <div class="flex items-center justify-between">
              <span class="text-[11px] text-zinc-500">Color variables</span>
              <button
                onclick={() => {
                  const vars = { ...(app.config.scene.vars ?? {}) }
                  let n = 1
                  while (vars[`color${n}`] !== undefined) n++
                  vars[`color${n}`] = '#ffffff'
                  app.updateScene({ vars })
                }}
                title="Add color variable"
                class="h-5 w-5 rounded flex items-center justify-center text-zinc-500 cursor-pointer
                   hover:text-zinc-200 hover:bg-[var(--panel3)] transition-colors"
              >
                <Plus size={12} />
              </button>
            </div>
            {#each Object.entries(app.config.scene.vars ?? {}) as [name, value] (name)}
              <div class="flex items-center gap-1.5">
                <input
                  type="color"
                  {value}
                  oninput={(e) => {
                    const vars = { ...(app.config.scene.vars ?? {}) }
                    vars[name] = e.target.value
                    app.updateScene({ vars })
                  }}
                  class="h-7 w-7 shrink-0 rounded-[4px] border border-white/10 bg-transparent cursor-pointer p-0.5"
                  title={value}
                />
                <input
                  type="text"
                  value={name}
                  onchange={(e) => {
                    const newName = e.target.value.trim().replace(/\s+/g, '_')
                    if (!newName || newName === name) {
                      e.target.value = name
                      return
                    }
                    const entries = Object.entries(app.config.scene.vars ?? {})
                    const vars = Object.fromEntries(
                      entries.map(([k, v]) => [k === name ? newName : k, v]),
                    )
                    app.updateScene({ vars })
                  }}
                  class="min-w-0 flex-1 h-7 rounded-[6px] border-0 bg-[var(--panel2)] px-2 text-xs
                     text-zinc-300 font-mono focus:outline-none focus:ring-1 focus:ring-ring"
                  title="Variable name — reference as ${name} in any color field"
                />
                <button
                  onclick={() => {
                    const vars = Object.fromEntries(
                      Object.entries(app.config.scene.vars ?? {}).filter(
                        ([k]) => k !== name,
                      ),
                    )
                    app.updateScene({ vars })
                  }}
                  title="Remove variable"
                  class="h-5 w-5 shrink-0 rounded flex items-center justify-center text-zinc-600 cursor-pointer
                     hover:text-zinc-300 hover:bg-[var(--panel3)] transition-colors"
                >
                  <X size={11} />
                </button>
              </div>
            {/each}
            {#if Object.keys(app.config.scene.vars ?? {}).length === 0}
              <p class="text-[10px] text-zinc-600">
                No variables. Add one above and reference it as $name in any
                color field.
              </p>
            {/if}
          </div>
        </div>
      {/if}
    </section>
  {/if}

  <ElementList />
</aside>
