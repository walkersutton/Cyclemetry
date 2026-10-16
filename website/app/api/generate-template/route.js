import { NextResponse } from 'next/server'

// ── OpenRouter proxy ──────────────────────────────────────────────────────────
// The desktop app never sees our API key: it POSTs a prompt here, this route
// adds the server-side key and forwards the request to OpenRouter, then returns
// the generated template JSON. Only free models are used for now.

const OPENROUTER_URL = 'https://openrouter.ai/api/v1/chat/completions'
// Free model (note the `:free` suffix). Override with OPENROUTER_MODEL.
const DEFAULT_MODEL = 'deepseek/deepseek-chat-v3-0324:free'

const RATE_LIMIT = 10
const RATE_WINDOW_MS = 60 * 60 * 1000 // 1 hour

const MAX_PROMPT_LENGTH = 2000

// ip -> array of timestamps
const ipLog = new Map()

function checkRateLimit(ip) {
  const now = Date.now()
  const cutoff = now - RATE_WINDOW_MS
  const hits = (ipLog.get(ip) ?? []).filter((t) => t > cutoff)
  if (hits.length >= RATE_LIMIT) return false
  ipLog.set(ip, [...hits, now])
  return true
}

// The Cyclemetry template schema, distilled for the model. Kept tight so weaker
// free models stay on-format. See src-tauri/src/render/template.rs for the
// authoritative definition.
const SYSTEM_PROMPT = `You design overlay templates for Cyclemetry, a cycling telemetry video overlay tool.

Output ONLY a single JSON object — no prose, no markdown fences. Shape:
{ "scene": { ... }, "elements": [ ... ] }

Templates are authored on a 3840x2160 (4K) canvas. All x/y/width/height are pixels at that resolution. The overlay is drawn on top of cycling footage, so use a transparent/dark aesthetic and keep elements near the edges, not centered over the rider.

scene fields:
- width: 3840, height: 2160 (always use these)
- fps: 30
- font: a font name string (e.g. "Arial.ttf"); optional
- font_size: default text size in px (e.g. 64)
- color: default element color as hex (e.g. "#ffffff")
- opacity: 0..1

Each element needs a unique "id" (e.g. "value-0"), a "type", and "x"/"y". Element types:
- label: static text. fields: text, font_size, color, opacity, italic, text_align ("left"|"center"|"right"), letter_spacing
- value: live metric readout. fields: value (metric name), unit, suffix, font_size, color, opacity, text_align, decimal_rounding
- plot: line graph. fields: value ("course" for the map route, or a metric), width, height, color, opacity, line {width,color}, fill {color,opacity}, point {color,weight}
- meter: proportional bar. fields: value (metric), width, height, min, max, color, gradient (array of hex), background, direction ("up"|"down"|"left"|"right"), radius
- gauge: circular dial. fields: value (metric), width, height, min, max, arc_color, progress_color, gradient, needle_color, start_angle, sweep_angle
- rect: rectangle. fields: width, height, color, fill_opacity, opacity, radius, border_color, border_width
- image: static asset. fields: file (filename), width, height, opacity

Metric names usable in "value": "speed", "power", "heartrate", "cadence", "distance", "elevation", "gradient", "temperature", "time", "calories", "total_calories", and "course" (the GPS route, plot only).

Use scene.color/font_size as shared defaults; elements inherit them unless they override. Numbers must be numbers, not strings (except min/max may be "min"/"max").`

const EXAMPLE = `{
  "scene": { "width": 3840, "height": 2160, "fps": 30, "color": "#ffffff", "font_size": 72, "opacity": 1 },
  "elements": [
    { "type": "value", "id": "value-0", "value": "speed", "unit": "mph", "suffix": " MPH", "x": 200, "y": 1820, "font_size": 140, "decimal_rounding": 0 },
    { "type": "value", "id": "value-1", "value": "power", "suffix": " W", "x": 200, "y": 1980, "font_size": 100 },
    { "type": "value", "id": "value-2", "value": "heartrate", "suffix": " BPM", "x": 200, "y": 2080, "font_size": 100, "color": "#dc143c" },
    { "type": "plot", "id": "plot-0", "value": "course", "x": 3100, "y": 1750, "width": 600, "height": 320, "line": { "width": 6, "color": "#ffffff" }, "point": { "color": "#dc143c", "weight": 18 } }
  ]
}`

function extractJson(text) {
  if (!text) return null
  let s = text.trim()
  // Strip markdown code fences if the model added them.
  const fence = s.match(/```(?:json)?\s*([\s\S]*?)```/i)
  if (fence) s = fence[1].trim()
  // Fall back to the outermost { ... } span.
  const first = s.indexOf('{')
  const last = s.lastIndexOf('}')
  if (first === -1 || last === -1 || last < first) return null
  try {
    return JSON.parse(s.slice(first, last + 1))
  } catch {
    return null
  }
}

export async function POST(request) {
  const apiKey = process.env.OPENROUTER_API_KEY
  if (!apiKey) {
    return NextResponse.json(
      { error: 'Template generation is not configured on the server.' },
      { status: 503 },
    )
  }

  const ip =
    request.headers.get('x-real-ip') ??
    request.headers.get('x-forwarded-for')?.split(',')[0]?.trim() ??
    'unknown'
  if (!checkRateLimit(ip)) {
    return NextResponse.json(
      { error: 'Too many requests. Try again later.' },
      { status: 429 },
    )
  }

  let payload
  try {
    payload = await request.json()
  } catch {
    return NextResponse.json({ error: 'Invalid request body.' }, { status: 400 })
  }

  const prompt = typeof payload?.prompt === 'string' ? payload.prompt.trim() : ''
  const currentTemplate = payload?.currentTemplate ?? null

  if (!prompt) {
    return NextResponse.json({ error: 'A prompt is required.' }, { status: 400 })
  }
  if (prompt.length > MAX_PROMPT_LENGTH) {
    return NextResponse.json(
      { error: 'Prompt is too long.' },
      { status: 400 },
    )
  }
  if (currentTemplate && JSON.stringify(currentTemplate).length > 32000) {
    return NextResponse.json(
      { error: 'Current template is too large.' },
      { status: 400 },
    )
  }

  const userContent = currentTemplate
    ? `Modify the following Cyclemetry template according to this instruction, and return the COMPLETE updated template JSON (keep existing elements unless the instruction says otherwise).\n\nInstruction: ${prompt}\n\nCurrent template:\n${JSON.stringify(currentTemplate)}`
    : `Create a new Cyclemetry template for this request: ${prompt}\n\nHere is an example of a valid template for reference:\n${EXAMPLE}`

  const model = process.env.OPENROUTER_MODEL || DEFAULT_MODEL

  try {
    const orRes = await fetch(OPENROUTER_URL, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${apiKey}`,
        'Content-Type': 'application/json',
        'HTTP-Referer': 'https://cyclemetry.walkersutton.com',
        'X-Title': 'Cyclemetry',
      },
      body: JSON.stringify({
        model,
        messages: [
          { role: 'system', content: SYSTEM_PROMPT },
          { role: 'user', content: userContent },
        ],
        response_format: { type: 'json_object' },
        temperature: 0.4,
        max_tokens: 4000,
      }),
    })

    if (!orRes.ok) {
      const detail = await orRes.text().catch(() => '')
      console.error('[generate-template] OpenRouter error', orRes.status, detail)
      return NextResponse.json(
        { error: 'The template generator is unavailable right now. Try again shortly.' },
        { status: 502 },
      )
    }

    const data = await orRes.json()
    const content = data?.choices?.[0]?.message?.content
    const template = extractJson(content)

    if (
      !template ||
      typeof template !== 'object' ||
      typeof template.scene !== 'object' ||
      !Array.isArray(template.elements)
    ) {
      console.error('[generate-template] Unparseable model output', content)
      return NextResponse.json(
        { error: 'The generator returned an invalid template. Try rephrasing your prompt.' },
        { status: 422 },
      )
    }

    // Force the authored resolution regardless of what the model returned —
    // the app scales from 4K at render time.
    template.scene.width = 3840
    template.scene.height = 2160

    return NextResponse.json({ template })
  } catch (err) {
    console.error('[generate-template]', err)
    return NextResponse.json(
      { error: 'Something went wrong generating the template.' },
      { status: 500 },
    )
  }
}
