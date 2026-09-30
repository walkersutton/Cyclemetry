<script>
  import { onMount } from 'svelte'
  import WindowDragStrip from './WindowDragStrip.svelte'
  import { Check, Upload, X } from 'lucide-svelte'
  import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
  import { listAssets, importAsset } from '../../api/backend.js'
  import { dialogExtensions } from '../../lib/utils.js'

  let { current = '', onselect, oncancel } = $props()

  let assets = $state([])
  let loading = $state(true)
  let uploading = $state(false)
  let uploadError = $state(null)

  onMount(() => {
    load()
    window.addEventListener('keydown', onKeydown)
    return () => window.removeEventListener('keydown', onKeydown)
  })

  async function load() {
    loading = true
    try {
      assets = await listAssets()
    } finally {
      loading = false
    }
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault()
      oncancel?.()
    }
  }

  async function upload() {
    uploadError = null
    const path = await openFileDialog({
      multiple: false,
      filters: [
        {
          name: 'Images',
          extensions: dialogExtensions(['png', 'webp', 'svg']),
        },
      ],
    })
    if (!path) return
    uploading = true
    try {
      await importAsset(path)
      await load()
    } catch (e) {
      uploadError = e?.message ?? String(e)
    } finally {
      uploading = false
    }
  }
</script>

<div
  role="dialog"
  aria-modal="true"
  aria-label="Asset Library"
  tabindex="-1"
  class="fixed inset-0 z-[60] flex items-center justify-center pt-14"
  onmousedown={(e) => {
    if (e.target === e.currentTarget) oncancel?.()
  }}
>
  <div class="absolute inset-0 bg-black/60 backdrop-blur-sm"></div>
  <WindowDragStrip />

  <div
    class="relative z-10 flex flex-col w-[520px] max-h-[72vh] rounded-[14px] border border-white/[0.08] bg-[var(--panel)] shadow-2xl"
  >
    <!-- Header -->
    <div
      class="flex items-center justify-between px-4 py-3 border-b border-zinc-800 shrink-0"
    >
      <p class="text-sm font-semibold text-zinc-100">Asset Library</p>
      <div class="flex items-center gap-2">
        {#if uploadError}
          <span
            class="text-[10px] text-red-400 max-w-[180px] truncate"
            title={uploadError}>{uploadError}</span
          >
        {/if}
        <button
          onclick={upload}
          disabled={uploading}
          class="flex items-center gap-1.5 px-2.5 py-1.5 rounded-[6px] text-xs font-medium
                 border border-zinc-700 text-zinc-300 hover:border-zinc-500 hover:text-zinc-100
                 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          <Upload size={11} />
          {uploading ? 'Importing…' : 'Import'}
        </button>
        <button
          onclick={() => oncancel?.()}
          class="p-1 rounded text-zinc-500 hover:text-zinc-200 transition-colors"
          title="Close"
        >
          <X size={15} />
        </button>
      </div>
    </div>

    <!-- Asset grid -->
    <div class="flex-1 overflow-y-auto p-3 min-h-0">
      {#if loading}
        <div class="flex items-center justify-center py-12">
          <p class="text-xs text-zinc-500">Loading…</p>
        </div>
      {:else if assets.length === 0}
        <div class="flex flex-col items-center justify-center py-12 gap-2">
          <p class="text-xs text-zinc-500">No assets yet.</p>
          <p class="text-[10px] text-zinc-600">
            Click Import to add PNG, WebP, or SVG files.
          </p>
        </div>
      {:else}
        <div class="grid grid-cols-4 gap-2">
          {#each assets as asset (asset.name)}
            {@const isSelected = asset.name === current}
            <button
              onclick={() => onselect?.(asset.name)}
              title={asset.name}
              class={`relative flex flex-col items-center gap-1.5 p-2 rounded-[6px] border text-left transition-colors
                ${
                  isSelected
                    ? 'border-primary/60 bg-primary/10'
                    : 'border-zinc-800 bg-zinc-900/60 hover:border-zinc-600 hover:bg-zinc-800/60'
                }`}
            >
              <!-- Thumbnail with checkerboard background -->
              <div
                class="w-full aspect-square rounded flex items-center justify-center overflow-hidden"
                style="background-image: linear-gradient(45deg,#2a2a2a 25%,transparent 25%),linear-gradient(-45deg,#2a2a2a 25%,transparent 25%),linear-gradient(45deg,transparent 75%,#2a2a2a 75%),linear-gradient(-45deg,transparent 75%,#2a2a2a 75%);background-size:10px 10px;background-position:0 0,0 5px,5px -5px,-5px 0;background-color:#1a1a1a;"
              >
                {#if asset.data_url}
                  <img
                    src={asset.data_url}
                    alt={asset.name}
                    class="max-w-full max-h-full object-contain"
                  />
                {/if}
              </div>

              <!-- Filename -->
              <span
                class="w-full text-[9px] text-center truncate text-zinc-500 leading-tight"
                >{asset.name}</span
              >

              <!-- Selected checkmark -->
              {#if isSelected}
                <div
                  class="absolute top-1.5 right-1.5 w-4 h-4 rounded-full bg-primary flex items-center justify-center"
                >
                  <Check size={9} class="text-white" strokeWidth={3} />
                </div>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Footer -->
    <div class="px-4 py-2.5 border-t border-zinc-800 shrink-0">
      <p class="text-[10px] text-zinc-600">
        PNG, WebP, and SVG. Imported assets are saved to your local library.
      </p>
    </div>
  </div>
</div>
