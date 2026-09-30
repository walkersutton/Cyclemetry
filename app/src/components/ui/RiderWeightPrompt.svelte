<script>
  // W/kg is the one metric that needs a number the activity file can't supply.
  // With no rider weight every W/kg readout renders 0.0, which reads as a bug —
  // so wherever that's about to happen, ask for the weight right there instead
  // of sending the user off to Settings.
  //
  // Mount this anywhere W/kg is in play (it decides for itself whether there's
  // anything to say); the parent only supplies the contextual gate.
  import { getContext, onDestroy } from 'svelte'
  import { quintOut } from 'svelte/easing'
  import { fade, slide } from 'svelte/transition'
  import { Check, TriangleAlert } from 'lucide-svelte'
  import RiderWeightField from './RiderWeightField.svelte'

  const app = getContext('app')

  // Respect the OS "reduce motion" setting: keep the fade that explains the
  // box is leaving, drop the movement that can trigger motion sickness.
  const reduceMotion =
    typeof window !== 'undefined' &&
    window.matchMedia?.('(prefers-reduced-motion: reduce)').matches

  // Floating dismiss: fade with a clear lift + shrink so it reads as the box
  // sliding away rather than a flat crossfade. quintOut on both directions — an
  // exiting element still eases out, never in. This is an auto-dismiss (the
  // user isn't the one closing it, so their eye isn't already on it): it needs
  // more travel and time than a user-initiated close to be caught at all.
  //
  // Driven with `tick` (JS sets the style each frame) rather than `css`: a css
  // transition makes Svelte emit a @keyframes rule, and WebKit (this app runs
  // in WKWebView) snaps a keyframed transform to its first frame before playing
  // — that snap was the abrupt jump. tick writes the exact eased value each
  // frame, so there's no keyframe to jump to.
  function dismiss(node, { duration = 260, lift = 10 } = {}) {
    return {
      duration,
      easing: quintOut,
      tick: (t) => {
        node.style.opacity = String(t)
        node.style.transform = reduceMotion
          ? ''
          : `translateY(${(1 - t) * -lift}px) scale(${0.96 + t * 0.04})`
      },
    }
  }

  let {
    // Sentence explaining what's wrong in this particular spot.
    message = 'W/kg needs your rider weight — without it this element renders as 0.',
    // Single-row layout for strips above the preview; the stacked block is for
    // panels and dialogs where there's vertical room.
    compact = false,
    // Set when the parent positions this over its content instead of in the
    // layout flow: nothing reflows when it goes, so it just fades out.
    floating = false,
    // Outer spacing — applied to the box so nothing is reserved when the prompt
    // has nothing to say.
    class: className = '',
  } = $props()

  // Hold the box just long enough to read the confirmation rather than
  // disappearing the instant the weight commits. In flow, the click that
  // commits it (blur, then Render / a canvas click) would otherwise land on
  // whatever slid up into its place — and a prompt that blinks out mid-entry is
  // what made people stop typing after one digit in the first place.
  const CONFIRM_MS = 1400
  // Kept separate so the displayed value survives the exit: `showConfirm` drives
  // visibility, `confirmedWeight` the text. Resetting one value to hide the box
  // would blank the label mid-fade.
  let showConfirm = $state(false)
  let confirmedWeight = $state(null)
  let confirmTimer = null

  function onset(weight) {
    confirmedWeight = weight
    showConfirm = true
    clearTimeout(confirmTimer)
    confirmTimer = setTimeout(() => (showConfirm = false), CONFIRM_MS)
  }

  onDestroy(() => clearTimeout(confirmTimer))

  let needed = $derived(app.needsRiderWeight)
  let showing = $derived(needed || showConfirm)
  let headline = $derived(
    needed
      ? message
      : `Rider weight set — ${confirmedWeight} ${app.riderWeightUnit}.`,
  )
</script>

{#snippet box()}
  <div
    class="{compact
      ? 'flex items-center gap-2.5 rounded-[6px] px-3 py-1.5'
      : 'space-y-2 rounded-[6px] px-3 py-2.5'} border {needed
      ? 'border-[var(--ds-warning)]/30 bg-[var(--ds-warning)]/10'
      : 'border-[var(--ds-success)]/30 bg-[var(--ds-success)]/10'}"
  >
    <div
      class="flex {compact
        ? 'min-w-0 flex-1 items-center'
        : 'items-start'} gap-2.5"
    >
      {#if needed}
        <TriangleAlert
          size={14}
          class="{compact ? '' : 'mt-0.5'} shrink-0 text-[var(--ds-warning)]"
        />
      {:else}
        <Check
          size={14}
          class="{compact ? '' : 'mt-0.5'} shrink-0 text-[var(--ds-success)]"
        />
      {/if}
      <p class="{compact ? 'truncate ' : ''}text-xs leading-snug text-zinc-300">
        {headline}
      </p>
    </div>
    <div class={compact ? 'shrink-0' : ''}>
      <RiderWeightField {onset} />
    </div>
    {#if !compact && needed}
      <p class="text-[10px] leading-snug text-zinc-500">
        Stored on this device only — never saved into templates.
      </p>
    {/if}
  </div>
{/snippet}

{#if showing}
  {#if floating}
    <!-- Out of the layout flow: nothing below it moves, so the box just puts
         itself away — lift + shrink + fade, ease-out both ways, exit quicker
         than entry. An opaque base under the tinted box keeps it readable over
         whatever it's floating on.
         `|global`: the block that toggles is the parent `{#if showing}`, not
         this `{#if floating}` — without it a local transition never fires. -->
    <div
      class="{className} overflow-hidden rounded-[6px] bg-[var(--panel)] shadow-lg"
      style="transform-origin: top center"
      in:dismiss|global={{ duration: 200, lift: 6 }}
      out:dismiss|global={{ duration: 280, lift: 10 }}
    >
      {@render box()}
    </div>
  {:else}
    <!-- In flow (panels, dialogs): collapse the height rather than popping out
         of existence, so what sits below settles instead of snapping upward.
         The box fades a little sooner so the text is gone before it clips.
         Entering and exiting both ease out; the exit is the quicker of the two. -->
    <div
      class={className}
      in:slide|global={{ duration: 200, easing: quintOut }}
      out:slide|global={{ duration: 160, easing: quintOut }}
    >
      <div
        in:fade|global={{ duration: 150, easing: quintOut }}
        out:fade|global={{ duration: 110, easing: quintOut }}
      >
        {@render box()}
      </div>
    </div>
  {/if}
{/if}
