<script>
  // Rider weight input + unit picker, with the typing rules the W/kg prompts
  // depend on: the app-level weight is only written on Enter or blur, never
  // per-keystroke. Committing every keystroke would turn the "7" of "75" into a
  // real 7 kg weight — silently wrong numbers, and the prompt asking for the
  // weight would vanish before the second digit was typed.
  import { getContext } from 'svelte'
  import Select from './Select.svelte'

  const app = getContext('app')

  let {
    // Called with the committed weight (in the current unit) after a valid entry.
    onset = null,
  } = $props()

  // Plausible rider range in kg. A partially typed number almost always lands
  // outside it, so it doubles as the guard against committing half an entry.
  const MIN_KG = 30
  const MAX_KG = 200
  const LB_PER_KG = 1 / 0.45359237

  let draft = $state(app.riderWeight == null ? '' : String(app.riderWeight))
  let editing = $state(false)
  let error = $state(null)

  // Follow the weight when it changes elsewhere (Settings, another prompt),
  // but never overwrite what the user is in the middle of typing.
  $effect(() => {
    const weight = app.riderWeight
    if (!editing) draft = weight == null ? '' : String(weight)
  })

  let bounds = $derived(
    app.riderWeightUnit === 'lb'
      ? [Math.round(MIN_KG * LB_PER_KG), Math.round(MAX_KG * LB_PER_KG)]
      : [MIN_KG, MAX_KG],
  )

  function commit() {
    const raw = draft.trim()
    if (raw === '') {
      // Cleared on purpose — drop the weight and go back to prompting.
      error = null
      app.riderWeight = null
      return
    }
    const entered = Number(raw)
    const kg = app.riderWeightUnit === 'lb' ? entered * 0.45359237 : entered
    if (!Number.isFinite(entered) || kg < MIN_KG || kg > MAX_KG) {
      error = `Enter a weight between ${bounds[0]} and ${bounds[1]} ${app.riderWeightUnit}.`
      return
    }
    error = null
    app.riderWeight = entered
    onset?.(entered)
  }

  function onKeydown(e) {
    if (e.key === 'Enter') {
      // Don't let Enter reach a dialog that treats it as "confirm".
      e.preventDefault()
      e.stopPropagation()
      commit()
    }
  }

  // Switching units re-reads the same number under the new unit, so re-check it.
  function changeUnit(unit) {
    app.riderWeightUnit = unit
    if (draft.trim() !== '') commit()
  }
</script>

<div class="space-y-1">
  <div class="flex items-center gap-1.5">
    <input
      type="number"
      min="0"
      step="0.1"
      inputmode="decimal"
      placeholder="—"
      aria-label="Rider weight"
      aria-invalid={error ? 'true' : undefined}
      value={draft}
      oninput={(e) => {
        draft = e.target.value
        error = null
      }}
      onfocus={() => (editing = true)}
      onblur={() => {
        editing = false
        commit()
      }}
      onkeydown={onKeydown}
      class="w-20 h-7 rounded-[6px] border-0 bg-[var(--panel2)] px-2 text-xs
             text-foreground font-mono focus:outline-none focus:ring-1
             {error
        ? 'ring-1 ring-destructive focus:ring-destructive'
        : 'focus:ring-ring'}"
    />
    <div class="shrink-0 w-20">
      <Select
        value={app.riderWeightUnit}
        options={[
          { value: 'kg', label: 'kg' },
          { value: 'lb', label: 'lb' },
        ]}
        onchange={changeUnit}
      />
    </div>
  </div>
  {#if error}
    <p class="text-[10px] leading-snug text-destructive">{error}</p>
  {/if}
</div>
