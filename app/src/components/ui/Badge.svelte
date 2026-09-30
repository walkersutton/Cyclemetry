<script>
  import { cn } from '@/lib/utils.js'
  let { variant = 'default', class: className = '', children } = $props()
  const variants = {
    default: 'bg-primary/15 text-primary border border-primary/30',
    secondary: 'bg-[var(--panel3)] text-zinc-300 border border-transparent',
    destructive:
      'bg-destructive/15 text-destructive border border-destructive/30',
    success: 'bg-green-500/15 text-green-400 border border-green-500/30',
    outline: 'border border-zinc-700 text-muted-foreground',
  }
</script>

<span
  class={cn(
    'inline-flex items-center gap-1 rounded-[4px] px-2 py-0.5 text-[11px] font-medium',
    variants[variant] ?? variants.secondary,
    className,
  )}
>
  {@render children?.()}
</span>
