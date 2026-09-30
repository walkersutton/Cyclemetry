<script>
  /**
   * A single draggable/selectable handle in the WYSIWYG SVG overlay.
   * Uses a dragDelta offset so the bounds prop stays reactive to parent
   * config changes even across drags.
   */

  // Circular-arrow cursor for the rotation handle (clockwise arc + arrowhead).
  const ROTATE_CURSOR = `url("data:image/svg+xml,${encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20">' +
      '<path d="M10 2A8 8 0 1 1 2 10" stroke="white" stroke-width="3" fill="none" stroke-linecap="round"/>' +
      '<path d="M10 2A8 8 0 1 1 2 10" stroke="#1a1a1a" stroke-width="1.5" fill="none" stroke-linecap="round"/>' +
      '<path d="M13 2L10 0L10 4Z" fill="white"/>' +
      '<path d="M12.5 2L10 0.5L10 3.5Z" fill="#1a1a1a"/>' +
      '</svg>',
  )}") 10 10, grab`

  const RESIZE_CURSORS = {
    tl: 'nw-resize',
    tr: 'ne-resize',
    bl: 'sw-resize',
    br: 'se-resize',
  }
  const CORNERS = ['tl', 'tr', 'bl', 'br']

  let {
    bounds = { x: 0, y: 0, w: 50, h: 30 },
    label = '',
    selected = false,
    rotation = 0,
    groupOffset = { dx: 0, dy: 0 }, // live offset when another group member is dragging
    resizable = false,
    locked = false,
    onselect, // (event) — event carries shiftKey for multi-select
    ondrag, // (dx, dy) live, every pointermove
    ondragend, // (dx, dy) in scene/overlay coords
    onrotate, // (degrees) live, every pointermove
    onrotateend, // (degrees) committed on pointerup
    onresize, // (corner, dx, dy, shiftKey) live, every pointermove
    onresizeend, // (corner, dx, dy, shiftKey) committed on pointerup
  } = $props()

  let dragging = $state(false)
  let dragOrigin = { mx: 0, my: 0 }
  let dragDelta = $state({ dx: 0, dy: 0 })

  // Pointer-move threshold (in screen pixels) below which a pointerdown→up is
  // treated as a plain click rather than a drag/resize/rotate. Without this,
  // selecting an element fires an ondragend with zero delta, which causes a
  // spurious config commit + movedIds entry in the parent and makes the
  // bounding box flash to its synthetic fallback for one frame cycle.
  const CLICK_PX = 3
  function pastClickThreshold(e, origin) {
    return (
      Math.abs(e.clientX - origin.mx) > CLICK_PX ||
      Math.abs(e.clientY - origin.my) > CLICK_PX
    )
  }

  // Display position: base bounds + live drag offset
  let d = $derived.by(() => ({
    x: bounds.x + dragDelta.dx + groupOffset.dx,
    y: bounds.y + dragDelta.dy + groupOffset.dy,
    w: bounds.w,
    h: bounds.h,
  }))

  let cx = $derived(d.x + d.w / 2)
  let cy = $derived(d.y + d.h / 2)

  const ROTATE_HANDLE_OFFSET = 32

  function screenToOverlayDelta(svg, mx0, my0, mx1, my1) {
    // Use getBoundingClientRect() instead of getScreenCTM(): the rect already
    // accounts for CSS ancestor transforms (incl. the stage zoom), whereas
    // WebKit's getScreenCTM() on SVG children omits them inconsistently.
    const rect = svg.getBoundingClientRect()
    if (!rect.width || !rect.height) return { dx: 0, dy: 0 }
    const vb = svg.viewBox.baseVal
    const scaleX = vb.width / rect.width
    const scaleY = vb.height / rect.height
    return { dx: (mx1 - mx0) * scaleX, dy: (my1 - my0) * scaleY }
  }

  function onpointerdown(e) {
    e.stopPropagation()
    onselect?.(e)
    if (locked) return // select is fine; drag is not
    dragging = true
    dragOrigin = { mx: e.clientX, my: e.clientY }
    dragDelta = { dx: 0, dy: 0 }
    e.currentTarget.setPointerCapture(e.pointerId)
  }

  function onpointermove(e) {
    if (!dragging) return
    const svg = e.currentTarget.ownerSVGElement
    if (!svg) return
    dragDelta = screenToOverlayDelta(
      svg,
      dragOrigin.mx,
      dragOrigin.my,
      e.clientX,
      e.clientY,
    )
    ondrag?.(dragDelta.dx, dragDelta.dy)
  }

  function onpointerup(e) {
    if (!dragging) return
    dragging = false
    if (pastClickThreshold(e, dragOrigin))
      ondragend?.(dragDelta.dx, dragDelta.dy)
    dragDelta = { dx: 0, dy: 0 }
  }

  // ── Rotation handle ───────────────────────────────────────────────────────
  let rotating = $state(false)
  let rotateStartAngle = 0
  let rotateStartValue = 0
  let rotateOrigin = { mx: 0, my: 0 }

  function sceneAngle(svg, mx, my) {
    const rect = svg.getBoundingClientRect()
    if (!rect.width || !rect.height) return 0
    const vb = svg.viewBox.baseVal
    const px = (mx - rect.left) * (vb.width / rect.width)
    const py = (my - rect.top) * (vb.height / rect.height)
    return Math.atan2(px - cx, -(py - cy)) * (180 / Math.PI)
  }

  function rotatePointerDown(e) {
    e.stopPropagation()
    rotating = true
    rotateStartValue = rotation
    rotateStartAngle = sceneAngle(
      e.currentTarget.ownerSVGElement,
      e.clientX,
      e.clientY,
    )
    rotateOrigin = { mx: e.clientX, my: e.clientY }
    e.currentTarget.setPointerCapture(e.pointerId)
  }

  function rotatePointerMove(e) {
    if (!rotating) return
    const a = sceneAngle(e.currentTarget.ownerSVGElement, e.clientX, e.clientY)
    onrotate?.(rotateStartValue + (a - rotateStartAngle))
  }

  function rotatePointerUp(e) {
    if (!rotating) return
    rotating = false
    if (!pastClickThreshold(e, rotateOrigin)) return
    const a = sceneAngle(e.currentTarget.ownerSVGElement, e.clientX, e.clientY)
    onrotateend?.(rotateStartValue + (a - rotateStartAngle))
  }

  // ── Resize handles ────────────────────────────────────────────────────────
  let resizingCorner = $state(null)
  let resizeOrigin = { mx: 0, my: 0 }
  let resizeDelta = $state({ dx: 0, dy: 0 })

  function cornerPointerDown(e, corner) {
    e.stopPropagation()
    resizingCorner = corner
    resizeOrigin = { mx: e.clientX, my: e.clientY }
    resizeDelta = { dx: 0, dy: 0 }
    e.currentTarget.setPointerCapture(e.pointerId)
  }

  function cornerPointerMove(e) {
    if (!resizingCorner) return
    const svg = e.currentTarget.ownerSVGElement
    if (!svg) return
    resizeDelta = screenToOverlayDelta(
      svg,
      resizeOrigin.mx,
      resizeOrigin.my,
      e.clientX,
      e.clientY,
    )
    onresize?.(resizingCorner, resizeDelta.dx, resizeDelta.dy, e.shiftKey)
  }

  function cornerPointerUp(e) {
    if (!resizingCorner) return
    if (pastClickThreshold(e, resizeOrigin))
      onresizeend?.(resizingCorner, resizeDelta.dx, resizeDelta.dy, e.shiftKey)
    resizingCorner = null
    resizeDelta = { dx: 0, dy: 0 }
  }

  function cornerPos(corner) {
    switch (corner) {
      case 'tl':
        return { x: d.x, y: d.y }
      case 'tr':
        return { x: d.x + d.w, y: d.y }
      case 'bl':
        return { x: d.x, y: d.y + d.h }
      case 'br':
        return { x: d.x + d.w, y: d.y + d.h }
    }
  }
</script>

<g transform="rotate({rotation} {cx} {cy})">
  <!-- Hit area (larger than visual for easier grabbing) -->
  <rect
    x={d.x - 4}
    y={d.y - 4}
    width={Math.max(d.w + 8, 24)}
    height={Math.max(d.h + 8, 24)}
    fill="transparent"
    style="cursor: {locked
      ? 'not-allowed'
      : dragging
        ? 'grabbing'
        : 'grab'}; pointer-events: all; outline: none"
    role="button"
    aria-label="{locked ? 'Locked' : 'Move'} {label}"
    tabindex="0"
    {onpointerdown}
    {onpointermove}
    {onpointerup}
  />

  <!-- Visual border -->
  <rect
    x={d.x}
    y={d.y}
    width={Math.max(d.w, 4)}
    height={Math.max(d.h, 4)}
    fill="none"
    stroke-width={selected ? 1.5 : 1}
    stroke-dasharray={selected ? 'none' : '4 3'}
    rx="2"
    style="pointer-events: none; stroke: {selected
      ? locked
        ? '#F59E0B'
        : 'var(--primary)'
      : 'rgba(255,255,255,0.25)'}"
  />

  <!-- Label tag (only when selected) -->
  {#if selected}
    <rect
      x={d.x}
      y={d.y - 18}
      width={Math.max(label.length * 6.5 + 8, 30)}
      height={16}
      rx="3"
      style="pointer-events: none; fill: var(--primary)"
    />
    <text
      x={d.x + 4}
      y={d.y - 6}
      font-size="10"
      fill="white"
      font-family="system-ui"
      style="pointer-events: none; user-select: none">{label}</text
    >

    <!-- Corner handles -->
    {#each CORNERS as corner (corner)}
      {@const { x: hx, y: hy } = cornerPos(corner)}
      <!-- Visual dot -->
      <rect
        x={hx - 3}
        y={hy - 3}
        width={6}
        height={6}
        rx="1"
        style="pointer-events: none; fill: var(--primary)"
      />
      <!-- Resize hit area (resizable elements only) -->
      {#if resizable}
        <rect
          role="button"
          tabindex="0"
          aria-label={`Resize ${corner}`}
          x={hx - 12}
          y={hy - 12}
          width={24}
          height={24}
          fill="transparent"
          style="cursor: {RESIZE_CURSORS[corner]}; pointer-events: all"
          onpointerdown={(e) => cornerPointerDown(e, corner)}
          onpointermove={cornerPointerMove}
          onpointerup={cornerPointerUp}
        />
      {/if}
    {/each}

    <!-- Rotation stem -->
    <line
      x1={cx}
      y1={d.y}
      x2={cx}
      y2={d.y - ROTATE_HANDLE_OFFSET}
      stroke-width="1"
      style="pointer-events: none; stroke: var(--primary)"
    />

    <!-- Rotation handle: large transparent hit area + small visual dot -->
    <circle
      role="button"
      aria-label="Rotate element"
      tabindex="0"
      {cx}
      cy={d.y - ROTATE_HANDLE_OFFSET}
      r="14"
      fill="transparent"
      style="cursor: {ROTATE_CURSOR}; pointer-events: all"
      onpointerdown={rotatePointerDown}
      onpointermove={rotatePointerMove}
      onpointerup={rotatePointerUp}
    />
    <circle
      {cx}
      cy={d.y - ROTATE_HANDLE_OFFSET}
      r="6"
      stroke="white"
      stroke-width="1.5"
      style="pointer-events: none; fill: var(--primary)"
    />
  {/if}
</g>
