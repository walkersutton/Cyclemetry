<script>
  import { onMount } from 'svelte'
  import { check } from '@tauri-apps/plugin-updater'
  import { relaunch } from '@tauri-apps/plugin-process'
  import { listen } from '@tauri-apps/api/event'

  // hidden | checking | available | downloading | done | up_to_date | error
  let status = $state('hidden')
  let update = $state(null)
  let progress = $state(0)

  async function doCheck({ silent = false } = {}) {
    if (import.meta.env.DEV) return
    if (!silent) status = 'checking'
    try {
      const result = await check()
      if (result?.available) {
        update = result
        status = 'available'
      } else if (!silent) {
        status = 'up_to_date'
        setTimeout(() => {
          if (status === 'up_to_date') status = 'hidden'
        }, 3000)
      } else {
        status = 'hidden'
      }
    } catch {
      if (!silent) {
        status = 'error'
      } else {
        status = 'hidden'
      }
    }
  }

  async function handleInstall() {
    if (!update) return
    status = 'downloading'
    progress = 0
    let downloaded = 0
    let total = 0
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          total = event.data.contentLength ?? 0
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength
          if (total > 0) progress = Math.round((downloaded / total) * 100)
        } else if (event.event === 'Finished') {
          status = 'done'
        }
      })
      status = 'done'
      setTimeout(() => relaunch(), 800)
    } catch {
      status = 'error'
    }
  }

  onMount(async () => {
    if (typeof window.__TAURI__ === 'undefined') return

    const unlisten = await listen('check_for_updates', () =>
      doCheck({ silent: false }),
    )

    // Silent auto-check on startup — only show banner if update found
    if (!import.meta.env.DEV) {
      await doCheck({ silent: true })
    }

    return unlisten
  })
</script>

{#if status !== 'hidden'}
  <div
    class="fixed bottom-4 right-4 z-[200] flex items-center gap-2.5 rounded-[6px] border border-zinc-800 bg-zinc-950 px-3 py-2 shadow-xl text-xs"
    role="status"
  >
    {#if status === 'checking'}
      <span class="h-1.5 w-1.5 rounded-full bg-zinc-600 shrink-0 animate-pulse"
      ></span>
      <span class="text-zinc-400">Checking for updates…</span>
    {:else if status === 'available'}
      <span class="h-1.5 w-1.5 rounded-full bg-[#22C55E] shrink-0"></span>
      <span class="text-zinc-300">v{update.version} available</span>
      <button
        onclick={handleInstall}
        class="ml-1 cursor-pointer font-medium text-[#22C55E] hover:text-white transition-colors duration-[150ms]"
        >Update &amp; Restart</button
      >
      <button
        onclick={() => (status = 'hidden')}
        class="ml-1 cursor-pointer text-zinc-600 hover:text-zinc-300 text-sm leading-none transition-colors duration-[150ms]"
        aria-label="Dismiss">×</button
      >
    {:else if status === 'downloading'}
      <span class="h-1.5 w-1.5 rounded-full bg-[#F59E0B] shrink-0 animate-pulse"
      ></span>
      <span class="text-zinc-300"
        >Downloading{progress > 0 ? ` ${progress}%` : '…'}</span
      >
    {:else if status === 'done'}
      <span class="h-1.5 w-1.5 rounded-full bg-[#22C55E] shrink-0"></span>
      <span class="text-zinc-300">Restarting…</span>
    {:else if status === 'up_to_date'}
      <span class="h-1.5 w-1.5 rounded-full bg-zinc-600 shrink-0"></span>
      <span class="text-zinc-400">You're up to date</span>
    {:else if status === 'error'}
      <span class="h-1.5 w-1.5 rounded-full bg-[#EF4444] shrink-0"></span>
      <span class="text-zinc-400">Update check failed</span>
      <button
        onclick={() => (status = 'hidden')}
        class="ml-1 cursor-pointer text-zinc-600 hover:text-zinc-300 text-sm leading-none transition-colors duration-[150ms]"
        aria-label="Dismiss">×</button
      >
    {/if}
  </div>
{/if}
