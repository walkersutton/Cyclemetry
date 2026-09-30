<script>
  import { onMount } from 'svelte'
  import { TriangleAlert } from 'lucide-svelte'
  import WindowDragStrip from './WindowDragStrip.svelte'
  import Switch from '../ui/Switch.svelte'
  import RiderWeightPrompt from '../ui/RiderWeightPrompt.svelte'
  import { formatFileSize } from '@/lib/utils.js'

  let {
    // [{ value, label, container, transparent, desc }] — available export formats.
    formats = [],
    // Preselected format (last used, persisted)
    initial = 'prores',
    // Preselected sizing for transparent exports (last used, persisted):
    // false = trim to overlay (crop + placement offset), true = full canvas.
    initialFullFrame = false,
    // (format) => "~1:20" | null — estimated render wall-clock time
    timeFor = null,
    // (format) => "~1.5 GB" | null — estimated output file size
    sizeFor = null,
    // (format) => bytes | null — same estimate, unformatted, for the
    // disk-space check below
    bytesFor = null,
    // Free bytes on the output volume, or null when unknown
    diskFreeBytes = null,
    // True when the template renders W/kg — with no rider weight set those
    // readouts export as 0, so offer the weight field before rendering.
    usesRiderWeight = false,
    // (format) => boolean — true when the codec has no same-machine
    // measurement yet, so it offers a quick test render
    testAvailableFor = null,
    // Format currently being measured by the test render, or null
    calibrating = null,
    // (format) => void — run the quick test render for a codec
    oncalibrate = null,
    onconfirm,
    oncancel,
  } = $props()

  let selected = $state(initial)
  let selectedFormat = $derived(formats.find((f) => f.value === selected))
  // Sizing only applies to transparent overlays; stitched always fills the frame.
  let fullFrame = $state(initialFullFrame)
  let showSizing = $derived(!!selectedFormat?.transparent)

  // Warn when the estimated file wouldn't fit while keeping the renderer's
  // low-disk reserve intact (it pauses the render below ~2 GB free — see
  // DISK_PAUSE_BYTES in render/scene.rs).
  const DISK_RESERVE_BYTES = 2 * 1024 ** 3
  let selectedBytes = $derived(bytesFor?.(selected) ?? null)
  let lowDisk = $derived(
    selectedBytes != null &&
      diskFreeBytes != null &&
      selectedBytes > diskFreeBytes - DISK_RESERVE_BYTES,
  )

  // Shared column layout so the header cells and every row align.
  const COLS = 'grid grid-cols-[1fr_6rem_5rem_4.5rem_5rem] items-center gap-2'

  onMount(() => {
    window.addEventListener('keydown', onKeydown)
    return () => window.removeEventListener('keydown', onKeydown)
  })

  function onKeydown(e) {
    // Enter inside a field (the rider-weight prompt) edits, it doesn't render.
    const t = e.target
    if (
      e.key === 'Enter' &&
      (t?.tagName === 'INPUT' || t?.tagName === 'SELECT')
    )
      return
    if (e.key === 'Escape') {
      e.preventDefault()
      oncancel?.()
    } else if (e.key === 'Enter') {
      e.preventDefault()
      onconfirm?.(selected, showSizing && fullFrame)
    }
  }
</script>

<div
  role="dialog"
  aria-modal="true"
  aria-label="Export settings"
  tabindex="-1"
  class="fixed inset-0 z-[60] flex items-center justify-center pt-14"
  onmousedown={(e) => {
    if (e.target === e.currentTarget) oncancel?.()
  }}
>
  <div class="absolute inset-0 bg-black/60 backdrop-blur-sm"></div>
  <WindowDragStrip />

  <div
    class="relative z-10 w-[600px] rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl p-5"
  >
    <p class="text-base font-semibold text-zinc-100">Export settings</p>

    <!-- Comparison table: transparency, render time, and file size side by
         side so the formats are easy to weigh against each other. -->
    <div
      class="mt-3 rounded-[6px] border border-zinc-700 overflow-hidden"
      role="radiogroup"
      aria-label="Export format"
    >
      <div
        class="{COLS} px-3 py-2 bg-zinc-800/60 border-b border-zinc-700
               text-xs font-medium uppercase tracking-wide text-zinc-500"
      >
        <span>Format</span>
        <span>Best for</span>
        <span class="text-center">Transparent</span>
        <span class="text-right">Est. time</span>
        <span class="text-right">Est. size</span>
      </div>

      {#each formats as f, i (f.value)}
        {@const active = selected === f.value}
        {@const time = timeFor?.(f.value) ?? null}
        {@const size = sizeFor?.(f.value) ?? null}
        {@const measuring = calibrating === f.value}
        {@const needsTest =
          time == null && size == null && testAvailableFor?.(f.value)}
        <div
          role="radio"
          aria-checked={active}
          tabindex="0"
          onclick={() => (selected = f.value)}
          onkeydown={(e) => {
            if (e.key === ' ') {
              e.preventDefault()
              selected = f.value
            }
          }}
          class="{COLS} px-3 py-2.5 cursor-pointer transition-colors {i > 0
            ? 'border-t border-zinc-800'
            : ''} {active
            ? 'bg-primary/10'
            : 'bg-zinc-800/20 hover:bg-zinc-800/50'}"
        >
          <span class="flex items-baseline gap-1.5 min-w-0">
            <span
              class="truncate text-xs font-medium {active
                ? 'text-zinc-50'
                : 'text-zinc-200'}">{f.label}</span
            >
            {#if f.container}
              <span class="shrink-0 font-mono text-xs text-zinc-600"
                >{f.container}</span
              >
            {/if}
          </span>

          <span
            class="truncate text-xs {active
              ? 'text-zinc-300'
              : 'text-zinc-400'}">{f.bestFor ?? ''}</span
          >

          <span
            class="text-center font-mono text-xs {f.transparent
              ? active
                ? 'text-zinc-300'
                : 'text-zinc-500'
              : 'text-zinc-600'}">{f.transparent ? 'Yes' : 'No'}</span
          >

          {#if measuring}
            <span
              class="col-span-2 text-right font-mono text-xs text-zinc-400 animate-pulse"
              >Testing…</span
            >
          {:else if needsTest}
            {#if calibrating}
              <span
                class="col-span-2 text-right font-mono text-xs text-zinc-500"
                >Queued…</span
              >
            {:else}
              <button
                type="button"
                onclick={(e) => {
                  e.stopPropagation()
                  oncalibrate?.(f.value)
                }}
                class="col-span-2 text-right font-mono text-xs text-zinc-400
                       underline decoration-zinc-600 underline-offset-2
                       hover:text-zinc-200 cursor-pointer transition-colors"
              >
                Test render →
              </button>
            {/if}
          {:else}
            <span
              class="text-right font-mono text-xs tabular-nums {active
                ? 'text-zinc-200'
                : 'text-zinc-400'}">{time ?? '—'}</span
            >
            <span
              class="text-right font-mono text-xs tabular-nums {active
                ? 'text-zinc-200'
                : 'text-zinc-400'}">{size ?? '—'}</span
            >
          {/if}
        </div>
      {/each}
    </div>

    <!-- Detail for the selected format — keeps the table rows scannable while
         still explaining what the choice means. -->
    {#if selectedFormat?.desc}
      <p class="mt-3 text-sm leading-relaxed text-zinc-400">
        {selectedFormat.desc}
      </p>
    {/if}

    <!-- Sizing (transparent overlays only): off crops to the overlay's bounding
         box with a placement offset for the editor; on keeps the full canvas so
         the clip drops onto footage at 0,0. Stitched fills the frame either way. -->
    {#if showSizing}
      <div
        class="mt-4 flex items-center justify-between gap-4 rounded-[6px] border border-zinc-700 bg-zinc-800/20 px-3 py-2.5"
      >
        <div class="min-w-0">
          <span class="block text-sm font-medium text-zinc-200"
            >Full-frame export</span
          >
          <span class="mt-0.5 block text-xs leading-snug text-zinc-400">
            {fullFrame
              ? 'Full canvas with transparent dead space — drops onto footage at 0,0.'
              : 'Cropped to the overlay — smaller file, with a placement offset for your editor.'}
          </span>
        </div>
        <Switch
          checked={fullFrame}
          ariaLabel="Full-frame export"
          onchange={(checked) => (fullFrame = checked)}
        />
      </div>
    {/if}

    <!-- Disk-space check: rough estimate vs. free space on the output volume.
         Starting anyway is fine — the renderer pauses itself before the disk
         fills and resumes once space is freed. -->
    {#if lowDisk}
      <div
        class="mt-4 flex items-start gap-2.5 rounded-[6px] border border-[var(--ds-warning)]/30 bg-[var(--ds-warning)]/10 px-3 py-2.5"
      >
        <TriangleAlert
          size={14}
          class="mt-0.5 shrink-0 text-[var(--ds-warning)]"
        />
        <p class="text-xs leading-snug text-zinc-300">
          This export needs roughly {formatFileSize(selectedBytes)}, but the
          output disk has only {formatFileSize(diskFreeBytes)} free. You can still
          start — the render pauses itself before the disk fills and resumes automatically
          when you free up space.
        </p>
      </div>
    {/if}

    <!-- W/kg with no rider weight: the export would bake in 0.0 for every such
         readout, so collect the weight here rather than after the render. -->
    {#if usesRiderWeight}
      <RiderWeightPrompt
        class="mt-4"
        message="This template shows W/kg, but no rider weight is set — those readouts will export as 0."
      />
    {/if}

    <div class="mt-4 flex justify-end gap-2">
      <button
        onclick={() => oncancel?.()}
        class="text-xs px-3 py-1.5 rounded border border-zinc-600 text-zinc-300
               hover:border-zinc-400 hover:text-zinc-100 cursor-pointer transition-colors"
      >
        Cancel
      </button>
      <button
        onclick={() => onconfirm?.(selected, showSizing && fullFrame)}
        class="text-xs px-3 py-1.5 rounded border border-primary/70 bg-primary/15 text-zinc-100
               hover:border-primary hover:bg-primary/25 cursor-pointer transition-colors"
      >
        Render
      </button>
    </div>
  </div>
</div>
