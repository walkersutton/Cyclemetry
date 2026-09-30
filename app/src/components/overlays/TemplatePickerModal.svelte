<script>
  import { getContext, onMount } from 'svelte'
  import WindowDragStrip from './WindowDragStrip.svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import * as backend from '@/api/backend.js'
  import { X, Trash2, Plus, Upload } from 'lucide-svelte'
  import NewTemplateDialog from './NewTemplateDialog.svelte'

  const app = getContext('app')
  let { onclose } = $props()

  let communityList = $state([])
  let communityLoading = $state(true)
  let communityError = $state(null)
  let installing = $state([])
  let deleting = $state([])
  let failedPreviews = $state([])
  let creating = $state(false)
  let importing = $state(false)
  let showNameDialog = $state(false)
  let viewportHeight = $state(window.innerHeight)
  let resizeFrame = null

  onMount(() => {
    window.addEventListener('keydown', onKeydown)

    function onResize() {
      if (resizeFrame) cancelAnimationFrame(resizeFrame)
      resizeFrame = requestAnimationFrame(() => {
        viewportHeight = window.innerHeight
        resizeFrame = null
      })
    }

    window.addEventListener('resize', onResize)
    ;(async () => {
      try {
        communityList = await backend.fetchCommunityTemplates()
      } catch (e) {
        communityError = e?.message ?? 'Failed to load'
      } finally {
        communityLoading = false
      }
    })()

    return () => {
      window.removeEventListener('keydown', onKeydown)
      window.removeEventListener('resize', onResize)
      if (resizeFrame) cancelAnimationFrame(resizeFrame)
    }
  })

  function onKeydown(e) {
    if (e.key === 'Escape') onclose()
  }

  // Templates the user has locally (any type)
  let installed = $derived(app.templates ?? [])

  // Community templates not yet installed
  let available = $derived(
    communityList.filter((c) => !installed.some((i) => i.id === c.id)),
  )
  let communityIds = $derived(new Set(communityList.map((c) => c.id)))

  function isActive(id) {
    return app.loadedTemplateFilename === id
  }

  function statusLabel(type) {
    if (type === 'community-modified') return 'Modified'
    if (type === 'built-in') return 'Built-in'
    return null
  }

  function isCommunityTemplate(tpl) {
    return (
      tpl.type === 'community' ||
      tpl.type === 'community-modified' ||
      communityIds.has(tpl.id)
    )
  }

  function previewFailed(id) {
    return failedPreviews.includes(id)
  }

  function onImgError(id) {
    if (!failedPreviews.includes(id)) failedPreviews = [...failedPreviews, id]
  }

  function handleLoad(id) {
    if (isActive(id)) {
      onclose()
      return
    }
    app.confirmIfModified(async () => {
      try {
        await app.loadTemplate(id)
        onclose()
      } catch (e) {
        app.errorMessage = `Failed to load: ${e?.message ?? e}`
      }
    })
  }

  async function handleCommunityClick(id) {
    app.confirmIfModified(async () => {
      installing = [...installing, id]
      try {
        await backend.installCommunityTemplate(id)
        await app.loadTemplate(id)
        onclose()
        app.fetchTemplates()
      } catch (e) {
        app.errorMessage = `Failed to install: ${e?.message ?? e}`
      } finally {
        installing = installing.filter((x) => x !== id)
      }
    })
  }

  async function handleDelete(id) {
    deleting = [...deleting, id]
    try {
      await backend.deleteTemplate(id)
      if (app.loadedTemplateFilename === id) app.clearTemplate()
      await app.fetchTemplates()
    } catch (e) {
      app.errorMessage = `Delete failed: ${e?.message ?? e}`
    } finally {
      deleting = deleting.filter((x) => x !== id)
    }
  }

  function handleCreate() {
    app.confirmIfModified(() => {
      showNameDialog = true
    })
  }

  async function handleNameConfirm(name) {
    showNameDialog = false
    creating = true
    try {
      await app.newTemplate(name)
      onclose()
    } catch (e) {
      app.errorMessage = `Create failed: ${e?.message ?? e}`
    } finally {
      creating = false
    }
  }

  async function handleImport() {
    importing = true
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: 'Cyclemetry Template', extensions: ['json'] }],
        title: 'Import Template',
      })
      if (!selected) return
      const result = await backend.importTemplate(selected)
      await app.fetchTemplates()
      app.confirmIfModified(async () => {
        try {
          await app.loadTemplate(result.filename)
          onclose()
        } catch (e) {
          app.errorMessage = `Imported, but failed to load: ${e?.message ?? e}`
        }
      })
    } catch (e) {
      app.errorMessage = `Import failed: ${e?.message ?? e}`
    } finally {
      importing = false
    }
  }
</script>

<div
  role="dialog"
  aria-modal="true"
  aria-label="Choose Template"
  tabindex="-1"
  class="fixed inset-0 z-50 flex items-center justify-center pt-14"
>
  <button
    type="button"
    class="absolute inset-0 cursor-default bg-black/60 backdrop-blur-sm"
    aria-label="Close template picker"
    onclick={onclose}
  ></button>
  <WindowDragStrip />

  <div
    class="relative z-10 flex w-[720px] max-h-[80vh] flex-col rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl"
  >
    <!-- Header -->
    <div
      class="flex items-center justify-between px-5 py-4 border-b border-zinc-800 shrink-0"
    >
      <h2 class="text-sm font-semibold text-zinc-100">Choose Template</h2>
      <div class="flex items-center gap-2">
        <button
          onclick={handleImport}
          disabled={importing}
          class="inline-flex cursor-pointer items-center gap-1.5 rounded-[6px] border border-zinc-700 bg-zinc-900/70 px-2.5 py-1.5 text-xs font-medium text-zinc-300 transition-colors hover:border-zinc-500 hover:bg-zinc-800/80 hover:text-zinc-100 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Upload size={12} />
          {importing ? 'Importing…' : 'Import from disk'}
        </button>
        <button
          onclick={handleCreate}
          disabled={creating}
          class="inline-flex cursor-pointer items-center gap-1.5 rounded-[6px] border border-zinc-700 bg-zinc-900/70 px-2.5 py-1.5 text-xs font-medium text-zinc-300 transition-colors hover:border-zinc-500 hover:bg-zinc-800/80 hover:text-zinc-100 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Plus size={12} />
          {creating ? 'Creating…' : 'Create template'}
        </button>
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
    {#key viewportHeight}
      <div class="min-h-0 overflow-y-auto px-5 py-4 space-y-6">
        <!-- Installed templates -->
        {#if installed.length > 0}
          <div>
            <p
              class="text-[10px] font-semibold uppercase tracking-wider text-zinc-500 mb-3"
            >
              Installed
            </p>
            <div class="grid grid-cols-3 gap-3">
              {#each installed as tpl (tpl.id)}
                {@const active = isActive(tpl.id)}
                {@const label = statusLabel(tpl.type)}
                {@const communityTemplate = isCommunityTemplate(tpl)}
                {@const busy = deleting.includes(tpl.id)}
                <div
                  class="rounded-lg border overflow-hidden transition-colors
                         {active
                    ? 'border-primary bg-zinc-800'
                    : 'border-zinc-700 bg-zinc-800/40 hover:border-zinc-500 hover:bg-zinc-800/80'}"
                >
                  <!-- Preview (clickable) -->
                  <button
                    onclick={() => handleLoad(tpl.id)}
                    class="w-full text-left aspect-video bg-zinc-800 flex items-center justify-center overflow-hidden block cursor-pointer"
                  >
                    {#if tpl.preview_url && !previewFailed(tpl.id)}
                      <img
                        src={tpl.preview_url}
                        alt={tpl.name}
                        class="w-full h-full object-contain"
                        onerror={() => onImgError(tpl.id)}
                      />
                    {:else}
                      <span class="text-[10px] text-zinc-600 font-mono"
                        >{tpl.id}</span
                      >
                    {/if}
                  </button>
                  <!-- Info row -->
                  <div class="px-2.5 py-2 flex items-center gap-1">
                    <button
                      onclick={() => handleLoad(tpl.id)}
                      class="flex-1 min-w-0 text-left cursor-pointer flex items-center gap-1.5"
                    >
                      {#if communityTemplate}
                        <img
                          src="/logo192.png"
                          alt=""
                          title="Community template"
                          class="h-3.5 w-3.5 shrink-0 rounded-[3px]"
                        />
                      {/if}
                      <span
                        class="min-w-0 text-xs font-medium text-zinc-100 truncate block"
                        >{tpl.name}</span
                      >
                    </button>
                    {#if label}
                      <span class="shrink-0 text-[10px] text-zinc-500"
                        >{label}</span
                      >
                    {/if}
                    <button
                      onclick={() => handleDelete(tpl.id)}
                      disabled={busy}
                      class="shrink-0 cursor-pointer p-1 rounded text-zinc-500 hover:text-red-400 transition-colors disabled:cursor-not-allowed disabled:opacity-40 ml-0.5"
                      title="Remove from disk"
                    >
                      {#if busy}
                        <span class="text-[10px]">…</span>
                      {:else}
                        <Trash2 size={13} />
                      {/if}
                    </button>
                  </div>
                </div>
              {/each}
            </div>
          </div>
        {/if}

        <!-- Community templates -->
        <div>
          <p
            class="text-[10px] font-semibold uppercase tracking-wider text-zinc-500 mb-3"
          >
            Community
          </p>

          {#if communityLoading}
            <div class="grid grid-cols-3 gap-3">
              {#each [1, 2, 3, 4, 5, 6] as i (i)}
                <div
                  class="rounded-lg border border-zinc-800 bg-zinc-800/30 animate-pulse"
                >
                  <div class="aspect-video bg-zinc-800/60"></div>
                  <div class="px-2.5 py-2 h-8"></div>
                </div>
              {/each}
            </div>
          {:else if communityError}
            <p class="text-xs text-red-400">{communityError}</p>
          {:else if available.length === 0}
            <p class="text-xs text-zinc-500">
              All community templates are installed.
            </p>
          {:else}
            <div class="grid grid-cols-3 gap-3">
              {#each available as tpl (tpl.id)}
                {@const busy = installing.includes(tpl.id)}
                <button
                  onclick={() => handleCommunityClick(tpl.id)}
                  disabled={busy}
                  class="w-full rounded-lg border border-zinc-700 bg-zinc-800/40 overflow-hidden text-left cursor-pointer hover:border-zinc-500 hover:bg-zinc-800/80 transition-colors disabled:cursor-not-allowed"
                >
                  <!-- Preview -->
                  <div
                    class="aspect-video bg-zinc-800 relative flex items-center justify-center overflow-hidden"
                  >
                    {#if tpl.preview_url && !previewFailed(tpl.id)}
                      <img
                        src={tpl.preview_url}
                        alt={tpl.name}
                        class="w-full h-full object-contain"
                        onerror={() => onImgError(tpl.id)}
                      />
                    {:else}
                      <span class="text-[10px] text-zinc-600 font-mono"
                        >{tpl.id}</span
                      >
                    {/if}
                    {#if busy}
                      <div
                        class="absolute inset-0 bg-black/50 flex items-center justify-center"
                      >
                        <span class="text-[11px] text-zinc-300"
                          >Installing…</span
                        >
                      </div>
                    {/if}
                  </div>
                  <!-- Name -->
                  <div class="px-2.5 py-2 flex items-center gap-1.5">
                    <img
                      src="/logo192.png"
                      alt=""
                      title="Cyclemetry template"
                      class="h-3.5 w-3.5 shrink-0 rounded-[3px]"
                    />
                    <span
                      class="min-w-0 text-xs font-medium text-zinc-100 truncate block"
                      >{tpl.name}</span
                    >
                  </div>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>
    {/key}
  </div>
</div>

{#if showNameDialog}
  <NewTemplateDialog
    oncreate={handleNameConfirm}
    oncancel={() => (showNameDialog = false)}
  />
{/if}
