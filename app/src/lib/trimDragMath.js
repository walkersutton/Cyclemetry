export function clamp(v, lo, hi) {
  return Math.min(hi, Math.max(lo, v))
}

export function computeTrimDrag({
  handle,
  initial,
  startX,
  clientX,
  trackWidth,
  total,
  currentStart,
  currentEnd,
}) {
  const w = trackWidth || 1
  const t = Math.max(0.0001, total ?? 0)
  const dxSec = ((clientX - startX) / w) * t
  const next = Math.round(initial + dxSec)
  if (handle === 'start') {
    const maxStart = Math.max(0, (currentEnd ?? t) - 1)
    return { field: 'start', value: clamp(next, 0, maxStart) }
  }
  const minEnd = (currentStart ?? 0) + 1
  return { field: 'end', value: clamp(next, minEnd, Math.round(t)) }
}

export function nudgeTrim({
  handle,
  currentStart,
  currentEnd,
  total,
  deltaSec,
}) {
  const t = Math.max(0.0001, total ?? 0)
  if (handle === 'start') {
    const next = (currentStart ?? 0) + deltaSec
    const maxStart = Math.max(0, (currentEnd ?? t) - 1)
    return { field: 'start', value: clamp(next, 0, maxStart) }
  }
  const next = (currentEnd ?? t) + deltaSec
  const minEnd = (currentStart ?? 0) + 1
  return { field: 'end', value: clamp(next, minEnd, Math.round(t)) }
}

export function trimFromKey({ key, shiftKey, handle, start, end, total }) {
  const t = Math.max(0.0001, total ?? 0)
  const currentStart = start ?? 0
  const currentEnd = end ?? t

  if (key === 'Home') {
    return handle === 'start'
      ? { field: 'start', value: 0 }
      : { field: 'end', value: currentStart + 1 }
  }
  if (key === 'End') {
    return handle === 'start'
      ? { field: 'start', value: Math.max(0, currentEnd - 1) }
      : { field: 'end', value: Math.round(t) }
  }

  const delta = shiftKey ? 10 : 1
  let step = 0
  if (key === 'ArrowLeft' || key === 'ArrowDown') step = -delta
  else if (key === 'ArrowRight' || key === 'ArrowUp') step = delta
  else return null

  return nudgeTrim({
    handle,
    currentStart,
    currentEnd,
    total: t,
    deltaSec: step,
  })
}
