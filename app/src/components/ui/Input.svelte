<script>
  import { getContext } from 'svelte'
  import { cn } from '@/lib/utils.js'
  let {
    value = $bindable(''),
    type = 'text',
    placeholder = '',
    disabled = false,
    class: className = '',
    oninput,
    onchange,
    onblur,
    onfocus,
    onkeydown,
    ...rest
  } = $props()
  const app = getContext('app')
  const isNumber = $derived(type === 'number')

  function handleFocus(e) {
    if (isNumber) app?.beginEditBatch?.()
    onfocus?.(e)
  }

  function handleBlur(e) {
    onblur?.(e)
    if (isNumber) app?.endEditBatch?.()
  }

  function handleKeydown(e) {
    if (
      isNumber &&
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey &&
      (e.key === 'z' || e.key === 'Z')
    ) {
      e.preventDefault()
      app?.undo?.()
      e.currentTarget.blur()
      return
    }
    if (
      isNumber &&
      (e.metaKey || e.ctrlKey) &&
      ((e.shiftKey && (e.key === 'z' || e.key === 'Z')) ||
        e.key === 'y' ||
        e.key === 'Y')
    ) {
      e.preventDefault()
      app?.redo?.()
      e.currentTarget.blur()
      return
    }
    onkeydown?.(e)
  }
</script>

<input
  {type}
  bind:value
  {placeholder}
  {disabled}
  {oninput}
  {onchange}
  onfocus={handleFocus}
  onblur={handleBlur}
  onkeydown={handleKeydown}
  class={cn(
    'h-7 w-full rounded-[6px] border-0 bg-[var(--panel2)] px-2.5 text-sm text-foreground',
    'placeholder:text-zinc-600 focus:outline-none focus:ring-1 focus:ring-ring',
    'disabled:opacity-50 disabled:cursor-not-allowed',
    className,
  )}
  {...rest}
/>
