import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import {
  clamp,
  computeTrimDrag,
  nudgeTrim,
  trimFromKey,
} from './trimDragMath.js'

describe('clamp', () => {
  it('bounds values within [lo, hi]', () => {
    assert.equal(clamp(5, 0, 10), 5)
    assert.equal(clamp(-5, 0, 10), 0)
    assert.equal(clamp(15, 0, 10), 10)
  })
})

describe('computeTrimDrag', () => {
  it('computes forward drag for start handle clamped to currentEnd - 1', () => {
    const res = computeTrimDrag({
      handle: 'start',
      initial: 10,
      startX: 100,
      clientX: 200,
      trackWidth: 100,
      total: 100,
      currentStart: 10,
      currentEnd: 50,
    })
    // dxSec = ((200 - 100) / 100) * 100 = 100 -> next = 110, clamped to 49
    assert.deepEqual(res, { field: 'start', value: 49 })
  })

  it('computes backward drag for start handle clamped to 0', () => {
    const res = computeTrimDrag({
      handle: 'start',
      initial: 10,
      startX: 100,
      clientX: 50,
      trackWidth: 100,
      total: 100,
      currentStart: 10,
      currentEnd: 50,
    })
    assert.deepEqual(res, { field: 'start', value: 0 })
  })

  it('computes backward drag for end handle clamped to currentStart + 1', () => {
    const res = computeTrimDrag({
      handle: 'end',
      initial: 80,
      startX: 200,
      clientX: 50,
      trackWidth: 100,
      total: 100,
      currentStart: 30,
      currentEnd: 80,
    })
    assert.deepEqual(res, { field: 'end', value: 31 })
  })

  it('computes forward drag for end handle clamped to total', () => {
    const res = computeTrimDrag({
      handle: 'end',
      initial: 80,
      startX: 100,
      clientX: 200,
      trackWidth: 100,
      total: 100,
      currentStart: 30,
      currentEnd: 80,
    })
    assert.deepEqual(res, { field: 'end', value: 100 })
  })

  it('handles edge case total = 0 gracefully', () => {
    const res = computeTrimDrag({
      handle: 'start',
      initial: 0,
      startX: 100,
      clientX: 150,
      trackWidth: 100,
      total: 0,
      currentStart: 0,
      currentEnd: 0,
    })
    assert.deepEqual(res, { field: 'start', value: 0 })
  })

  it('handles currentEnd undefined fallback', () => {
    const res = computeTrimDrag({
      handle: 'start',
      initial: 10,
      startX: 100,
      clientX: 120,
      trackWidth: 100,
      total: 100,
      currentStart: 10,
      currentEnd: undefined,
    })
    assert.deepEqual(res, { field: 'start', value: 30 })
  })

  it('handles end - 1 < 0 safely', () => {
    const res = computeTrimDrag({
      handle: 'start',
      initial: 5,
      startX: 100,
      clientX: 110,
      trackWidth: 100,
      total: 100,
      currentStart: 0,
      currentEnd: 0,
    })
    assert.deepEqual(res, { field: 'start', value: 0 })
  })
})

describe('nudgeTrim', () => {
  it('nudges start handle forward and clamps', () => {
    const res = nudgeTrim({
      handle: 'start',
      currentStart: 10,
      currentEnd: 20,
      total: 100,
      deltaSec: 15,
    })
    assert.deepEqual(res, { field: 'start', value: 19 })
  })

  it('nudges start handle backward and clamps to 0', () => {
    const res = nudgeTrim({
      handle: 'start',
      currentStart: 5,
      currentEnd: 20,
      total: 100,
      deltaSec: -10,
    })
    assert.deepEqual(res, { field: 'start', value: 0 })
  })

  it('nudges end handle backward and clamps to currentStart + 1', () => {
    const res = nudgeTrim({
      handle: 'end',
      currentStart: 50,
      currentEnd: 60,
      total: 100,
      deltaSec: -20,
    })
    assert.deepEqual(res, { field: 'end', value: 51 })
  })

  it('nudges end handle forward and clamps to total', () => {
    const res = nudgeTrim({
      handle: 'end',
      currentStart: 50,
      currentEnd: 95,
      total: 100,
      deltaSec: 10,
    })
    assert.deepEqual(res, { field: 'end', value: 100 })
  })
})

describe('trimFromKey', () => {
  it('handles ArrowLeft / ArrowRight with and without shift', () => {
    const left = trimFromKey({
      key: 'ArrowLeft',
      shiftKey: false,
      handle: 'start',
      start: 20,
      end: 50,
      total: 100,
    })
    assert.deepEqual(left, { field: 'start', value: 19 })

    const rightShift = trimFromKey({
      key: 'ArrowRight',
      shiftKey: true,
      handle: 'start',
      start: 20,
      end: 50,
      total: 100,
    })
    assert.deepEqual(rightShift, { field: 'start', value: 30 })
  })

  it('handles Home key for start and end handles', () => {
    assert.deepEqual(
      trimFromKey({
        key: 'Home',
        shiftKey: false,
        handle: 'start',
        start: 20,
        end: 50,
        total: 100,
      }),
      { field: 'start', value: 0 },
    )
    assert.deepEqual(
      trimFromKey({
        key: 'Home',
        shiftKey: false,
        handle: 'end',
        start: 20,
        end: 50,
        total: 100,
      }),
      { field: 'end', value: 21 },
    )
  })

  it('handles End key for start and end handles', () => {
    assert.deepEqual(
      trimFromKey({
        key: 'End',
        shiftKey: false,
        handle: 'start',
        start: 20,
        end: 50,
        total: 100,
      }),
      { field: 'start', value: 49 },
    )
    assert.deepEqual(
      trimFromKey({
        key: 'End',
        shiftKey: false,
        handle: 'end',
        start: 20,
        end: 50,
        total: 100,
      }),
      { field: 'end', value: 100 },
    )
  })

  it('returns null for unhandled keys', () => {
    assert.equal(
      trimFromKey({
        key: 'Tab',
        shiftKey: false,
        handle: 'start',
        start: 20,
        end: 50,
        total: 100,
      }),
      null,
    )
    assert.equal(
      trimFromKey({
        key: 'Enter',
        shiftKey: false,
        handle: 'start',
        start: 20,
        end: 50,
        total: 100,
      }),
      null,
    )
  })
})
