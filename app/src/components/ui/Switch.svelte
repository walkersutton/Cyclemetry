<script>
  import { cn } from '@/lib/utils.js'
  let {
    checked = $bindable(false),
    id,
    ariaLabel,
    disabled = false,
    onchange,
    class: className = '',
  } = $props()
</script>

<button
  role="switch"
  aria-checked={checked}
  aria-label={ariaLabel}
  {id}
  {disabled}
  onclick={() => {
    checked = !checked
    onchange?.(checked)
  }}
  class={cn(
    'relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-150',
    'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
    'disabled:opacity-50 disabled:cursor-not-allowed',
    checked ? 'bg-primary' : 'bg-zinc-700',
    className,
  )}
>
  <span
    class={cn(
      'pointer-events-none block h-4 w-4 rounded-full bg-white shadow transition-transform duration-150',
      checked ? 'translate-x-4' : 'translate-x-0',
    )}
  ></span>
</button>
