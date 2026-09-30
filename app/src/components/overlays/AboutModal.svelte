<script>
  import { onMount } from 'svelte'
  import WindowDragStrip from './WindowDragStrip.svelte'
  import { getVersion } from '@tauri-apps/api/app'
  import { open } from '@tauri-apps/plugin-shell'
  import { X } from 'lucide-svelte'

  let { onclose } = $props()

  let version = $state('')

  onMount(async () => {
    version = await getVersion()
    window.addEventListener('keydown', onKeydown)
    return () => window.removeEventListener('keydown', onKeydown)
  })

  function onKeydown(e) {
    if (e.key === 'Escape') onclose()
  }
</script>

<!-- Backdrop -->
<div
  role="dialog"
  aria-modal="true"
  aria-label="About Cyclemetry"
  tabindex="-1"
  class="fixed inset-0 z-50 flex items-center justify-center pt-14"
>
  <div
    role="presentation"
    class="absolute inset-0 bg-black/60 backdrop-blur-sm"
    onmousedown={onclose}
  ></div>
  <WindowDragStrip />

  <!-- Panel -->
  <div
    class="relative z-10 w-72 rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl overflow-hidden"
  >
    <!-- Close -->
    <button
      onclick={onclose}
      class="absolute top-3 right-3 cursor-pointer text-zinc-600 hover:text-zinc-300 transition-colors rounded-md p-0.5"
      aria-label="Close"
    >
      <X size={14} />
    </button>

    <!-- Content -->
    <div class="flex flex-col items-center px-8 pt-10 pb-8 gap-3">
      <!-- Logo -->
      <img
        src="/logo192.png"
        alt="Cyclemetry"
        class="w-20 h-20 rounded-2xl shadow-lg"
      />

      <!-- Name + version -->
      <div class="flex flex-col items-center gap-1 mt-1">
        <h1 class="text-lg font-semibold tracking-tight text-zinc-50">
          Cyclemetry
        </h1>
        {#if version}
          <span class="text-xs font-mono text-zinc-500">v{version}</span>
        {/if}
      </div>

      <!-- Divider -->
      <div class="w-full h-px bg-zinc-800 my-1"></div>

      <!-- Description + links -->
      <div class="flex flex-col items-center gap-2 text-center">
        <p class="text-xs text-zinc-400 leading-relaxed">
          Cycling telemetry overlays<br />for your ride videos.
        </p>
        <button
          onclick={() => open('https://github.com/walkersutton/cyclemetry')}
          class="text-[11px] text-zinc-500 font-mono hover:text-zinc-300 transition-colors cursor-pointer"
          >github.com/walkersutton/cyclemetry</button
        >
      </div>

      <!-- Copyright -->
      <p class="text-[11px] text-zinc-600 mt-1">© 2025-2026 Walker Sutton</p>
    </div>
  </div>
</div>
