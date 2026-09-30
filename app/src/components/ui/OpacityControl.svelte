<script>
  import { getContext } from 'svelte'
  import { cn } from '@/lib/utils.js'
  import Input from './Input.svelte'

  let {
    value = 1,
    min = 0,
    max = 1,
    step = 0.01,
    class: className = '',
    oninput,
  } = $props()

  const app = getContext('app')
  // Writable $derived: tracks the `value` prop but can be reassigned locally
  // for in-flight slider/number-input edits before `oninput` is committed.
  let currentValue = $derived(value ?? 1)
  let inputDraft = $state(String(value ?? 1))
  let editingInput = $state(false)

  // Filled-track percentage. Drives a CSS variable on the input so the
  // custom track gradient knows where to switch from crimson → zinc, giving
  // pixel-perfect color control instead of relying on the browser's
  // accent-color (which Chromium desaturates).
  let pct = $derived.by(() => {
    const span = max - min
    if (!(span > 0)) return 0
    return Math.max(0, Math.min(100, ((currentValue - min) / span) * 100))
  })

  function emit(raw) {
    const nextValue = Math.max(min, Math.min(max, parseFloat(raw) || 0))
    currentValue = nextValue
    if (!editingInput) inputDraft = String(nextValue)
    oninput?.({ target: { value: nextValue } })
  }

  function onTextInput(raw) {
    const sanitized = raw.replace(/[^\d.]/g, '').replace(/(\..*)\./g, '$1')
    inputDraft = sanitized
    if (sanitized === '' || sanitized === '.') return
    const parsed = parseFloat(sanitized)
    if (!Number.isFinite(parsed)) return
    emit(sanitized)
  }

  function onTextKeydown(e) {
    if (e.metaKey || e.ctrlKey || e.altKey) return
    if (
      e.key === 'Backspace' ||
      e.key === 'Delete' ||
      e.key === 'Tab' ||
      e.key === 'Enter' ||
      e.key === 'Escape' ||
      e.key.startsWith('Arrow') ||
      e.key === 'Home' ||
      e.key === 'End'
    )
      return
    if (/^\d$/.test(e.key) || e.key === '.') return
    e.preventDefault()
  }

  function beginRangeEdit() {
    app?.beginEditBatch?.()
  }

  function endRangeEdit() {
    app?.endEditBatch?.()
  }

  function beginTextEdit() {
    editingInput = true
    app?.beginEditBatch?.()
  }

  function endTextEdit() {
    editingInput = false
    inputDraft = String(currentValue)
    app?.endEditBatch?.()
  }

  $effect(() => {
    if (!editingInput) inputDraft = String(currentValue)
  })
</script>

<div class={cn('flex items-center gap-2', className)}>
  <input
    type="range"
    bind:value={currentValue}
    {min}
    {max}
    {step}
    style={`--pct: ${pct}%`}
    onpointerdown={beginRangeEdit}
    onpointerup={endRangeEdit}
    onpointercancel={endRangeEdit}
    onkeydown={(e) => {
      if (e.key.startsWith('Arrow') || e.key === 'Home' || e.key === 'End')
        beginRangeEdit()
    }}
    onkeyup={endRangeEdit}
    oninput={(e) => emit(e.target.value)}
    class="cm-slider cm-slider--filled h-7 min-w-0 flex-1"
  />
  <Input
    type="text"
    inputmode="decimal"
    value={inputDraft}
    onfocus={beginTextEdit}
    onblur={endTextEdit}
    onkeydown={onTextKeydown}
    oninput={(e) => onTextInput(e.target.value)}
    class="w-16 px-2 text-right tabular-nums"
  />
</div>
