<script>
  import { onMount, tick } from 'svelte'
  import WindowDragStrip from './WindowDragStrip.svelte'

  let { oncreate, oncancel } = $props()

  let name = $state('my_overlay')
  let inputEl = $state(null)

  onMount(() => {
    window.addEventListener('keydown', onKeydown)
    tick().then(() => inputEl?.focus())
    return () => window.removeEventListener('keydown', onKeydown)
  })

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault()
      oncancel?.()
    }
  }

  function handleSubmit(e) {
    e?.preventDefault()
    const trimmed = name.trim()
    if (trimmed) oncreate?.(trimmed)
  }
</script>

<div
  role="dialog"
  aria-modal="true"
  aria-label="Create Template"
  tabindex="-1"
  class="fixed inset-0 z-[70] flex items-center justify-center pt-14"
  onmousedown={(e) => {
    if (e.target === e.currentTarget) oncancel?.()
  }}
>
  <div class="absolute inset-0 bg-black/60 backdrop-blur-sm"></div>
  <WindowDragStrip />

  <div
    class="relative z-10 w-[380px] rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl p-5"
  >
    <p class="text-sm font-semibold text-zinc-100">Create template</p>
    <form onsubmit={handleSubmit} class="mt-4">
      <input
        bind:this={inputEl}
        bind:value={name}
        type="text"
        placeholder="my_overlay"
        class="w-full rounded-[6px] border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 outline-none focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500"
      />
    </form>
    <div class="mt-4 flex justify-end gap-2">
      <button
        onclick={() => oncancel?.()}
        class="text-xs px-3 py-1.5 rounded border border-zinc-600 text-zinc-300
               hover:border-zinc-400 hover:text-zinc-100 transition-colors"
      >
        Cancel
      </button>
      <button
        onclick={handleSubmit}
        disabled={!name.trim()}
        class="text-xs px-3 py-1.5 rounded border border-zinc-600 text-zinc-100
               hover:border-zinc-400 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
      >
        Create
      </button>
    </div>
  </div>
</div>
