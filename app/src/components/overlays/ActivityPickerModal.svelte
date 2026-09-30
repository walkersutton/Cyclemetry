<script>
  import { getContext, onMount } from 'svelte'
  import WindowDragStrip from './WindowDragStrip.svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import * as backend from '@/api/backend.js'
  import {
    X,
    Trash2,
    FolderOpen,
    Activity,
    ChevronLeft,
    ChevronRight,
  } from 'lucide-svelte'
  import Tooltip from '@/components/ui/Tooltip.svelte'
  import { dialogExtensions } from '@/lib/utils.js'

  const app = getContext('app')
  let { onload, onclose } = $props()

  let saved = $state([])
  let loading = $state(true)
  let loadError = $state(null)
  let deleting = $state([])
  let opening = $state(false)
  let sortMode = $state('recent')

  // Strava import is fully built (backend + UI below) but hidden until the
  // API application is active again — Strava requires the app owner to hold a
  // paid subscription, and without it every call 403s. Flip to true once the
  // app is reactivated at strava.com/settings/api.
  const STRAVA_ENABLED = false

  // Strava is 10 activities per page (matches the backend's per_page) so a
  // single modal open costs one API request against the rate limit.
  let strava = $state({ connected: false, athlete: null })
  let stravaActivities = $state([])
  let stravaPage = $state(1)
  let stravaLoading = $state(false)
  let stravaConnecting = $state(false)
  let stravaError = $state(null)
  let importingId = $state(null)
  let stravaHasMore = $state(false)

  onMount(() => {
    window.addEventListener('keydown', onKeydown)
    refresh()
    if (STRAVA_ENABLED) refreshStrava()
    return () => window.removeEventListener('keydown', onKeydown)
  })

  function onKeydown(e) {
    if (e.key === 'Escape') onclose()
  }

  async function refresh() {
    loading = true
    try {
      saved = await backend.listActivities()
    } catch (e) {
      loadError = e?.message ?? String(e)
    } finally {
      loading = false
    }
  }

  async function refreshStrava() {
    try {
      strava = await backend.stravaStatus()
      if (strava.connected) await loadStravaPage(1)
    } catch {
      /* status check is best-effort; the connect button still works */
    }
  }

  async function handleStravaConnect() {
    stravaConnecting = true
    stravaError = null
    try {
      strava = await backend.stravaConnect()
      await loadStravaPage(1)
    } catch (e) {
      stravaError = e?.message ?? String(e)
    } finally {
      stravaConnecting = false
    }
  }

  async function handleStravaDisconnect() {
    try {
      await backend.stravaDisconnect()
    } catch {
      /* local disconnect always succeeds; server revoke is best-effort */
    }
    strava = { connected: false, athlete: null }
    stravaActivities = []
    stravaPage = 1
    stravaHasMore = false
  }

  async function loadStravaPage(page) {
    stravaLoading = true
    stravaError = null
    try {
      const items = await backend.stravaActivities(page)
      stravaActivities = items
      stravaPage = page
      // A full page means there's probably another one behind it.
      stravaHasMore = items.length === 10
    } catch (e) {
      const msg = e?.message ?? String(e)
      stravaError = msg
      if (msg.includes('reconnect')) {
        strava = { connected: false, athlete: null }
        stravaActivities = []
      }
    } finally {
      stravaLoading = false
    }
  }

  async function handleImportStrava(a) {
    if (importingId !== null) return
    importingId = a.id
    try {
      const filename = await backend.stravaImportActivity(a)
      await refresh()
      await onload({ savedFilename: filename })
      onclose()
    } catch (e) {
      app.errorMessage = `Strava import failed: ${e?.message ?? e}`
    } finally {
      importingId = null
    }
  }

  function isActive(filename) {
    return app.gpxFilename === filename
  }

  function formatStart(ms) {
    if (!ms) return ''
    const d = new Date(ms)
    const now = new Date()
    return d.toLocaleDateString([], {
      month: 'short',
      day: 'numeric',
      year: d.getFullYear() === now.getFullYear() ? undefined : 'numeric',
    })
  }

  function displayName(filename) {
    // Strip the extension only; keep dots that are part of the name (e.g. "2024.05.20.gpx").
    return filename.replace(/\.[^.]+$/, '')
  }

  function formatDuration(s) {
    if (!s) return ''
    const total = Math.round(s)
    if (total < 60) return `${total}s`
    const h = Math.floor(total / 3600)
    const m = Math.floor((total % 3600) / 60)
    return h > 0 ? `${h}h ${m}m` : `${m}m`
  }

  function formatDistance(m) {
    if (!m) return ''
    return app.units === 'imperial'
      ? `${(m / 1609.344).toFixed(1)} mi`
      : `${(m / 1000).toFixed(1)} km`
  }

  function metaLine(a) {
    return [formatStart(a.start_ms), formatDuration(a.duration_s)]
      .filter(Boolean)
      .join(' · ')
  }

  function stravaMetaLine(a) {
    return [
      formatStart(a.start_ms),
      formatDuration(a.duration_s),
      formatDistance(a.distance_m),
    ]
      .filter(Boolean)
      .join(' · ')
  }

  let sortedSaved = $derived.by(() => {
    const items = [...saved]
    if (sortMode === 'name-asc') {
      return items.sort((a, b) =>
        displayName(a.filename).localeCompare(displayName(b.filename)),
      )
    }
    if (sortMode === 'name-desc') {
      return items.sort((a, b) =>
        displayName(b.filename).localeCompare(displayName(a.filename)),
      )
    }
    if (sortMode === 'oldest') {
      return items.sort((a, b) => (a.start_ms ?? 0) - (b.start_ms ?? 0))
    }
    return items.sort((a, b) => (b.start_ms ?? 0) - (a.start_ms ?? 0))
  })

  async function handleChooseFromDisk() {
    opening = true
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: 'Activity (GPX, FIT, TCX)',
            extensions: dialogExtensions(['gpx', 'fit', 'tcx']),
          },
        ],
        title: 'Select Activity File',
      })
      if (!selected) return
      await onload(selected)
      onclose()
    } catch (e) {
      app.errorMessage = `Activity load failed: ${e?.message ?? e}`
    } finally {
      opening = false
    }
  }

  async function handleLoadSaved(filename) {
    if (isActive(filename)) {
      onclose()
      return
    }
    try {
      await onload({ savedFilename: filename })
      onclose()
    } catch (e) {
      const msg = e?.message ?? String(e)
      if (msg.includes('missing or its source moved')) {
        // Broken symlink — remove it and tell the user why it vanished.
        await backend.deleteActivity(filename).catch(() => {})
        await refresh()
        app.errorMessage = `"${filename}" can't be found — the original file may have been moved or deleted. Use the file picker to locate it again.`
      } else {
        app.errorMessage = `Could not open ${filename}: ${msg}`
      }
    }
  }

  async function handleDelete(filename) {
    deleting = [...deleting, filename]
    try {
      await backend.deleteActivity(filename)
      if (app.gpxFilename === filename) {
        // Activity no longer exists — clear it so the UI doesn't show a stale label.
        app.gpxFilename = null
      }
      await refresh()
    } catch (e) {
      app.errorMessage = `Delete failed: ${e?.message ?? e}`
    } finally {
      deleting = deleting.filter((x) => x !== filename)
    }
  }
</script>

<div
  role="dialog"
  aria-modal="true"
  aria-label="Choose Activity"
  tabindex="-1"
  class="fixed inset-0 z-50 flex items-center justify-center pt-14"
>
  <button
    type="button"
    class="absolute inset-0 cursor-default bg-black/60 backdrop-blur-sm"
    aria-label="Close activity picker"
    onclick={onclose}
  ></button>
  <WindowDragStrip />

  <div
    class="relative z-10 w-[720px] max-h-[80vh] flex flex-col rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl"
  >
    <!-- Header -->
    <div
      class="flex items-center justify-between px-5 py-4 border-b border-zinc-800 shrink-0"
    >
      <h2 class="text-sm font-semibold text-zinc-100">Choose Activity</h2>
      <div class="flex items-center gap-2">
        {#if STRAVA_ENABLED && !strava.connected}
          <Tooltip
            content="Authorize in your browser to import recent rides"
            side="bottom"
          >
            <button
              onclick={handleStravaConnect}
              disabled={stravaConnecting}
              class="inline-flex cursor-pointer items-center gap-1.5 rounded-[6px] border border-zinc-700 bg-zinc-900/70 px-2.5 py-1.5 text-xs font-medium text-zinc-300 transition-colors hover:border-zinc-500 hover:bg-zinc-800/80 hover:text-zinc-100 disabled:cursor-not-allowed disabled:opacity-60"
            >
              <Activity size={12} />
              {stravaConnecting ? 'Waiting for Strava…' : 'Connect to Strava'}
            </button>
          </Tooltip>
        {/if}
        <Tooltip
          content="Choose a GPX, FIT, or TCX activity file"
          side="bottom"
        >
          <button
            onclick={handleChooseFromDisk}
            disabled={opening}
            class="inline-flex cursor-pointer items-center gap-1.5 rounded-[6px] border border-primary/70 bg-primary/15 px-2.5 py-1.5 text-xs font-medium text-zinc-100 transition-colors hover:border-primary hover:bg-primary/25 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <FolderOpen size={12} />
            {opening ? 'Opening…' : 'Choose from disk'}
          </button>
        </Tooltip>
        <button
          onclick={onclose}
          class="cursor-pointer text-zinc-500 hover:text-zinc-200 transition-colors rounded-md p-0.5"
          aria-label="Close"
        >
          <X size={16} />
        </button>
      </div>
    </div>

    <!-- Scrollable body -->
    <div class="overflow-y-auto flex-1 px-5 py-4 space-y-6">
      <div>
        {#if saved.length >= 2}
          <div class="mb-3 flex items-center justify-end">
            <label
              class="flex items-center gap-2 text-[10px] font-medium uppercase tracking-wider text-zinc-500"
            >
              Sort by
              <select
                bind:value={sortMode}
                class="h-7 rounded-[6px] border border-zinc-700 bg-zinc-800/60 px-2 text-xs normal-case tracking-normal text-zinc-200 outline-none transition-colors hover:border-zinc-500 focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500"
              >
                <option value="recent">Recent</option>
                <option value="oldest">Oldest</option>
                <option value="name-asc">Name A-Z</option>
                <option value="name-desc">Name Z-A</option>
              </select>
            </label>
          </div>
        {/if}

        {#if loading}
          <div class="grid grid-cols-2 gap-2">
            {#each [1, 2, 3, 4] as i (i)}
              <div
                class="h-14 rounded-lg border border-zinc-800 bg-zinc-800/30 animate-pulse"
              ></div>
            {/each}
          </div>
        {:else if loadError}
          <p class="text-xs text-red-400 select-text cursor-text">
            {loadError}
          </p>
        {:else if saved.length === 0}
          <p class="text-xs text-zinc-500">
            No activities yet. Choose one from disk to get started.
          </p>
        {:else}
          <div class="grid grid-cols-2 gap-2">
            {#each sortedSaved as a (a.filename)}
              {@const active = isActive(a.filename)}
              {@const busy = deleting.includes(a.filename)}
              <div
                class="rounded-lg border overflow-hidden transition-colors flex items-center
                       {active
                  ? 'border-primary bg-zinc-800'
                  : 'border-zinc-700 bg-zinc-800/40 hover:border-zinc-500 hover:bg-zinc-800/80'}"
              >
                <button
                  onclick={() => handleLoadSaved(a.filename)}
                  class="flex-1 min-w-0 flex items-center gap-2.5 text-left px-3 py-2.5 cursor-pointer"
                >
                  <svg
                    viewBox="-0.1 -0.1 1.2 1.2"
                    class="h-9 w-9 shrink-0 {active
                      ? 'text-primary'
                      : 'text-zinc-500'}"
                    aria-hidden="true"
                  >
                    {#if a.track?.length}
                      <polyline
                        points={a.track.map(([x, y]) => `${x},${y}`).join(' ')}
                        fill="none"
                        stroke="currentColor"
                        stroke-width="0.07"
                        stroke-linejoin="round"
                        stroke-linecap="round"
                      />
                    {:else}
                      <circle
                        cx="0.5"
                        cy="0.5"
                        r="0.05"
                        fill="currentColor"
                        opacity="0.4"
                      />
                    {/if}
                  </svg>
                  <span class="min-w-0">
                    <span
                      class="text-xs font-medium text-zinc-100 truncate block"
                      >{displayName(a.filename)}</span
                    >
                    <span
                      class="text-[10px] text-zinc-500 truncate block mt-0.5"
                      >{metaLine(a)}</span
                    >
                  </span>
                </button>

                <button
                  onclick={() => handleDelete(a.filename)}
                  disabled={busy}
                  class="shrink-0 cursor-pointer p-2 mr-1 rounded text-zinc-500 hover:text-red-400 transition-colors disabled:cursor-not-allowed disabled:opacity-40"
                  title="Remove from disk"
                >
                  {#if busy}
                    <span class="text-[10px]">…</span>
                  {:else}
                    <Trash2 size={13} />
                  {/if}
                </button>
              </div>
            {/each}
          </div>
        {/if}
      </div>

      {#if STRAVA_ENABLED && (strava.connected || stravaError)}
        <div>
          <div class="mb-3 flex items-center justify-between">
            <span
              class="text-[10px] font-medium uppercase tracking-wider text-zinc-500"
            >
              Strava{strava.athlete ? ` · ${strava.athlete}` : ''}
            </span>
            {#if strava.connected}
              <div class="flex items-center gap-1.5">
                <button
                  onclick={() => loadStravaPage(stravaPage - 1)}
                  disabled={stravaPage <= 1 || stravaLoading}
                  class="cursor-pointer rounded p-1 text-zinc-500 transition-colors hover:text-zinc-200 disabled:cursor-not-allowed disabled:opacity-40"
                  aria-label="Newer activities"
                >
                  <ChevronLeft size={13} />
                </button>
                <span class="text-[10px] tabular-nums text-zinc-500"
                  >Page {stravaPage}</span
                >
                <button
                  onclick={() => loadStravaPage(stravaPage + 1)}
                  disabled={!stravaHasMore || stravaLoading}
                  class="cursor-pointer rounded p-1 text-zinc-500 transition-colors hover:text-zinc-200 disabled:cursor-not-allowed disabled:opacity-40"
                  aria-label="Older activities"
                >
                  <ChevronRight size={13} />
                </button>
                <button
                  onclick={handleStravaDisconnect}
                  class="ml-2 cursor-pointer text-[10px] text-zinc-600 transition-colors hover:text-zinc-300"
                >
                  Disconnect
                </button>
              </div>
            {/if}
          </div>

          {#if stravaError}
            <p class="text-xs text-red-400 select-text cursor-text">
              {stravaError}
            </p>
          {:else if stravaLoading}
            <div class="grid grid-cols-2 gap-2">
              {#each [1, 2, 3, 4] as i (i)}
                <div
                  class="h-14 rounded-lg border border-zinc-800 bg-zinc-800/30 animate-pulse"
                ></div>
              {/each}
            </div>
          {:else if stravaActivities.length === 0}
            <p class="text-xs text-zinc-500">No activities found on Strava.</p>
          {:else}
            <div class="grid grid-cols-2 gap-2">
              {#each stravaActivities as a (a.id)}
                {@const importing = importingId === a.id}
                <button
                  onclick={() => handleImportStrava(a)}
                  disabled={importingId !== null}
                  class="rounded-lg border border-zinc-700 bg-zinc-800/40 overflow-hidden transition-colors flex items-center gap-2.5 text-left px-3 py-2.5 cursor-pointer hover:border-zinc-500 hover:bg-zinc-800/80 disabled:cursor-not-allowed
                         {importing ? 'border-primary' : ''} {importingId !==
                    null && !importing
                    ? 'opacity-50'
                    : ''}"
                >
                  <svg
                    viewBox="-0.1 -0.1 1.2 1.2"
                    class="h-9 w-9 shrink-0 {importing
                      ? 'text-primary'
                      : 'text-zinc-500'}"
                    aria-hidden="true"
                  >
                    {#if a.track?.length}
                      <polyline
                        points={a.track.map(([x, y]) => `${x},${y}`).join(' ')}
                        fill="none"
                        stroke="currentColor"
                        stroke-width="0.07"
                        stroke-linejoin="round"
                        stroke-linecap="round"
                      />
                    {:else}
                      <circle
                        cx="0.5"
                        cy="0.5"
                        r="0.05"
                        fill="currentColor"
                        opacity="0.4"
                      />
                    {/if}
                  </svg>
                  <span class="min-w-0">
                    <span
                      class="text-xs font-medium text-zinc-100 truncate block"
                      >{a.name}</span
                    >
                    <span
                      class="text-[10px] text-zinc-500 truncate block mt-0.5"
                    >
                      {importing ? 'Importing…' : stravaMetaLine(a)}
                    </span>
                  </span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
</div>
