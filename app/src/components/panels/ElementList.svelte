<script>
  import { getContext } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import {
    ArrowDown,
    ArrowUp,
    BarChart2,
    ChevronDown,
    ChevronRight,
    Folder,
    FolderPlus,
    Gauge,
    Hash,
    Image as ImageIcon,
    Map as MapIcon,
    Plus,
    Square,
    Thermometer,
    Trash2,
    Type,
    X,
  } from 'lucide-svelte'

  import * as backend from '../../api/backend.js'
  import { ADD_PRESETS, elementMeta } from '../../lib/elementTypes.js'
  import { metricRangeIssues } from '../../lib/metricLimits.js'
  import AssetPicker from '../overlays/AssetPicker.svelte'

  const app = getContext('app')

  let addImagePending = $state(false)

  // ── Drag state ───────────────────────────────────────────────────────────────
  let draggingId = $state(null)
  let dropIndex = $state(null) // index into listItems() for between-item drops
  let dropGroupId = $state(null) // group id when hovering over a group header (add-to-group)
  let dropGroupedIndex = $state(null) // insert index within a group's element list
  let dropGroupedGroupId = $state(null) // which group the within-group drop targets
  let pointerDrag = $state(null)
  let suppressClickId = $state(null)
  let suppressGroupClickId = $state(null)
  let addMenuOpen = $state(false)

  // ── Group UI state ───────────────────────────────────────────────────────────
  let collapsedGroups = new SvelteSet()
  let renamingGroupId = $state(null)
  let renameValue = $state('')

  const ICONS = {
    type: Type,
    hash: Hash,
    bar: BarChart2,
    map: MapIcon,
    meter: Thermometer,
    gauge: Gauge,
    rect: Square,
    image: ImageIcon,
  }

  // All elements in reverse layer order (front first). Used for drag logic.
  let flatElements = $derived(() => {
    if (!app.config?.elements) return []
    const byId = {}
    for (const el of app.config.elements) {
      const meta = elementMeta(el)
      byId[el.id] = {
        id: el.id,
        type: meta.kind,
        name: meta.name,
        unit: meta.unit,
      }
    }
    return [...(app.elementLayerOrder ?? [])]
      .reverse()
      .map((id) => byId[id])
      .filter(Boolean)
  })

  // Structured list for rendering: groups appear where their front-most
  // element would be; ungrouped elements appear in their normal z-position.
  let listItems = $derived(() => {
    if (!app.config?.elements) return []
    const groups = app.config?.scene?.groups ?? []
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const elementToGroup = new Map()
    for (const g of groups) {
      for (const eid of g.element_ids) elementToGroup.set(eid, g.id)
    }
    const groupById = new Map(groups.map((g) => [g.id, g]))
    const els = flatElements()
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const seenGroups = new Set()
    const items = []
    for (const el of els) {
      const gid = elementToGroup.get(el.id)
      if (!gid) {
        items.push({ kind: 'element', ...el })
      } else if (!seenGroups.has(gid)) {
        seenGroups.add(gid)
        const group = groupById.get(gid)
        const groupEls = els.filter((e) => elementToGroup.get(e.id) === gid)
        items.push({
          kind: 'group',
          id: gid,
          name: group?.name ?? gid,
          elements: groupEls,
        })
      }
    }
    return items
  })

  async function defaultsForPreset(preset) {
    const defaults = preset.defaults(app.config?.scene)
    if (preset.type !== 'meter' || !app.gpxFilename || !app.config?.scene)
      return defaults
    const metric = defaults.value ?? 'speed'
    const start = app.config.scene.start ?? 0
    const end = app.config.scene.end ?? app.timelineDuration
    try {
      const range = await backend.getActivityMetricRange(
        app.gpxFilename,
        metric,
        defaults.unit,
        start,
        end,
      )
      const issues = metricRangeIssues(metric, defaults.unit, range)
      if (issues.min || issues.max) return defaults
      return { ...defaults, min: range.min, max: range.max }
    } catch (err) {
      console.debug('Could not initialize meter range from activity:', err)
      return defaults
    }
  }

  async function addPreset(preset) {
    addMenuOpen = false
    if (preset.type === 'image') {
      addImagePending = true
      return
    }
    app.addElement(preset.type, await defaultsForPreset(preset))
  }

  async function onImageSelected(filename) {
    addImagePending = false
    const defaults = ADD_PRESETS.find((p) => p.type === 'image').defaults()
    let width = defaults.width,
      height = defaults.height
    try {
      const size = await backend.imageSize(filename)
      width = size.width
      height = size.height
    } catch {
      /* fallback to preset defaults */
    }
    app.addElement('image', {
      ...defaults,
      file: filename,
      width,
      height,
      natural_width: width,
      natural_height: height,
    })
  }

  function presetLabel(title) {
    return title.replace(/^Add\s+/i, '').replace(/\s*\([^)]*\)/g, '')
  }

  function presetHint(preset) {
    if (preset.key === 'label') return 'Static copy'
    if (preset.key === 'value') return 'Live metric'
    if (preset.key === 'chart') return 'Metric graph'
    if (preset.key === 'map') return 'GPS route'
    if (preset.key === 'meter') return 'Progress bar'
    if (preset.key === 'gauge') return 'Dial'
    if (preset.key === 'rect') return 'Shape'
    if (preset.key === 'image') return 'Asset'
    return 'Overlay'
  }

  function groupSelectedElements() {
    const ids = app.selectedElementIds
    if (ids.length < 2) return
    const n = (app.config?.scene?.groups?.length ?? 0) + 1
    app.createGroup(`Group ${n}`, ids)
  }

  function toggleCollapse(groupId) {
    if (collapsedGroups.has(groupId)) collapsedGroups.delete(groupId)
    else collapsedGroups.add(groupId)
  }

  function startRename(groupId, currentName) {
    renamingGroupId = groupId
    renameValue = currentName
  }

  function commitRename() {
    if (renamingGroupId && renameValue.trim()) {
      app.renameGroup(renamingGroupId, renameValue.trim())
    }
    renamingGroupId = null
  }

  function handleGroupClick(groupId) {
    if (suppressGroupClickId === groupId) {
      suppressGroupClickId = null
      return
    }
    if (app.selectedGroupId === groupId) {
      app.selectedElementId = null // deselect
    } else {
      app.selectGroup(groupId)
    }
  }

  // ── Drag to reorder / add to group ──────────────────────────────────────────

  function onRowPointerDown(e, id, kind, fromGroupId = null) {
    if (e.button !== 0 || e.target.closest('[data-layer-action]')) return
    const startIdx = fromGroupId
      ? listItems().findIndex(
          (it) => it.kind === 'group' && it.id === fromGroupId,
        )
      : listItems().findIndex((it) => it.id === id)
    pointerDrag = {
      id,
      kind,
      fromGroupId,
      pointerId: e.pointerId,
      startY: e.clientY,
      moved: false,
      shiftKey: e.shiftKey,
    }
    dropIndex = startIdx
    e.currentTarget.setPointerCapture?.(e.pointerId)
  }

  // Convert a reordered listItems array back to elementLayerOrder (back-to-front).
  // Groups are moved as a block; relative element order within a group is preserved.
  function listItemsToLayerOrder(newListItems) {
    const currentOrder = app.elementLayerOrder ?? []
    const result = []
    for (let i = newListItems.length - 1; i >= 0; i--) {
      const item = newListItems[i]
      if (item.kind === 'element') {
        result.push(item.id)
      } else {
        const groupElIds = new Set(item.elements.map((e) => e.id))
        result.push(...currentOrder.filter((id) => groupElIds.has(id)))
      }
    }
    return result
  }

  function updateDropTarget(clientY) {
    const isDraggingElement = pointerDrag?.kind === 'element'
    const fromGroupId = pointerDrag?.fromGroupId ?? null

    // Within-group reorder: only when dragging a grouped element over siblings of its own group
    if (isDraggingElement && fromGroupId != null) {
      const siblings = [
        ...document.querySelectorAll(`[data-grouped-item="${fromGroupId}"]`),
      ]
      if (siblings.length > 0) {
        const firstRect = siblings[0].getBoundingClientRect()
        const lastRect = siblings[siblings.length - 1].getBoundingClientRect()
        if (clientY >= firstRect.top && clientY <= lastRect.bottom) {
          const hitIdx = siblings.findIndex((row) => {
            const r = row.getBoundingClientRect()
            return clientY < r.top + r.height / 2
          })
          dropGroupedIndex = hitIdx === -1 ? siblings.length : hitIdx
          dropGroupedGroupId = fromGroupId
          dropIndex = null
          dropGroupId = null
          return
        }
      }
    }
    dropGroupedIndex = null
    dropGroupedGroupId = null

    // Check group headers for add-to-group (only when dragging an ungrouped element)
    if (isDraggingElement) {
      const groupHeaders = [...document.querySelectorAll('[data-group-id]')]
      for (const el of groupHeaders) {
        const rect = el.getBoundingClientRect()
        if (clientY >= rect.top && clientY <= rect.bottom) {
          dropGroupId = el.dataset.groupId
          dropIndex = null
          return
        }
      }
    }

    dropGroupId = null

    // Compute between-item drop position using top-level list rows
    const rows = [...document.querySelectorAll('[data-list-item]')]
    if (rows.length === 0) return
    const hitRow = rows.findIndex((row) => {
      const rect = row.getBoundingClientRect()
      return clientY < rect.top + rect.height / 2
    })
    dropIndex =
      hitRow === -1
        ? listItems().length
        : parseInt(rows[hitRow].dataset.listItem)
  }

  function onWindowPointerMove(e) {
    if (!pointerDrag) return
    const dy = Math.abs(e.clientY - pointerDrag.startY)
    if (!pointerDrag.moved && dy < 4) return
    pointerDrag = { ...pointerDrag, moved: true }
    draggingId = pointerDrag.id
    updateDropTarget(e.clientY)
    e.preventDefault()
  }

  // Rebuild elementLayerOrder so the given group's elements occupy their
  // existing layer slots, but in the order specified (front-to-back).
  function reorderWithinGroup(groupId, newGroupElsFrontToBack) {
    const currentOrder = app.elementLayerOrder ?? []
    const groupElSet = new Set(newGroupElsFrontToBack)
    const newBackToFront = [...newGroupElsFrontToBack].reverse()
    const newOrder = [...currentOrder]
    let cursor = 0
    for (let i = 0; i < newOrder.length; i++) {
      if (groupElSet.has(newOrder[i])) {
        newOrder[i] = newBackToFront[cursor++]
      }
    }
    app.setElementLayerOrder(newOrder)
    app.reorderGroupElements(groupId, newGroupElsFrontToBack)
  }

  function commitPointerDrop() {
    if (!pointerDrag?.moved) return
    const { id, kind, fromGroupId } = pointerDrag

    // Within-group reorder
    if (
      kind === 'element' &&
      dropGroupedIndex != null &&
      dropGroupedGroupId != null
    ) {
      const groupId = dropGroupedGroupId
      const groupItem = listItems().find(
        (it) => it.kind === 'group' && it.id === groupId,
      )
      if (!groupItem) return
      const fromIdx = groupItem.elements.findIndex((e) => e.id === id)
      if (fromIdx < 0) return
      const newGroupEls = [...groupItem.elements]
      const [moved] = newGroupEls.splice(fromIdx, 1)
      const toIdx = Math.max(
        0,
        Math.min(
          newGroupEls.length,
          dropGroupedIndex - (fromIdx < dropGroupedIndex ? 1 : 0),
        ),
      )
      if (toIdx === fromIdx) {
        app.selectedElementId = id
        suppressClickId = id
        return
      }
      newGroupEls.splice(toIdx, 0, moved)
      reorderWithinGroup(
        groupId,
        newGroupEls.map((e) => e.id),
      )
      app.selectedElementId = id
      suppressClickId = id
      return
    }

    if (kind === 'element' && dropGroupId != null) {
      // Drop element onto a group → add it to that group (also removes from old group)
      app.addElementToGroup(id, dropGroupId)
      suppressClickId = id
      return
    }

    if (dropIndex == null) return

    // Drag a grouped element out to a top-level position
    if (fromGroupId != null && dropGroupId == null) {
      const items = listItems()
      const groupItem = items.find(
        (it) => it.kind === 'group' && it.id === fromGroupId,
      )
      if (!groupItem) return
      const el = groupItem.elements.find((e) => e.id === id)
      if (!el) return

      const groupBecomesEmpty = groupItem.elements.length === 1
      const groupIdx = items.findIndex(
        (it) => it.kind === 'group' && it.id === fromGroupId,
      )

      // Build list with element removed from its group (possibly dropping the now-empty group)
      const modifiedBase = items.flatMap((it) => {
        if (it.kind === 'group' && it.id === fromGroupId) {
          const remaining = it.elements.filter((e) => e.id !== id)
          return remaining.length > 0 ? [{ ...it, elements: remaining }] : []
        }
        return [it]
      })

      // Adjust index for the removed group item (if group was eliminated)
      const adjustedIdx =
        groupBecomesEmpty && dropIndex > groupIdx
          ? Math.max(0, dropIndex - 1)
          : dropIndex
      const clampedIdx = Math.max(0, Math.min(modifiedBase.length, adjustedIdx))

      const newItems = [...modifiedBase]
      newItems.splice(clampedIdx, 0, { kind: 'element', ...el })

      app.removeFromGroupAndReorder(id, listItemsToLayerOrder(newItems))
      suppressClickId = id
      return
    }

    const items = listItems()
    const fromIdx = items.findIndex((it) => it.id === id)
    if (fromIdx < 0) return

    const next = [...items]
    const [moved] = next.splice(fromIdx, 1)
    const toIdx = Math.max(
      0,
      Math.min(next.length, dropIndex - (fromIdx < dropIndex ? 1 : 0)),
    )
    next.splice(toIdx, 0, moved)

    if (toIdx !== fromIdx) {
      app.setElementLayerOrder(listItemsToLayerOrder(next))
    }

    if (kind === 'element') {
      app.selectedElementId = id
      suppressClickId = id
    } else {
      suppressGroupClickId = id
    }
  }

  function onWindowPointerUp() {
    if (!pointerDrag) return
    if (pointerDrag.moved) {
      commitPointerDrop()
    } else {
      const { id, kind, shiftKey } = pointerDrag
      if (kind === 'element') {
        if (shiftKey) {
          app.toggleElementSelection(id)
        } else {
          const selected = app.selectedElementIds.includes(id)
          app.selectedElementId =
            selected && app.selectedElementIds.length === 1 ? null : id
        }
        suppressClickId = id
      }
      // group clicks fall through to the group header's onclick
    }
    pointerDrag = null
    draggingId = null
    dropIndex = null
    dropGroupId = null
    dropGroupedIndex = null
    dropGroupedGroupId = null
  }

  function selectElement(e, id) {
    if (suppressClickId === id) {
      suppressClickId = null
      return
    }
    if (e.shiftKey) {
      app.toggleElementSelection(id)
    } else {
      const selected = app.selectedElementIds.includes(id)
      app.selectedElementId =
        selected && app.selectedElementIds.length === 1 ? null : id
    }
  }

  function onKeydown(e) {
    if (e.key === 'Escape') addMenuOpen = false
  }
</script>

<svelte:window
  onpointermove={onWindowPointerMove}
  onpointerup={onWindowPointerUp}
  onpointercancel={onWindowPointerUp}
  onkeydown={onKeydown}
/>

<section class="px-4 py-3 flex-1 overflow-y-auto">
  <div class="flex items-center justify-between mb-2">
    <p class="text-[10px] font-semibold uppercase tracking-wider text-zinc-500">
      Elements
    </p>
    <div class="flex items-center gap-1">
      {#if (app.selectedElementIds?.length ?? 0) >= 2}
        <button
          onclick={groupSelectedElements}
          title="Group selected elements"
          class="p-1 rounded text-zinc-400 hover:text-zinc-200 hover:bg-[var(--panel2)] transition-colors"
        >
          <FolderPlus size={13} />
        </button>
      {/if}
      <button
        onclick={() => (addMenuOpen = !addMenuOpen)}
        disabled={!app.config}
        aria-expanded={addMenuOpen}
        class="inline-flex h-7 items-center gap-1.5 rounded-[6px] bg-[var(--panel2)] px-2 text-[11px] font-medium text-zinc-300 transition-colors hover:bg-[var(--panel3)] hover:text-zinc-100 cursor-pointer disabled:cursor-not-allowed disabled:opacity-50"
        title="Add element"
      >
        <Plus size={12} />
        Add
      </button>
    </div>
  </div>

  {#if !app.config}
    <p class="text-xs text-zinc-600 italic">Load a template to see elements.</p>
  {:else if listItems().length === 0}
    <p class="text-xs text-zinc-600 italic">
      No elements. Use Add to create one.
    </p>
  {:else}
    <ul class="space-y-0.5 pb-3">
      {#each listItems() as item (item.kind + (item.id ?? item.name))}
        {#if item.kind === 'group'}
          {@const groupSelected = app.selectedGroupId === item.id}
          {@const collapsed = collapsedGroups.has(item.id)}
          {@const listIdx = listItems().findIndex(
            (it) => it.id === item.id && it.kind === 'group',
          )}

          <!-- Group header row (draggable + drop target for add-to-group) -->
          <li
            data-list-item={listIdx}
            class={`rounded-[6px] relative
              ${draggingId === item.id ? 'opacity-45' : ''}
              ${dropIndex === listIdx && draggingId !== item.id ? 'before:absolute before:left-0 before:right-0 before:-top-0.5 before:h-px before:bg-primary' : ''}
              ${dropIndex === listIdx + 1 && draggingId !== item.id ? 'after:absolute after:left-0 after:right-0 after:-bottom-0.5 after:h-px after:bg-primary' : ''}`}
          >
            <div
              data-group-id={item.id}
              onpointerdown={(e) => onRowPointerDown(e, item.id, 'group')}
              class={`group/grp flex items-center gap-1 px-1.5 py-1.5 rounded-[6px] cursor-grab active:cursor-grabbing transition-colors
                ${
                  groupSelected
                    ? 'bg-primary/10 border border-primary/30 text-primary'
                    : 'text-zinc-400 hover:bg-[var(--panel2)] hover:text-zinc-200'
                }
                ${dropGroupId === item.id ? 'ring-1 ring-primary bg-primary/5' : ''}`}
              role="button"
              tabindex="0"
              onclick={() => handleGroupClick(item.id)}
              onkeydown={(e) => e.key === 'Enter' && handleGroupClick(item.id)}
            >
              <button
                data-layer-action
                class="shrink-0 p-0.5 rounded cursor-pointer hover:bg-[var(--panel3)] transition-colors"
                onclick={(e) => {
                  e.stopPropagation()
                  toggleCollapse(item.id)
                }}
                title={collapsed ? 'Expand' : 'Collapse'}
              >
                {#if collapsed}
                  <ChevronRight size={11} />
                {:else}
                  <ChevronDown size={11} />
                {/if}
              </button>
              <Folder size={12} class="shrink-0 opacity-70" />

              {#if renamingGroupId === item.id}
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  class="flex-1 min-w-0 bg-[var(--panel3)] border border-transparent rounded px-1 py-0.5 text-xs text-zinc-100 outline-none focus:border-primary"
                  value={renameValue}
                  oninput={(e) => (renameValue = e.currentTarget.value)}
                  onblur={commitRename}
                  onkeydown={(e) => {
                    if (e.key === 'Enter') commitRename()
                    if (e.key === 'Escape') renamingGroupId = null
                  }}
                  onclick={(e) => e.stopPropagation()}
                  autofocus
                />
              {:else}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="flex-1 min-w-0 truncate font-mono text-xs"
                  ondblclick={(e) => {
                    e.stopPropagation()
                    startRename(item.id, item.name)
                  }}
                  title="Double-click to rename">{item.name}</span
                >
              {/if}

              <span class="shrink-0 text-[9px] text-zinc-500 tabular-nums"
                >{item.elements.length}</span
              >

              <div
                class="shrink-0 flex items-center gap-0.5 opacity-0 group-hover/grp:opacity-100 transition-opacity ml-0.5"
              >
                <button
                  data-layer-action
                  onclick={(e) => {
                    e.stopPropagation()
                    app.deleteGroup(item.id)
                  }}
                  class="p-0.5 rounded text-zinc-600 cursor-pointer hover:text-destructive transition-colors"
                  title="Ungroup"
                >
                  <X size={11} />
                </button>
              </div>
            </div>

            <!-- Grouped element rows -->
            {#if !collapsed}
              <ul class="mt-0.5 space-y-0.5 pl-4">
                {#each item.elements as el, elIdx (el.id)}
                  {@const selected = app.selectedElementIds.includes(el.id)}
                  {@const Icon = ICONS[el.type] ?? BarChart2}
                  {@const showTopDrop =
                    dropGroupedGroupId === item.id &&
                    dropGroupedIndex === elIdx &&
                    draggingId !== el.id}
                  {@const showBottomDrop =
                    dropGroupedGroupId === item.id &&
                    dropGroupedIndex === item.elements.length &&
                    elIdx === item.elements.length - 1 &&
                    draggingId !== el.id}
                  <li
                    data-grouped-item={item.id}
                    data-grouped-item-idx={elIdx}
                    onpointerdown={(e) =>
                      onRowPointerDown(e, el.id, 'element', item.id)}
                    class={`group/el relative rounded-[6px] cursor-grab active:cursor-grabbing
                      ${draggingId === el.id ? 'opacity-45' : ''}
                      ${showTopDrop ? 'before:absolute before:left-0 before:right-0 before:-top-0.5 before:h-px before:bg-primary' : ''}
                      ${showBottomDrop ? 'after:absolute after:left-0 after:right-0 after:-bottom-0.5 after:h-px after:bg-primary' : ''}`}
                  >
                    <button
                      onclick={(e) => selectElement(e, el.id)}
                      class={`w-full flex items-center gap-2 px-2.5 py-2 pr-24 rounded-[6px] text-left text-sm transition-colors
                        cursor-grab active:cursor-grabbing
                        ${
                          selected
                            ? 'bg-primary/10 text-primary border border-primary/30'
                            : 'text-zinc-300 hover:bg-[var(--panel2)] hover:text-zinc-100'
                        }`}
                    >
                      <Icon size={12} class="shrink-0 opacity-60" />
                      <span class="truncate font-mono text-xs">{el.name}</span>
                      {#if el.unit}
                        <span
                          class="shrink-0 text-[9px] font-medium px-1 py-0.5 rounded bg-[var(--panel3)] text-[var(--dim)] uppercase tracking-wide"
                          >{el.unit === 'imperial' ? 'imp' : el.unit}</span
                        >
                      {/if}
                    </button>
                    <div
                      class="absolute right-1 top-1/2 -translate-y-1/2 flex items-center gap-0.5 opacity-0 group-hover/el:opacity-100 transition-opacity"
                    >
                      <button
                        data-layer-action
                        onclick={(e) => {
                          e.stopPropagation()
                          app.moveElementLayer(el.id, 1)
                        }}
                        class="p-1 rounded text-zinc-600 cursor-pointer hover:text-zinc-200 transition-colors"
                        title="Bring forward"
                        tabindex="-1"><ArrowUp size={11} /></button
                      >
                      <button
                        data-layer-action
                        onclick={(e) => {
                          e.stopPropagation()
                          app.moveElementLayer(el.id, -1)
                        }}
                        class="p-1 rounded text-zinc-600 cursor-pointer hover:text-zinc-200 transition-colors"
                        title="Send backward"
                        tabindex="-1"><ArrowDown size={11} /></button
                      >
                      <button
                        data-layer-action
                        onclick={(e) => {
                          e.stopPropagation()
                          app.removeElementFromGroups(el.id)
                        }}
                        class="p-1 rounded text-zinc-600 cursor-pointer hover:text-zinc-400 transition-colors"
                        title="Remove from group"
                        tabindex="-1"><X size={11} /></button
                      >
                      <button
                        data-layer-action
                        onclick={(e) => {
                          e.stopPropagation()
                          app.removeElement(el.id)
                        }}
                        class="p-1 rounded text-zinc-600 cursor-pointer hover:text-destructive transition-colors"
                        title="Remove"
                        tabindex="-1"><Trash2 size={11} /></button
                      >
                    </div>
                  </li>
                {/each}
              </ul>
            {/if}
          </li>
        {:else}
          <!-- Ungrouped element row (draggable) -->
          {@const el = item}
          {@const selected = app.selectedElementIds.includes(el.id)}
          {@const Icon = ICONS[el.type] ?? BarChart2}
          {@const listIdx = listItems().findIndex(
            (it) => it.id === el.id && it.kind === 'element',
          )}
          <li
            data-list-item={listIdx}
            onpointerdown={(e) => onRowPointerDown(e, el.id, 'element')}
            class={`relative group/el rounded-[6px]
              ${draggingId === el.id ? 'opacity-45' : ''}
              ${dropIndex === listIdx && draggingId !== el.id ? 'before:absolute before:left-0 before:right-0 before:-top-0.5 before:h-px before:bg-primary' : ''}
              ${dropIndex === listIdx + 1 && draggingId !== el.id ? 'after:absolute after:left-0 after:right-0 after:-bottom-0.5 after:h-px after:bg-primary' : ''}`}
          >
            <button
              onclick={(e) => selectElement(e, el.id)}
              class={`w-full flex items-center gap-2 px-2.5 py-2 pr-20 rounded-[6px] text-left text-sm transition-colors
                cursor-grab active:cursor-grabbing
                ${
                  selected
                    ? 'bg-primary/10 text-primary border border-primary/30'
                    : 'text-zinc-300 hover:bg-[var(--panel2)] hover:text-zinc-100'
                }`}
            >
              <Icon size={12} class="shrink-0 opacity-60" />
              <span class="truncate font-mono text-xs">{el.name}</span>
              {#if el.unit}
                <span
                  class="shrink-0 text-[9px] font-medium px-1 py-0.5 rounded bg-[var(--panel3)] text-[var(--dim)] uppercase tracking-wide"
                  >{el.unit === 'imperial' ? 'imp' : el.unit}</span
                >
              {/if}
            </button>
            <div
              class="absolute right-1 top-1/2 -translate-y-1/2 flex items-center gap-0.5 opacity-0 group-hover/el:opacity-100 transition-opacity"
            >
              <button
                data-layer-action
                onclick={(e) => {
                  e.stopPropagation()
                  app.moveElementLayer(el.id, 1)
                }}
                class="p-1 rounded text-zinc-600 cursor-pointer hover:text-zinc-200 transition-colors"
                title="Bring forward"
                tabindex="-1"><ArrowUp size={11} /></button
              >
              <button
                data-layer-action
                onclick={(e) => {
                  e.stopPropagation()
                  app.moveElementLayer(el.id, -1)
                }}
                class="p-1 rounded text-zinc-600 cursor-pointer hover:text-zinc-200 transition-colors"
                title="Send backward"
                tabindex="-1"><ArrowDown size={11} /></button
              >
              <button
                data-layer-action
                onclick={(e) => {
                  e.stopPropagation()
                  app.removeElement(el.id)
                }}
                class="p-1 rounded text-zinc-600 cursor-pointer hover:text-destructive transition-colors"
                title="Remove"
                tabindex="-1"><Trash2 size={11} /></button
              >
            </div>
          </li>
        {/if}
      {/each}
      <!-- Drop zone sentinel: shows indicator line when dropping after all items -->
      <li
        class={`h-1 relative ${dropIndex === listItems().length ? 'before:absolute before:left-0 before:right-0 before:top-0 before:h-px before:bg-primary' : ''}`}
      ></li>
    </ul>
  {/if}
</section>

{#if addMenuOpen && app.config}
  <div
    role="dialog"
    aria-modal="true"
    aria-label="Add element"
    tabindex="-1"
    class="fixed inset-0 z-[60] flex items-center justify-center p-4"
    onmousedown={(e) => {
      if (e.target === e.currentTarget) addMenuOpen = false
    }}
  >
    <div class="absolute inset-0 bg-black/60 backdrop-blur-sm"></div>

    <div
      class="relative z-10 w-[420px] max-w-full rounded-[12px] border border-white/[0.08] bg-[var(--panel)] shadow-2xl"
    >
      <div
        class="flex items-center justify-between border-b border-white/[0.06] px-4 py-3"
      >
        <div>
          <p class="text-sm font-semibold text-zinc-100">Add element</p>
          <p class="mt-0.5 text-[11px] text-zinc-500">
            Choose an overlay type to place on the canvas.
          </p>
        </div>
        <button
          onclick={() => (addMenuOpen = false)}
          class="cursor-pointer rounded p-1 text-zinc-500 transition-colors hover:bg-[var(--panel3)] hover:text-zinc-200"
          title="Close"
        >
          <X size={15} />
        </button>
      </div>

      <div class="grid grid-cols-2 gap-2 p-3">
        {#each ADD_PRESETS as preset (preset.key)}
          {@const Icon = ICONS[preset.icon]}
          <button
            onclick={() => addPreset(preset)}
            class="group flex min-h-16 cursor-pointer items-center gap-3 rounded-[8px] bg-[var(--panel2)] px-3 py-2 text-left transition-colors hover:bg-[var(--panel3)]"
            title={preset.title}
          >
            <span
              class="flex h-9 w-9 shrink-0 items-center justify-center rounded-[7px] bg-[var(--panel3)] text-zinc-400 transition-colors group-hover:text-zinc-100"
            >
              <Icon size={15} />
            </span>
            <span class="min-w-0">
              <span
                class="block truncate text-xs font-medium leading-4 text-zinc-200"
                >{presetLabel(preset.title)}</span
              >
              <span class="block truncate text-[10px] leading-4 text-zinc-500"
                >{presetHint(preset)}</span
              >
            </span>
          </button>
        {/each}
      </div>
    </div>
  </div>
{/if}

{#if addImagePending}
  <AssetPicker
    current=""
    onselect={onImageSelected}
    oncancel={() => (addImagePending = false)}
  />
{/if}
