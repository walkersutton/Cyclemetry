<script>
  import { onMount } from 'svelte'
  import WindowDragStrip from './WindowDragStrip.svelte'

  let {
    title = 'Are you sure?',
    message = '',
    confirmText = 'Confirm',
    cancelText = 'Cancel',
    // When provided, a "don't show again" checkbox is rendered.
    // onconfirm receives a boolean — true if the box was checked at confirm time.
    dontShowAgainLabel = null,
    onconfirm,
    oncancel,
  } = $props()

  let dontShowAgain = $state(false)

  onMount(() => {
    window.addEventListener('keydown', onKeydown)
    return () => window.removeEventListener('keydown', onKeydown)
  })

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault()
      oncancel?.()
    } else if (e.key === 'Enter') {
      e.preventDefault()
      onconfirm?.(dontShowAgain)
    }
  }
</script>

<div
  role="dialog"
  aria-modal="true"
  aria-label={title}
  tabindex="-1"
  class="fixed inset-0 z-[60] flex items-center justify-center pt-14"
  onmousedown={(e) => {
    if (e.target === e.currentTarget) oncancel?.()
  }}
>
  <div class="absolute inset-0 bg-black/60 backdrop-blur-sm"></div>
  <WindowDragStrip />

  <div
    class="relative z-10 w-[380px] rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl p-5"
  >
    <p class="text-sm font-semibold text-zinc-100">{title}</p>
    {#if message}
      <p class="mt-2 text-xs text-zinc-400 leading-relaxed">{message}</p>
    {/if}

    {#if dontShowAgainLabel}
      <label
        class="mt-4 flex items-center gap-2 cursor-pointer select-none w-fit"
      >
        <input
          type="checkbox"
          bind:checked={dontShowAgain}
          class="w-3.5 h-3.5 rounded-[3px] border border-zinc-600 bg-zinc-800
                 accent-zinc-400 cursor-pointer"
        />
        <span class="text-[11px] text-zinc-500">{dontShowAgainLabel}</span>
      </label>
    {/if}

    <div class="mt-5 flex justify-end gap-2">
      <button
        onclick={() => oncancel?.()}
        class="text-xs px-3 py-1.5 rounded border border-zinc-600 text-zinc-300
               hover:border-zinc-400 hover:text-zinc-100 cursor-pointer transition-colors"
      >
        {cancelText}
      </button>
      <button
        onclick={() => onconfirm?.(dontShowAgain)}
        class="text-xs px-3 py-1.5 rounded border border-red-500/60 text-red-400
               hover:border-red-400 hover:text-red-300 cursor-pointer transition-colors"
      >
        {confirmText}
      </button>
    </div>
  </div>
</div>
