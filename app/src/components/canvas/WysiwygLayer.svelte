<script>
  /**
   * Transparent SVG layer positioned over the canvas.
   *
   * Bounds strategy (two sources, merged per element):
   *   1. measuredElements — pixel-perfect Skia bounds returned by the Rust renderer.
   *      These are used whenever available (after the first frame loads).
   *   2. Config-derived fallback — used before the first frame is ready.
   *      Config `y` for text is the Skia baseline, so we subtract ~0.8×font_size
   *      to approximate the visual top of the glyph.
   */
  import { getContext, untrack, onMount } from 'svelte'
  import { SvelteMap, SvelteSet } from 'svelte/reactivity'
  import ElementHandle from './ElementHandle.svelte'

  const app = getContext('app')

  // Pixel-perfect bounds from the Rust renderer — { id, x, y, w, h }[]
  // frameImage: full rendered scene PNG (data URL) at the preview render size.
  // sceneWidth/sceneHeight: the dims the backend rendered + measured at (the
  // preview box CSS size × devicePixelRatio, capped at the export resolution).
  // The SVG overlay must use that same space so measured element bounds line up;
  // config-derived fallbacks and drag write-back are converted between
  // authored ↔ render space via `authorScale`.
  let {
    measuredElements = [],
    frameImage = null,
    zoom = 1,
    sceneWidth = 1920,
    sceneHeight = 1080,
  } = $props()

  let authoredHeight = $derived(app.config?.scene?.height ?? 1080)
  let authorScale = $derived(sceneHeight / (authoredHeight || sceneHeight))

  function elById(id) {
    return app.config?.elements?.find((e) => e.id === id) ?? null
  }

  // prettier-ignore
  const POINT_FRACS = {
    'top-left': [0, 0], top: [0.5, 0], 'top-right': [1, 0],
    left: [0, 0.5], center: [0.5, 0.5], right: [1, 0.5],
    'bottom-left': [0, 1], bottom: [0.5, 1], 'bottom-right': [1, 1],
  }

  // Authored-space (x, y) origin for an anchored element, mirroring the Rust
  // resolve pre-pass. Only used for pre-measure fallback bounds — once a frame
  // arrives, the renderer's measured bounds (already anchor-resolved) win.
  function anchoredXY(el) {
    const a = el.anchor
    if (!a?.target) return null
    const t = elById(a.target)
    if (!t || t.width == null || t.height == null) return null
    const [fx, fy] = POINT_FRACS[a.point ?? 'center'] ?? [0.5, 0.5]
    let x = (t.x ?? 0) + t.width * fx + (a.offset_x ?? 0)
    let y = (t.y ?? 0) + t.height * fy + (a.offset_y ?? 0)
    if (el.type === 'rect' || el.type === 'image') {
      const [sfx, sfy] = POINT_FRACS[a.self_point ?? 'center'] ?? [0.5, 0.5]
      x -= (el.width ?? 0) * sfx
      y -= (el.height ?? 0) * sfy
    }
    return { x, y }
  }

  // Fallback box origin for text elements: resolves anchors and shifts the
  // approximate box per text_align / vertical_align (config x is the
  // alignment point, y the baseline).
  function textFallbackXY(el, fs, w) {
    const ax = anchoredXY(el)
    let x = ax?.x ?? el.x ?? 100
    const y = ax?.y ?? el.y ?? 200
    if (el.text_align === 'center') x -= w / 2
    else if (el.text_align === 'right') x -= w
    const va = el.vertical_align
    const top =
      va === 'top'
        ? y
        : va === 'middle'
          ? y - fs / 2
          : va === 'bottom'
            ? y - fs
            : y - fs * 0.8
    return { x, y: top }
  }

  // Sticky measurement cache: a new frame's measuredElements may omit an
  // element briefly (e.g. while the post-edit re-render is in flight). Reusing
  // the last-known measurement keeps the selection box the size of the text
  // instead of falling back to an oversized synthetic estimate.
  const stickyMeasured = new SvelteMap()
  $effect(() => {
    for (const m of measuredElements) stickyMeasured.set(m.id, m)
  })

  let elements = $derived.by(() => {
    if (!app.config?.elements) return []
    const measured = new Map(measuredElements.map((e) => [e.id, e]))
    const s = authorScale
    // Config-derived fallback bounds are in authored coords; the rendered
    // image (and measured bounds) are in output coords — scale to match.
    const fb = (o) => ({
      id: o.id,
      x: o.x * s,
      y: o.y * s,
      w: o.w * s,
      h: o.h * s,
    })
    const byId = {}

    // Pick the best bounds source for a given element:
    //  • Dragging → use the pre-drag snapshot so dragDelta isn't double-counted.
    //  • Recently moved (stale measured) → skip measured, use config-derived.
    //  • Otherwise → prefer pixel-perfect measured (sticky, so a missing
    //    current-frame measurement falls through to the last-known one rather
    //    than the loose synthetic estimate).
    function boundsFor(id) {
      if (liveResize?.id === id) return liveResize.bounds
      if (draggingIds.has(id)) return dragBase.baseElements.get(id) ?? null
      if (movedIds.has(id)) return null
      if (dragOffsets.has(id)) {
        const { dx, dy } = dragOffsets.get(id)
        const base = stickyMeasured.get(id)
        if (base)
          return { id, x: base.x + dx, y: base.y + dy, w: base.w, h: base.h }
        return null
      }
      return measured.get(id) ?? stickyMeasured.get(id) ?? null
    }

    for (const el of app.config.elements) {
      const id = el.id
      if (el.type === 'label') {
        const fs = el.font_size ?? 32
        const text = el.text ?? 'LABEL'
        const charCount = Array.from(text).length
        const letterSpacing = el.letter_spacing ?? 0
        const w = Math.max(
          charCount * fs * 0.58 + Math.max(charCount - 1, 0) * letterSpacing,
          fs,
        )
        byId[id] =
          boundsFor(id) ?? fb({ id, ...textFallbackXY(el, fs, w), w, h: fs })
      } else if (el.type === 'value') {
        const fs = el.font_size ?? 48
        // ~3.5 chars at 0.58em average — covers typical metric values like
        // "120", "25.4", "1:42". Real bounds replace this on the next frame.
        byId[id] =
          boundsFor(id) ??
          fb({ id, ...textFallbackXY(el, fs, fs * 2), w: fs * 2, h: fs })
      } else if (
        el.type === 'plot' ||
        el.type === 'meter' ||
        el.type === 'gauge'
      ) {
        byId[id] =
          boundsFor(id) ??
          fb({
            id,
            x: el.x ?? 50,
            y: el.y ?? 400,
            w: el.width ?? 400,
            h: el.height ?? 150,
          })
      } else if (el.type === 'rect' || el.type === 'image') {
        const ax = anchoredXY(el)
        byId[id] =
          boundsFor(id) ??
          fb({
            id,
            x: ax?.x ?? el.x ?? 100,
            y: ax?.y ?? el.y ?? 100,
            w: el.width ?? 300,
            h: el.height ?? 200,
          })
      }
    }
    return [...(app.elementLayerOrder ?? [])]
      .map((id) => byId[id])
      .filter(Boolean)
  })

  function handleLabel(id) {
    const el = elById(id)
    if (!el) return id
    if (el.type === 'label') return el.text ?? 'label'
    if (el.type === 'value') return el.value ?? 'value'
    if (el.type === 'meter') return `${el.value} meter`
    if (el.type === 'gauge') return `${el.value} gauge`
    if (el.type === 'rect') return 'rect'
    if (el.type === 'image') return el.file || 'image'
    return `${el.value} chart`
  }

  let selectedSet = $derived(new Set(app.selectedElementIds ?? []))
  // During a group drag: { leaderId, dx, dy }. Non-leader selected handles
  // follow via groupOffset so the whole selection moves in unison.
  let liveGroup = $state(null)

  // ── Smart center guides ─────────────────────────────────────────────────────
  // While a single element is dragged, its center snaps to the canvas center
  // and a crimson guide line marks the active axis. [{ axis: 'v'|'h', pos }].
  let activeGuides = $state([])
  // The leader's own ElementHandle only knows its raw drag delta; this is the
  // snap correction (snapped − raw) so its box + floated pixels track the snap.
  let leaderSnap = $state({ dx: 0, dy: 0 })
  // Snap zone as a fraction of the scene dimension — resolution-independent in
  // screen terms since the canvas fills roughly the same display width.
  const SNAP_FRACTION = 0.005

  // Snap a raw drag delta so the element's center clicks onto the canvas center
  // line when within threshold. Single-element drags only (groups pass through).
  function snapDelta(id, dx, dy) {
    if (isGroupDrag(id)) return { dx, dy, guides: [] }
    const base =
      dragBase?.baseElements.get(id) ?? elements.find((e) => e.id === id)
    if (!base) return { dx, dy, guides: [] }
    const tx = sceneWidth / 2
    const ty = sceneHeight / 2
    const cx = base.x + base.w / 2 + dx
    const cy = base.y + base.h / 2 + dy
    const guides = []
    let sdx = dx
    let sdy = dy
    if (Math.abs(cx - tx) <= sceneWidth * SNAP_FRACTION) {
      sdx = dx + (tx - cx)
      guides.push({ axis: 'v', pos: tx })
    }
    if (Math.abs(cy - ty) <= sceneHeight * SNAP_FRACTION) {
      sdy = dy + (ty - cy)
      guides.push({ axis: 'h', pos: ty })
    }
    return { dx: sdx, dy: sdy, guides }
  }

  // Center the primary-selected element on the canvas. Uses the same
  // output-space bounds the handles draw, so it works for every element type
  // and honors current text alignment. Registered on app so the properties
  // panel's Center buttons can call it. axis: 'h' | 'v' | 'both'.
  function alignToCanvasCenter(id, axis) {
    const el = elById(id)
    if (!el || el.locked) return
    const b = elements.find((e) => e.id === id)
    if (!b) return
    const s = authorScale || 1
    const wantH = axis === 'h' || axis === 'both'
    const wantV = axis === 'v' || axis === 'both'
    const ddx = wantH ? (sceneWidth / 2 - (b.x + b.w / 2)) / s : 0
    const ddy = wantV ? (sceneHeight / 2 - (b.y + b.h / 2)) / s : 0
    const preConfig = JSON.stringify(app.config)
    let updates
    if (el.anchor?.target) {
      // Anchored elements derive x/y from their target — shift the offset.
      const a = el.anchor
      updates = {
        anchor: {
          ...a,
          offset_x: Math.round((a.offset_x ?? 0) + ddx),
          offset_y: Math.round((a.offset_y ?? 0) + ddy),
        },
      }
    } else {
      updates = {}
      if (wantH) updates.x = Math.round((el.x ?? 0) + ddx)
      if (wantV) updates.y = Math.round((el.y ?? 0) + ddy)
    }
    app.commitElementUpdate(preConfig, id, updates)
    // Show config-derived bounds at the new spot until the fresh frame lands.
    const next = new SvelteSet(movedIds)
    next.add(id)
    movedIds = next
  }

  onMount(() => {
    app.setAlignHandler(alignToCanvasCenter)
    return () => app.setAlignHandler(null)
  })

  // Live rotation state: { id, degrees } while the user is dragging the handle.
  let liveRotation = $state(null)

  // Live resize state: { id, bounds } while the user is dragging a resize corner.
  // Output-space bounds (x, y, w, h). Config is NOT mutated until release.
  let liveResize = $state(null)

  // Snapshot captured at drag start so live updates don't shift the base coords.
  // { preDragConfig: string, positions: Map<id,{category,idx,x,y}>, baseElements: Map<id,{x,y,w,h}> }
  let dragBase = $state(null)

  // Elements whose config position was just committed — skip stale measured
  // bounds for them until the next rendered frame arrives.
  let movedIds = $state(new Set())

  // Drag-offset cache: after a drop, stickyMeasured bounds are shifted by
  // (dx, dy) so text/metric elements keep their precise measured shape instead
  // of flashing to the approximate config-derived fallback.  Cleared when a
  // fresh render frame arrives (same effect that clears movedIds).
  const dragOffsets = new SvelteMap() // id → { dx, dy } in output px

  // Cropped pixels of the dragged element(s), floated under the cursor so the
  // real graphic moves during a drag (the box alone feels dead).
  // id → { url, baseX, baseY, x, y, w, h } — all output px.
  const dragSnaps = new SvelteMap()

  // When a new measured frame arrives, the stale-bounds guard AND the drag
  // snapshot are no longer needed — the fresh frame already shows the element
  // at its new position. untrack keeps these out of the effect's dependency
  // set so it only fires on a genuine new frame, not on our own drag writes.
  $effect(() => {
    void measuredElements
    untrack(() => {
      if (movedIds.size > 0) movedIds = new Set()
      if (dragOffsets.size > 0) dragOffsets.clear()
      if (dragSnaps.size > 0) dragSnaps.clear()
    })
  })

  let draggingIds = $derived(
    dragBase ? new Set([...dragBase.positions.keys()]) : new Set(),
  )

  function getRotation(id) {
    const el = elById(id)
    if (!el || !['plot', 'meter', 'gauge', 'rect', 'image'].includes(el.type))
      return 0
    return el.rotation ?? 0
  }

  function rotationFor(id) {
    if (liveRotation?.id === id) return liveRotation.degrees
    return getRotation(id)
  }

  function handleRotate(id, degrees) {
    liveRotation = { id, degrees }
  }

  function handleRotateEnd(id, degrees) {
    liveRotation = null
    const el = elById(id)
    if (!el || !['plot', 'meter', 'gauge', 'rect', 'image'].includes(el.type))
      return
    app.updateElement(id, { rotation: Math.round(degrees) })
  }

  function isGroupDrag(id) {
    return selectedSet.size > 1 && selectedSet.has(id)
  }

  function handleSelect(id, e) {
    if (e?.shiftKey) {
      app.toggleElementSelection(id)
    } else if (isGroupDrag(id)) {
      // Keep the multi-selection so a plain drag moves the whole group.
    } else {
      app.selectedElementId = id
    }
  }

  // Capture pre-drag state on the first move so we always delta from the
  // original authored position, not from a live-updated intermediate one.
  function ensureDragBase(leadId) {
    if (dragBase) return
    const ids = isGroupDrag(leadId) ? [...selectedSet] : [leadId]
    const positions = new SvelteMap()
    const baseElements = new SvelteMap()
    for (const sid of ids) {
      const item = elById(sid)
      if (!item || item.locked) continue
      positions.set(sid, {
        id: sid,
        x: item.x ?? 0,
        y: item.y ?? 0,
        ox: item.anchor?.offset_x ?? 0,
        oy: item.anchor?.offset_y ?? 0,
      })
      const elData = elements.find((e) => e.id === sid)
      if (elData)
        baseElements.set(sid, {
          id: sid,
          x: elData.x,
          y: elData.y,
          w: elData.w,
          h: elData.h,
        })
    }
    // Elements anchored (directly or via a chain) to anything being dragged
    // ride along visually — Rust re-derives their position on the commit
    // render, so they get snaps + live offsets but no config writes.
    const followerIds = new SvelteSet()
    let grew = true
    while (grew) {
      grew = false
      for (const el of app.config?.elements ?? []) {
        const target = el.anchor?.target
        if (!target || positions.has(el.id) || followerIds.has(el.id)) continue
        if (!positions.has(target) && !followerIds.has(target)) continue
        followerIds.add(el.id)
        const elData = elements.find((e) => e.id === el.id)
        if (elData)
          baseElements.set(el.id, {
            id: el.id,
            x: elData.x,
            y: elData.y,
            w: elData.w,
            h: elData.h,
          })
        grew = true
      }
    }
    dragBase = {
      preDragConfig: JSON.stringify(app.config),
      positions,
      baseElements,
      followerIds,
    }
    buildDragSnaps([...baseElements.keys()])
  }

  // Crop each dragged element out of the current rendered frame so its real
  // pixels can be floated under the cursor. Uses SVG viewBox clipping so the
  // original frame image is referenced directly — no canvas re-encode that
  // corrupts semi-transparent pixels via premultiplied alpha.
  function buildDragSnaps(ids) {
    if (!frameImage) return
    for (const id of ids) {
      const b = dragBase?.baseElements.get(id)
      if (!b) continue
      const el = elById(id)
      // Outside-stroke borders extend beyond element bounds by border_width px;
      // use a pad large enough to capture the full border.
      const hasBorder =
        (el?.type === 'rect' || el?.type === 'meter') && el.border_color
      const pad = hasBorder ? Math.max(2, (el.border_width ?? 2) + 2) : 2
      const sx = Math.max(0, Math.floor(b.x - pad))
      const sy = Math.max(0, Math.floor(b.y - pad))
      const sw = Math.min(sceneWidth - sx, Math.ceil(b.w + pad * 2))
      const sh = Math.min(sceneHeight - sy, Math.ceil(b.h + pad * 2))
      if (sw <= 0 || sh <= 0) continue
      const prev = dragSnaps.get(id)
      dragSnaps.set(id, {
        url: frameImage,
        sx,
        sy,
        sw,
        sh,
        baseX: sx,
        baseY: sy,
        x: prev?.x ?? sx,
        y: prev?.y ?? sy,
        w: sw,
        h: sh,
      })
    }
  }

  function moveDragSnaps(dx, dy) {
    for (const [id, s] of dragSnaps) {
      dragSnaps.set(id, { ...s, x: s.baseX + dx, y: s.baseY + dy })
    }
  }

  // dx/dy are in output space; config coords are authored — use the pre-drag
  // snapshot so repeated calls with the same delta stay idempotent. Anchored
  // elements commit the delta to their anchor offset (Rust derives x/y from
  // the target); if their target is also in the drag set, no write at all —
  // they follow automatically.
  function moveFor(id, dx, dy, draggedIds) {
    const item = elById(id)
    if (!item) return null
    const s = authorScale || 1
    const base = dragBase?.positions.get(id)
    if (item.anchor?.target) {
      if (draggedIds.has(item.anchor.target)) return null
      const ox = base?.ox ?? item.anchor.offset_x ?? 0
      const oy = base?.oy ?? item.anchor.offset_y ?? 0
      return {
        id,
        anchor: {
          ...item.anchor,
          offset_x: Math.round(ox + dx / s),
          offset_y: Math.round(oy + dy / s),
        },
      }
    }
    if (base) return { id, x: base.x + dx / s, y: base.y + dy / s }
    return { id, x: (item.x ?? 0) + dx / s, y: (item.y ?? 0) + dy / s }
  }

  function handleDrag(id, dx, dy) {
    ensureDragBase(id)
    // Snap the delta to the canvas center and surface the guide line.
    const snapped = snapDelta(id, dx, dy)
    activeGuides = snapped.guides
    leaderSnap = { dx: snapped.dx - dx, dy: snapped.dy - dy }
    // Float the cropped real pixels under the cursor; group members and
    // anchored followers ride along via groupOffsetFor. No live re-render: a
    // server-side render can't keep up with a drag, and queuing them only
    // delays the single commit render we want immediately on drop.
    liveGroup = { leaderId: id, dx: snapped.dx, dy: snapped.dy }
    moveDragSnaps(snapped.dx, snapped.dy)
  }

  function handleDragEnd(id, dx, dy) {
    ensureDragBase(id)
    // Commit the snapped position so the drop lands exactly on center.
    const snapped = snapDelta(id, dx, dy)
    dx = snapped.dx
    dy = snapped.dy
    activeGuides = []
    leaderSnap = { dx: 0, dy: 0 }
    const ids = isGroupDrag(id) ? [...selectedSet] : [id]
    const idSet = new Set(ids)
    const moves = ids.map((sid) => moveFor(sid, dx, dy, idSet)).filter(Boolean)

    // Record the drag offset so boundsFor can return stickyMeasured+offset
    // (precise shape at new position) instead of the approximate config
    // fallback. Followers and write-skipped anchored members moved too.
    for (const m of moves) dragOffsets.set(m.id, { dx, dy })
    for (const sid of dragBase?.followerIds ?? [])
      dragOffsets.set(sid, { dx, dy })
    for (const sid of ids) {
      const target = elById(sid)?.anchor?.target
      if (target && idSet.has(target)) dragOffsets.set(sid, { dx, dy })
    }

    // Freeze the snapshot at the drop point; it stays until the fresh frame
    // arrives (cleared by the measuredElements effect), so no blank gap.
    moveDragSnaps(dx, dy)
    app.commitElementPositions(dragBase?.preDragConfig ?? null, moves)
    dragBase = null
    liveGroup = null
  }

  function groupOffsetFor(id) {
    if (liveGroup) {
      // The leader's own handle already applies the raw delta — add just the
      // snap correction so its box tracks the snapped (floated) pixels.
      if (liveGroup.leaderId === id) return leaderSnap
      if (
        selectedSet.size > 1 &&
        selectedSet.has(id) &&
        selectedSet.has(liveGroup.leaderId)
      )
        return { dx: liveGroup.dx, dy: liveGroup.dy }
      if (dragBase?.followerIds?.has(id))
        return { dx: liveGroup.dx, dy: liveGroup.dy }
    }
    return { dx: 0, dy: 0 }
  }

  // ── Resize ────────────────────────────────────────────────────────────────
  // Captured at the start of a resize so deltas are always relative to the
  // original authored dimensions, not intermediate live-updated values.
  let resizeBase = $state(null) // { preConfig, id, origX, origY, origW, origH }

  function applyResizeDelta(
    origX,
    origY,
    origW,
    origH,
    corner,
    dx,
    dy,
    shiftKey,
    naturalW = null,
    naturalH = null,
  ) {
    const s = authorScale || 1
    // dx/dy are in output px; element coords are authored → undo the scale.
    const adx = dx / s
    const ady = dy / s

    // Raw dimension change per corner (positive = larger)
    let dw, dh
    switch (corner) {
      case 'br':
        dw = adx
        dh = ady
        break
      case 'bl':
        dw = -adx
        dh = ady
        break
      case 'tr':
        dw = adx
        dh = -ady
        break
      case 'tl':
        dw = -adx
        dh = -ady
        break
      default:
        dw = adx
        dh = ady
    }

    let newW = origW + dw
    let newH = origH + dh

    if (shiftKey && origW > 0 && origH > 0) {
      const aw = naturalW ?? origW
      const ah = naturalH ?? origH
      const ratio = aw / ah
      // Lock aspect: project onto dominant axis
      if (Math.abs(dw / origW) >= Math.abs(dh / origH)) {
        newH = newW / ratio
      } else {
        newW = newH * ratio
      }
    }

    // Minimum size
    newW = Math.max(newW, 4)
    newH = Math.max(newH, 4)

    // Compute x/y: the corner opposite to the dragged one is fixed.
    let newX = origX,
      newY = origY
    switch (corner) {
      case 'br':
        /* top-left fixed — x/y unchanged */ break
      case 'bl':
        newX = origX + origW - newW
        break
      case 'tr':
        newY = origY + origH - newH
        break
      case 'tl':
        newX = origX + origW - newW
        newY = origY + origH - newH
        break
    }

    return {
      x: Math.round(newX),
      y: Math.round(newY),
      width: Math.round(newW),
      height: Math.round(newH),
    }
  }

  function handleResize(id, corner, dx, dy, shiftKey) {
    if (!resizeBase) {
      const el = elById(id)
      if (!el) return
      resizeBase = {
        preConfig: JSON.stringify(app.config),
        id,
        origX: el.x ?? 0,
        origY: el.y ?? 0,
        origW: el.width ?? 100,
        origH: el.height ?? 100,
        naturalW: el.natural_width ?? null,
        naturalH: el.natural_height ?? null,
      }
    }
    const { origX, origY, origW, origH, naturalW, naturalH } = resizeBase
    const lockAspect = shiftKey || elById(id)?.type === 'image'
    const updates = applyResizeDelta(
      origX,
      origY,
      origW,
      origH,
      corner,
      dx,
      dy,
      lockAspect,
      naturalW,
      naturalH,
    )
    // Store live bounds locally — do NOT write to config until release.
    // This keeps the reactive cascade (ElementProperties, CenterCanvas debounce,
    // etc.) from firing on every pointer move.
    const s = authorScale || 1
    liveResize = {
      id,
      bounds: {
        x: updates.x * s,
        y: updates.y * s,
        w: updates.width * s,
        h: updates.height * s,
      },
    }
  }

  function handleResizeEnd(id, corner, dx, dy, shiftKey) {
    if (!resizeBase) return
    const { preConfig, origX, origY, origW, origH, naturalW, naturalH } =
      resizeBase
    const lockAspect = shiftKey || elById(id)?.type === 'image'
    const updates = applyResizeDelta(
      origX,
      origY,
      origW,
      origH,
      corner,
      dx,
      dy,
      lockAspect,
      naturalW,
      naturalH,
    )
    app.commitElementUpdate(preConfig, id, updates)
    resizeBase = null
    liveResize = null
    // Mark moved so config-derived bounds are used until the fresh render arrives.
    const next = new SvelteSet(movedIds)
    next.add(id)
    movedIds = next
  }

  // ── Marquee (rubber-band) selection ───────────────────────────────────────
  let marquee = $state(null) // normalized scene-coord rect being drawn
  let marqueeStart = null // scene-coord anchor
  let marqueeClient = null // client px anchor (drag-vs-click detection)
  let marqueeMoved = false

  function clientToScene(svg, cx, cy) {
    const rect = svg.getBoundingClientRect()
    if (!rect.width || !rect.height) return { x: 0, y: 0 }
    const vb = svg.viewBox.baseVal
    return {
      x: (cx - rect.left) * (vb.width / rect.width),
      y: (cy - rect.top) * (vb.height / rect.height),
    }
  }

  function bgPointerDown(e) {
    const svg = e.currentTarget.ownerSVGElement
    if (!svg) return
    const p = clientToScene(svg, e.clientX, e.clientY)
    marqueeStart = p
    marqueeClient = { cx: e.clientX, cy: e.clientY }
    marqueeMoved = false
    marquee = { x: p.x, y: p.y, w: 0, h: 0 }
    e.currentTarget.setPointerCapture(e.pointerId)
  }

  function bgPointerMove(e) {
    if (!marqueeStart) return
    const svg = e.currentTarget.ownerSVGElement
    if (!svg) return
    if (
      Math.abs(e.clientX - marqueeClient.cx) > 3 ||
      Math.abs(e.clientY - marqueeClient.cy) > 3
    )
      marqueeMoved = true
    const p = clientToScene(svg, e.clientX, e.clientY)
    marquee = {
      x: Math.min(marqueeStart.x, p.x),
      y: Math.min(marqueeStart.y, p.y),
      w: Math.abs(p.x - marqueeStart.x),
      h: Math.abs(p.y - marqueeStart.y),
    }
  }

  function bgPointerUp() {
    if (!marqueeStart) return
    if (marqueeMoved && marquee) {
      const minX = marquee.x
      const minY = marquee.y
      const maxX = marquee.x + marquee.w
      const maxY = marquee.y + marquee.h
      const hit = elements
        .filter(
          (el) =>
            !(
              el.x + el.w < minX ||
              el.x > maxX ||
              el.y + el.h < minY ||
              el.y > maxY
            ),
        )
        .map((el) => el.id)
      app.setSelectedElements(hit)
    } else {
      app.selectedElementId = null // plain click on empty space → deselect
    }
    marquee = null
    marqueeStart = null
  }
</script>

{#if app.config}
  <svg
    viewBox={`0 0 ${sceneWidth} ${sceneHeight}`}
    style="position:absolute; inset:0; width:100%; height:100%; overflow:visible; pointer-events:none"
    xmlns="http://www.w3.org/2000/svg"
  >
    <!-- Background: drag to marquee-select, click to deselect.
       FIRST so handles paint on top -->
    <rect
      role="presentation"
      x={0}
      y={0}
      width={sceneWidth}
      height={sceneHeight}
      fill="transparent"
      style="pointer-events:all; cursor:crosshair"
      onpointerdown={bgPointerDown}
      onpointermove={bgPointerMove}
      onpointerup={bgPointerUp}
      onkeydown={(e) => {
        if (e.key === 'Escape') app.selectedElementId = null
      }}
    />

    <!-- Cropped real pixels of the dragged element(s), under the handle boxes
       so the border/handles stay on top. Nested SVG viewBox clips the full
       frame image to the element region without re-encoding, preserving
       semi-transparent pixels correctly. -->
    {#each [...dragSnaps] as [sid, s] (sid)}
      <svg
        x={s.x}
        y={s.y}
        width={s.w}
        height={s.h}
        viewBox="{s.sx} {s.sy} {s.sw} {s.sh}"
        overflow="hidden"
        style="pointer-events:none"
      >
        <image
          href={s.url}
          x="0"
          y="0"
          width={sceneWidth}
          height={sceneHeight}
          preserveAspectRatio="none"
          style="pointer-events:none"
        />
      </svg>
    {/each}

    {#each elements as el (el.id)}
      <ElementHandle
        id={el.id}
        bounds={{ x: el.x, y: el.y, w: el.w, h: el.h }}
        label={handleLabel(el.id)}
        selected={selectedSet.has(el.id)}
        rotation={rotationFor(el.id)}
        groupOffset={groupOffsetFor(el.id)}
        {zoom}
        resizable={['rect', 'meter', 'gauge', 'plot', 'image'].includes(
          elById(el.id)?.type,
        )}
        locked={elById(el.id)?.locked === true}
        onselect={(e) => handleSelect(el.id, e)}
        ondrag={(dx, dy) => handleDrag(el.id, dx, dy)}
        ondragend={(dx, dy) => handleDragEnd(el.id, dx, dy)}
        onrotate={(deg) => handleRotate(el.id, deg)}
        onrotateend={(deg) => handleRotateEnd(el.id, deg)}
        onresize={(corner, dx, dy, shift) =>
          handleResize(el.id, corner, dx, dy, shift)}
        onresizeend={(corner, dx, dy, shift) =>
          handleResizeEnd(el.id, corner, dx, dy, shift)}
      />
    {/each}

    <!-- Center snap guides — crimson line on the axis the element is centered on -->
    {#each activeGuides as g (g.axis)}
      {#if g.axis === 'v'}
        <line
          x1={g.pos}
          y1={0}
          x2={g.pos}
          y2={sceneHeight}
          stroke="#dc143c"
          stroke-width="1"
          vector-effect="non-scaling-stroke"
          style="pointer-events:none"
        />
      {:else}
        <line
          x1={0}
          y1={g.pos}
          x2={sceneWidth}
          y2={g.pos}
          stroke="#dc143c"
          stroke-width="1"
          vector-effect="non-scaling-stroke"
          style="pointer-events:none"
        />
      {/if}
    {/each}

    {#if marquee && (marquee.w > 0 || marquee.h > 0)}
      <rect
        x={marquee.x}
        y={marquee.y}
        width={marquee.w}
        height={marquee.h}
        stroke-width="1"
        stroke-dasharray="4 3"
        style="pointer-events:none; fill: rgb(var(--primary-rgb) / 0.12); stroke: var(--primary)"
      />
    {/if}
  </svg>
{/if}
