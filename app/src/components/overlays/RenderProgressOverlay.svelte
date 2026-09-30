<script>
  import { getContext } from 'svelte'
  import { formatTime } from '@/lib/utils.js'
  import WindowDragStrip from './WindowDragStrip.svelte'
  import * as backend from '@/api/backend.js'
  import Button from '@/components/ui/Button.svelte'
  import { CirclePause, CircleStop, Minimize2 } from 'lucide-svelte'

  // Full render-progress dialog. Not shown by default — the header
  // RenderStatusChip is the ambient status; this opens only when the user
  // expands it, and `onminimize` collapses back to the chip.
  let { expanded = false, format = null, onminimize } = $props()

  const app = getContext('app')

  let fps = $derived(app.config?.scene?.fps ?? 30)

  let cancelling = $state(false)

  $effect(() => {
    void app.renderingVideo
    cancelling = false
  })

  async function cancel() {
    cancelling = true
    try {
      await backend.nativeCancelRender()
    } catch (e) {
      console.error('Cancel failed:', e)
      cancelling = false
    }
  }

  let p = $derived(app.renderProgress)
  let pct = $derived(p.total > 0 ? Math.round((p.current / p.total) * 100) : 0)
  // Low disk space: the backend has stalled frame production (FFmpeg stays
  // alive, progress is preserved) and resumes on its own once space is freed.
  let paused = $derived(p.status === 'paused-low-disk')
  let finalizing = $derived(pct >= 100 && !paused)
  let estimating = $derived(
    !finalizing &&
      app.renderingVideo &&
      p.status === 'rendering' &&
      p.estimatedSecondsRemaining == null,
  )
</script>

{#if app.renderingVideo && expanded}
  <div
    class="fixed inset-0 z-[100] flex items-center justify-center bg-zinc-950/90 backdrop-blur-md"
  >
    <WindowDragStrip />

    <div
      class="relative z-10 w-full max-w-sm rounded-xl border border-zinc-800 bg-zinc-900 p-8 shadow-2xl space-y-5"
    >
      <!-- Icon -->
      <div class="flex flex-col items-center gap-3 text-center">
        {#if paused}
          <div
            class="relative w-14 h-14 rounded-full bg-[var(--ds-warning)]/10 flex items-center justify-center"
          >
            <CirclePause size={36} class="text-[var(--ds-warning)]" />
          </div>
          <div>
            <h2 class="text-xl font-semibold">Render Paused</h2>
            <p class="text-sm text-zinc-400 mt-1">
              The disk this video is being saved to is nearly full. Free up
              space and the render resumes automatically — progress so far is
              preserved.
            </p>
          </div>
        {:else}
          <div
            class="relative w-14 h-14 rounded-full bg-primary/10 flex items-center justify-center"
          >
            <svg
              class="h-9 w-9 text-primary animate-spin absolute"
              viewBox="0 0 24 24"
              fill="none"
            >
              <circle
                class="opacity-25"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                stroke-width="3"
              />
              <path
                class="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
              />
            </svg>
            <svg
              class="h-5 w-5 text-primary/50"
              fill="currentColor"
              viewBox="0 0 24 24"
            >
              <path d="M4 2l16 10L4 22V2z" />
            </svg>
          </div>
          <div>
            <h2 class="text-xl font-semibold">
              {finalizing ? 'Finalizing Video' : 'Generating Video'}
            </h2>
            <p class="text-sm text-zinc-400 mt-1">
              {finalizing
                ? 'Encoding output file…'
                : `${formatTime(p.overlaySecondsRendered)} / ${formatTime(p.overlayTotalSeconds)} of overlay rendered`}
            </p>
          </div>
        {/if}
      </div>

      <!-- Export format details -->
      {#if format}
        <div
          class="rounded-lg border border-zinc-800 bg-zinc-800/30 p-3 space-y-2"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm font-medium text-zinc-200">{format.label}</span
            >
            <span
              class="text-[11px] font-mono text-zinc-400 rounded border border-zinc-700 px-1.5 py-0.5"
              >{format.container}</span
            >
          </div>
          <p class="text-xs text-zinc-500 leading-relaxed">
            {format.summary ?? format.desc}
          </p>
          <div
            class="flex flex-wrap items-center gap-1.5 pt-0.5 text-[11px] font-mono text-zinc-400"
          >
            <span>{app.outputWidth}×{app.outputHeight}</span>
            <span class="text-zinc-700">·</span>
            <span>{fps} fps</span>
            <span class="text-zinc-700">·</span>
            <span>{format.transparent ? 'Transparent' : 'Opaque'}</span>
          </div>
        </div>
      {/if}

      <!-- Progress bar -->
      <div>
        <div class="flex justify-between text-xs mb-1.5">
          <span
            class="{paused
              ? 'text-[var(--ds-warning)]'
              : 'text-primary'} font-medium">{pct}%</span
          >
          {#if paused}
            <span class="text-[var(--ds-warning)] font-mono"
              >paused — low disk space</span
            >
          {:else if estimating}
            <span
              class="text-zinc-400 font-mono inline-flex items-center gap-1"
            >
              <span
                class="h-2 w-2 rounded-full border border-zinc-400 border-t-transparent animate-spin"
              ></span>
              estimating…
            </span>
          {:else if !finalizing && p.estimatedSecondsRemaining != null}
            <span class="text-zinc-400 font-mono"
              >{formatTime(p.estimatedSecondsRemaining)} remaining</span
            >
          {/if}
        </div>
        <div class="h-1.5 w-full rounded-full bg-zinc-800 overflow-hidden">
          <div
            class="h-full {paused
              ? 'bg-[var(--ds-warning)]'
              : 'bg-primary'} rounded-full transition-all duration-300"
            style={`width: ${pct}%`}
          ></div>
        </div>
      </div>

      <!-- Actions -->
      <div class="flex justify-center gap-2">
        <Button
          variant="outline"
          size="sm"
          onclick={() => onminimize?.()}
          class="min-w-28 border-zinc-700 text-zinc-300 hover:border-zinc-600 hover:bg-zinc-800"
        >
          <Minimize2 size={13} />
          Continue editing
        </Button>
        <Button
          variant="outline"
          size="sm"
          onclick={cancel}
          disabled={cancelling}
          class="min-w-28 border-zinc-700 text-zinc-300 hover:border-destructive/60 hover:bg-destructive/10 hover:text-destructive"
        >
          <CircleStop size={13} />
          {cancelling ? 'Cancelling…' : 'Cancel'}
        </Button>
      </div>

      <p class="text-xs text-center text-zinc-500 italic">
        Keep the app open during rendering
      </p>
    </div>
  </div>
{/if}
