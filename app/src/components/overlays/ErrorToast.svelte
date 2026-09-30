<script>
  import { getContext } from 'svelte'
  const app = getContext('app')
</script>

{#if app.successMessage}
  <div
    class="fixed bottom-4 left-1/2 -translate-x-1/2 z-[200] flex items-center gap-3 rounded-lg border border-emerald-500/30 bg-zinc-900 px-4 py-3 shadow-xl max-w-sm w-full"
    role="status"
  >
    <svg
      class="h-4 w-4 text-emerald-500 shrink-0"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
    >
      <path
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="2"
        d="M5 13l4 4L19 7"
      />
    </svg>
    <p class="text-sm text-zinc-200 flex-1 select-text cursor-text">
      {app.successMessage}
    </p>
  </div>
{:else if app.errorMessage}
  <div
    class="fixed bottom-4 left-1/2 -translate-x-1/2 z-[200] flex items-start gap-3 rounded-lg border border-destructive/30 bg-zinc-900 px-4 py-3 shadow-xl max-w-sm w-full"
    role="alert"
  >
    <svg
      class="h-4 w-4 text-destructive shrink-0 mt-0.5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
    >
      <path
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="2"
        d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
      />
    </svg>
    <p class="text-sm text-zinc-200 flex-1 select-text cursor-text">
      {app.errorMessage}
    </p>
    <button
      onclick={() => app.clearError()}
      class="cursor-pointer text-zinc-500 hover:text-zinc-200 text-lg leading-none"
      aria-label="Dismiss">×</button
    >
  </div>
{/if}
