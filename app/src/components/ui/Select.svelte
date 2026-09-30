<script>
  import { cn } from '@/lib/utils.js'
  let {
    value = $bindable(''),
    options = [],
    placeholder = '',
    disabled = false,
    class: className = '',
    onchange,
  } = $props()
  // options: array of {value, label, group?} or strings
  // Ungrouped options render first; the rest render inside <optgroup> elements.

  const optVal = (o) => (typeof o === 'string' ? o : o.value)
  const optLabel = (o) => (typeof o === 'string' ? o : o.label)
  const optGroup = (o) => (typeof o === 'string' ? null : (o.group ?? null))

  let ungrouped = $derived(options.filter((o) => !optGroup(o)))
  let groups = $derived.by(() => {
    const names = []
    for (const o of options) {
      const g = optGroup(o)
      if (g && !names.includes(g)) names.push(g)
    }
    return names.map((name) => ({
      name,
      opts: options.filter((o) => optGroup(o) === name),
    }))
  })
</script>

<select
  bind:value
  {disabled}
  onchange={(e) => onchange?.(e.target.value)}
  class={cn(
    'h-7 w-full rounded-[6px] border-0 bg-[var(--panel2)] px-2.5 text-sm text-foreground',
    'focus:outline-none focus:ring-1 focus:ring-ring cursor-pointer',
    'disabled:opacity-50 disabled:cursor-not-allowed',
    className,
  )}
>
  {#if placeholder}
    <option value="" disabled>{placeholder}</option>
  {/if}
  {#each ungrouped as opt (optVal(opt))}
    <option value={optVal(opt)}>{optLabel(opt)}</option>
  {/each}
  {#each groups as group (group.name)}
    <optgroup label={group.name}>
      {#each group.opts as opt (optVal(opt))}
        <option value={optVal(opt)}>{optLabel(opt)}</option>
      {/each}
    </optgroup>
  {/each}
</select>
