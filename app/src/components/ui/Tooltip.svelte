<script>
  import { cn } from '@/lib/utils.js'
  let {
    content = '',
    side = 'top',
    align = 'center',
    delay = 500,
    children,
    class: className = '',
  } = $props()

  let visible = $state(false)
  let timer = null

  function show() {
    if (timer) clearTimeout(timer)
    if (delay > 0)
      timer = setTimeout(() => {
        visible = true
      }, delay)
    else visible = true
  }
  function hide() {
    if (timer) {
      clearTimeout(timer)
      timer = null
    }
    visible = false
  }
</script>

<span
  role="group"
  class={cn('relative inline-flex', className)}
  onmouseenter={show}
  onmouseleave={hide}
  onfocus={show}
  onblur={hide}
>
  {@render children?.()}
  {#if content && visible}
    <span
      role="tooltip"
      class={cn(
        'absolute z-50 w-max max-w-[220px] rounded-[6px] border border-white/[0.08] bg-[#242424] px-2.5 py-1.5 text-xs text-foreground shadow-lg whitespace-pre-line',
        'pointer-events-none',
        side === 'top' && 'bottom-full mb-1.5',
        side === 'bottom' && 'top-full mt-1.5',
        side === 'left' && 'right-full top-1/2 -translate-y-1/2 mr-1.5',
        side === 'right' && 'left-full top-1/2 -translate-y-1/2 ml-1.5',
        (side === 'top' || side === 'bottom') &&
          align === 'center' &&
          'left-1/2 -translate-x-1/2',
        (side === 'top' || side === 'bottom') && align === 'start' && 'left-0',
        (side === 'top' || side === 'bottom') && align === 'end' && 'right-0',
      )}>{content}</span
    >
  {/if}
</span>
