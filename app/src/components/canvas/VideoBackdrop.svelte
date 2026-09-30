<script>
  /**
   * Renders the reference video behind the overlay preview.
   *
   * Master clock depends on mode:
   *
   * - Paused / scrub: `selectedSecond` is master; sync effect seeks
   *   `video.currentTime` to match (tight 0.02s epsilon for precise scrubs).
   * - Playing & in range: the *video* is master — it plays natively at
   *   the file's framerate and emits `timeupdate` events that drive
   *   `selectedSecond`. CenterCanvas's RAF loop also detects this and
   *   stops advancing `selectedSecond` on its own. The sync effect uses
   *   a wide 0.5s epsilon during play, so normal frame-cadence drift
   *   doesn't seek (each seek interrupts decode = stutter) — only a
   *   genuine user scrub mid-play snaps the video.
   *
   * Hidden whenever the playhead falls outside the video's extent, so
   * the checkered background re-emerges.
   */
  import { getContext } from 'svelte'
  import * as backend from '@/api/backend.js'
  import { videoStartOnAxis as computeStartOnAxis } from '@/lib/videoAlignment.js'

  let { playing = false } = $props()

  const app = getContext('app')

  let videoEl = $state(null)
  let videoError = $state(null)

  let video = $derived(app.video)

  // Resolve to a local HTTP URL served by Rust so WebKitGTK (Linux) gets
  // proper range-request support for seeking. A generation counter prevents
  // stale async results from overwriting a newer path.
  let src = $state(null)
  let _srcGen = 0
  $effect(() => {
    const path = video?.path
    const missing = video?.missing
    if (!path || missing) {
      src = null
      videoError = null
      return
    }
    const gen = ++_srcGen
    videoError = null
    backend
      .videoSrcUrl(path)
      .then((url) => {
        if (gen === _srcGen) src = url
      })
      .catch((err) => {
        if (gen === _srcGen) videoError = err?.message ?? 'Could not load video'
      })
  })
  let startOnAxis = $derived(computeStartOnAxis(app.gpxStartTime, video))
  let endOnAxis = $derived(startOnAxis + (video?.duration ?? 0))
  let inRange = $derived(
    !!video &&
      !video.missing &&
      (video.duration ?? 0) > 0 &&
      app.selectedSecond >= startOnAxis &&
      app.selectedSecond <= endOnAxis,
  )

  // Drift-correction sync. Tight while paused (every scrub registers
  // visually); LOOSE while playing so the natural lag between vsync-driven
  // selectedSecond reads and the video's own frame cadence doesn't seek
  // every tick — that constant seeking was the source of the playback
  // stutter. Big jumps (> 0.5s) still seek, which is what we want for the
  // "scrub while playing" case.
  $effect(() => {
    if (!videoEl || !inRange || !video) return
    const target = app.selectedSecond - startOnAxis
    const clamped = Math.max(0, Math.min(target, video.duration))
    const epsilon = playing ? 0.5 : 0.02
    if (Math.abs(videoEl.currentTime - clamped) > epsilon) {
      videoEl.currentTime = clamped
    }
  })

  // Play / pause the video element to match the timeline's playback state.
  // Out-of-range playback pauses the video (the checkered area is showing,
  // so playing under it is wasted decode); when the playhead re-enters
  // range, this effect re-fires and resumes play.
  $effect(() => {
    if (!videoEl) return
    if (playing && inRange) {
      // play() may reject on hostile autoplay policies; the user gesture
      // that toggled `playing` should satisfy WebKit, so swallow rather
      // than surface noise the user can't act on.
      videoEl.play().catch(() => {})
    } else {
      videoEl.pause()
    }
  })

  function onVideoError(e) {
    const code = e?.target?.error?.code
    videoError =
      code === 4
        ? 'Video codec not supported. On Linux, install GStreamer plugins:\nsudo apt install gstreamer1.0-libav gstreamer1.0-plugins-bad'
        : 'Could not load video — the file may be corrupt or use an unsupported format.'
  }

  // Video drives selectedSecond during playback. Skip when not the master
  // clock to avoid feedback loops (sync effect would then re-seek, etc.).
  function onTimeupdate() {
    if (!playing || !inRange || !videoEl) return
    const next = startOnAxis + videoEl.currentTime
    // Only push back when meaningfully different from what's already there
    // — guards against ricochet between this update and the sync effect.
    if (Math.abs(app.selectedSecond - next) > 0.01) {
      app.selectedSecond = next
    }
  }
</script>

{#if src}
  <video
    bind:this={videoEl}
    {src}
    playsinline
    preload="auto"
    ontimeupdate={onTimeupdate}
    onerror={onVideoError}
    class="absolute inset-0 w-full h-full object-contain pointer-events-none rounded-lg"
    style:visibility={inRange ? 'visible' : 'hidden'}
  ></video>
{/if}
{#if videoError}
  <div
    class="absolute bottom-2 left-2 right-2 rounded bg-zinc-900/95 px-3 py-2 text-[11px] text-red-400 leading-snug whitespace-pre-line pointer-events-none"
  >
    {videoError}
  </div>
{/if}
