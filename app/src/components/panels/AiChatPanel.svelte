<script>
  import { getContext, tick } from 'svelte'
  import { Send, Sparkles } from 'lucide-svelte'

  const app = getContext('app')

  let messages = $state([])
  let prompt = $state('')
  let mode = $state('edit')
  let loading = $state(false)
  let inputEl = $state(null)
  let messagesEl = $state(null)

  const examples = [
    'Minimal speed and power in the bottom-left',
    'Route map with heart rate and elevation plot',
    'Big crimson power meter on the right side',
  ]

  $effect(() => {
    if (!app.config && mode === 'edit') mode = 'create'
  })

  async function send() {
    const trimmed = prompt.trim()
    if (!trimmed || loading) return
    const isEdit = mode === 'edit' && !!app.config
    messages = [...messages, { role: 'user', text: trimmed }]
    prompt = ''
    loading = true
    await tick()
    scrollToBottom()
    try {
      await app.generateTemplate(trimmed, { edit: isEdit })
      messages = [
        ...messages,
        {
          role: 'assistant',
          text: isEdit ? 'Template updated.' : 'New template created.',
        },
      ]
    } catch (e) {
      messages = [...messages, { role: 'error', text: e?.message ?? String(e) }]
    } finally {
      loading = false
      await tick()
      scrollToBottom()
      inputEl?.focus()
    }
  }

  function scrollToBottom() {
    if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight
  }

  function onKeydown(e) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault()
      send()
    }
  }
</script>

<div class="flex flex-col h-full min-h-0">
  <!-- Header -->
  <div
    class="px-4 py-3 border-b border-white/[0.06] flex items-center gap-2 shrink-0"
  >
    <Sparkles size={12} class="text-[#dc143c] shrink-0" />
    <span
      class="text-[10px] font-semibold uppercase tracking-wider text-zinc-400 flex-1"
    >
      AI Assistant
    </span>
  </div>

  <!-- Mode toggle -->
  {#if app.config}
    <div class="px-4 pt-3 pb-2 shrink-0">
      <div
        class="flex gap-0.5 rounded-[6px] border border-zinc-700 bg-zinc-800 p-0.5"
      >
        <button
          type="button"
          onclick={() => (mode = 'edit')}
          disabled={loading}
          class="flex-1 cursor-pointer rounded-[4px] px-2 py-1 text-[11px] transition-colors
                 {mode === 'edit'
            ? 'bg-zinc-700 text-zinc-100'
            : 'text-zinc-400 hover:text-zinc-200'} disabled:opacity-50"
        >
          Edit current
        </button>
        <button
          type="button"
          onclick={() => (mode = 'create')}
          disabled={loading}
          class="flex-1 cursor-pointer rounded-[4px] px-2 py-1 text-[11px] transition-colors
                 {mode === 'create'
            ? 'bg-zinc-700 text-zinc-100'
            : 'text-zinc-400 hover:text-zinc-200'} disabled:opacity-50"
        >
          New template
        </button>
      </div>
    </div>
  {/if}

  <!-- Messages -->
  <div
    bind:this={messagesEl}
    class="flex-1 overflow-y-auto px-4 py-3 space-y-3 min-h-0"
  >
    {#if messages.length === 0}
      <p class="text-[11px] text-zinc-600 mb-2">
        Try describing what you want:
      </p>
      {#each examples as ex (ex)}
        <button
          type="button"
          onclick={() => {
            prompt = ex
            inputEl?.focus()
          }}
          disabled={loading}
          class="cursor-pointer w-full text-left rounded-[6px] border-0 bg-[var(--panel2)] px-3 py-2
                 text-[11px] text-zinc-500 hover:border-zinc-600 hover:text-zinc-300
                 transition-colors disabled:opacity-50"
        >
          {ex}
        </button>
      {/each}
    {:else}
      {#each messages as msg, i (i)}
        {#if msg.role === 'user'}
          <div class="flex justify-end">
            <div
              class="max-w-[90%] rounded-[6px] bg-[#dc143c]/10 border border-[#dc143c]/20 px-3 py-2"
            >
              <p
                class="text-[12px] text-zinc-200 leading-relaxed whitespace-pre-wrap"
              >
                {msg.text}
              </p>
            </div>
          </div>
        {:else if msg.role === 'assistant'}
          <div class="flex justify-start">
            <div
              class="max-w-[90%] rounded-[6px] bg-zinc-800/60 border border-zinc-700 px-3 py-2 flex items-center gap-2"
            >
              <Sparkles size={10} class="text-[#dc143c] shrink-0" />
              <p class="text-[12px] text-zinc-400 leading-relaxed">
                {msg.text}
              </p>
            </div>
          </div>
        {:else}
          <div class="flex justify-start">
            <div
              class="max-w-[90%] rounded-[6px] bg-red-950/30 border border-red-800/30 px-3 py-2"
            >
              <p class="text-[12px] text-red-400 leading-relaxed">{msg.text}</p>
            </div>
          </div>
        {/if}
      {/each}
      {#if loading}
        <div class="flex justify-start">
          <div
            class="rounded-[6px] bg-zinc-800/60 border border-zinc-700 px-3 py-2.5 flex items-center gap-1"
          >
            <span
              class="h-1.5 w-1.5 rounded-full bg-zinc-500 animate-bounce [animation-delay:-0.3s]"
            ></span>
            <span
              class="h-1.5 w-1.5 rounded-full bg-zinc-500 animate-bounce [animation-delay:-0.15s]"
            ></span>
            <span class="h-1.5 w-1.5 rounded-full bg-zinc-500 animate-bounce"
            ></span>
          </div>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Input -->
  <div class="px-4 pb-4 pt-2 border-t border-white/[0.06] shrink-0">
    <div class="relative">
      <textarea
        bind:this={inputEl}
        bind:value={prompt}
        rows="3"
        disabled={loading}
        placeholder={mode === 'edit'
          ? 'e.g. make the speed bigger, add cadence…'
          : 'e.g. speed and power in bottom-left corner…'}
        onkeydown={onKeydown}
        class="w-full resize-none rounded-[6px] border border-zinc-700 bg-zinc-800 px-3 py-2 pr-10
               text-[12px] text-zinc-100 placeholder-zinc-600 outline-none
               focus:border-zinc-500 focus:ring-1 focus:ring-zinc-500 disabled:opacity-60"
      ></textarea>
      <button
        type="button"
        onclick={send}
        disabled={!prompt.trim() || loading}
        class="absolute bottom-2 right-2 cursor-pointer h-6 w-6 flex items-center justify-center
               rounded-[4px] bg-[#dc143c] text-white transition-colors
               hover:bg-[#c01236] disabled:opacity-40 disabled:cursor-not-allowed"
        aria-label="Send"
      >
        <Send size={11} />
      </button>
    </div>
    <p class="mt-1.5 text-[10px] text-zinc-600">⌘↵ to send</p>
  </div>
</div>
