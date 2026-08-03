<script>
  /**
   * Right panel: property editor for the currently selected element.
   * All changes write directly into app.config via app.updateElement().
   */
  import { getContext } from 'svelte'
  import { SvelteMap } from 'svelte/reactivity'
  import { AlertTriangle, AlignHorizontalJustifyCenter, AlignVerticalJustifyCenter, BarChart2, Folder, FolderOpen, Gauge as GaugeIcon, Hash, Image as ImageIcon, Lock, LockOpen, Map as MapIcon, Square, Thermometer, Type, Ungroup } from 'lucide-svelte'
  import Input from '../ui/Input.svelte'
  import OpacityControl from '../ui/OpacityControl.svelte'
  import Select from '../ui/Select.svelte'
  import Switch from '../ui/Switch.svelte'
  import Tooltip from '../ui/Tooltip.svelte'
  import ColorInput from '../ui/ColorInput.svelte'
  import AssetPicker from '../overlays/AssetPicker.svelte'
  import * as backend from '../../api/backend.js'
  import { defaultColorBands, elementMeta, elementTypeName, LAP_METRICS as ALL_LAP_METRICS, isLapMetric } from '../../lib/elementTypes.js'
  import { formatTime } from '../../lib/utils.js'
  import { metricRangeIssues, metricValueIssue } from '../../lib/metricLimits.js'
  import { normalizeElementField, normalizeTemplateIntegerField } from '../../lib/templateSchema.js'

  const app = getContext('app')

  // Single source of truth: app.fonts (bundled, user-installed, and system).
  // Importing new fonts lives in the Templates menu (Add Custom Font…).
  const fontGroup = (font) => (font.source === 'system' ? 'System fonts' : 'Font files')

  function fontOpts(includeSceneDefault) {
    return [
      ...(includeSceneDefault ? [{ value: '', label: 'Scene default' }] : []),
      ...app.fonts.map((font) => ({ value: font.value, label: font.label, group: fontGroup(font) })),
    ]
  }
  const ALL_METRICS = ['speed', 'heartrate', 'power', 'power_to_weight', 'elevation', 'cadence', 'gradient', 'lean', 'temperature', 'gear', 'front_gear', 'rear_gear', 'time', 'distance']

  // Summary (aggregate) metrics: a single constant value over the whole ride
  // or the overlay window. Each maps to the base telemetry attribute it needs,
  // used both for availability (base must be present) and unit resolution.
  const SUMMARY_BASE = {
    total_distance: 'distance',
    total_time: 'time',
    elevation_gain: 'elevation',
    elevation_loss: 'elevation',
    max_elevation: 'elevation',
    min_elevation: 'elevation',
    avg_speed: 'speed',
    max_speed: 'speed',
    avg_power: 'power',
    max_power: 'power',
    avg_heartrate: 'heartrate',
    max_heartrate: 'heartrate',
    avg_cadence: 'cadence',
  }
  const ALL_SUMMARY_METRICS = Object.keys(SUMMARY_BASE)
  const isSummaryMetric = (m) => m in SUMMARY_BASE
  const SUMMARY_SCOPES = [
    { value: 'activity', label: 'Whole activity' },
    { value: 'overlay', label: 'Overlay window' },
  ]

  // Running (cumulative-to-current-point) metrics: counters that accumulate
  // from the ride start up to the current frame, so they tick up as the render
  // sweeps — the readouts for a time-lapse ride-summary flyover. Like summary
  // metrics, each maps to the base telemetry attribute it needs.
  const RUNNING_BASE = {
    running_time: 'time',
    running_distance: 'distance',
    running_elevation_gain: 'elevation',
    running_elevation_loss: 'elevation',
  }
  const ALL_RUNNING_METRICS = Object.keys(RUNNING_BASE)
  const isRunningMetric = (m) => m in RUNNING_BASE

  // Lap metrics (imported as ALL_LAP_METRICS): crit lap counters driven by
  // the scene-level start/finish gate (scene.lap_gate) — see the race bar in
  // PlaybackControls. They need GPS course data to detect crossings.

  // Friendly labels for metrics whose raw key isn't self-explanatory.
  const METRIC_LABELS = {
    power_to_weight: 'W/kg',
    total_distance: 'Total distance',
    total_time: 'Total time',
    elevation_gain: 'Elevation gain',
    elevation_loss: 'Elevation loss',
    max_elevation: 'Max elevation',
    min_elevation: 'Min elevation',
    avg_speed: 'Avg speed',
    max_speed: 'Max speed',
    avg_power: 'Avg power',
    max_power: 'Max power',
    avg_heartrate: 'Avg heart rate',
    max_heartrate: 'Max heart rate',
    avg_cadence: 'Avg cadence',
    running_time: 'Running time',
    running_distance: 'Running distance',
    running_elevation_gain: 'Running climb',
    running_elevation_loss: 'Running descent',
    lap: 'Current lap',
    laps_to_go: 'Laps to go',
    lap_fraction: 'Lap count (2/20)',
  }
  const metricLabel = (m) => METRIC_LABELS[m] ?? m
  const ALL_PLOT_METRICS = ['elevation', 'speed', 'heartrate', 'power', 'cadence', 'gradient', 'temperature', 'front_gear', 'rear_gear', 'course', 'distance']
  const ALL_METER_METRICS = ['speed', 'heartrate', 'power', 'power_to_weight', 'elevation', 'cadence', 'gradient', 'temperature', 'front_gear', 'rear_gear']

  function filterMetrics(list) {
    const valid = app.activityMetrics
    if (!valid) return list
    return list.filter((m) => valid.includes(m))
  }

  // Summary metrics are available when their base telemetry attribute is
  // present in the loaded activity.
  function filterSummary(list) {
    const valid = app.activityMetrics
    if (!valid) return list
    return list.filter((m) => valid.includes(SUMMARY_BASE[m]))
  }

  // Running metrics are available when their base telemetry attribute is
  // present in the loaded activity.
  function filterRunning(list) {
    const valid = app.activityMetrics
    if (!valid) return list
    return list.filter((m) => valid.includes(RUNNING_BASE[m]))
  }

  // Lap metrics all detect crossings from the GPS track, so they're available
  // exactly when the activity has course data.
  function filterLap(list) {
    const valid = app.activityMetrics
    if (!valid) return list
    return valid.includes('course') ? list : []
  }

  const METRICS = $derived(filterMetrics(ALL_METRICS))
  const SUMMARY_METRICS = $derived(filterSummary(ALL_SUMMARY_METRICS))
  const RUNNING_METRICS = $derived(filterRunning(ALL_RUNNING_METRICS))
  const LAP_METRICS = $derived(filterLap(ALL_LAP_METRICS))
  // Value-element dropdown: live metrics first, then running counters, then
  // summary metrics, each in its own labeled group.
  const VALUE_METRIC_OPTIONS = $derived([
    ...METRICS.map((m) => ({ value: m, label: metricLabel(m), group: 'Live' })),
    ...RUNNING_METRICS.map((m) => ({ value: m, label: metricLabel(m), group: 'Running' })),
    ...LAP_METRICS.map((m) => ({ value: m, label: metricLabel(m), group: 'Laps' })),
    ...SUMMARY_METRICS.map((m) => ({ value: m, label: metricLabel(m), group: 'Summary' })),
  ])
  const PLOT_METRICS = $derived(filterMetrics(ALL_PLOT_METRICS))
  const METER_METRICS = $derived(filterMetrics(ALL_METER_METRICS))
  const METER_DIRECTIONS = [
    { value: 'up', label: 'Fill upward' },
    { value: 'down', label: 'Fill downward' },
    { value: 'right', label: 'Fill rightward' },
    { value: 'left', label: 'Fill leftward' },
  ]
  // Metrics that can drive plot band colors (must have per-frame plot data).
  const ALL_COLOR_BY_METRICS = ['gradient', 'speed', 'power', 'heartrate', 'cadence', 'temperature', 'elevation']
  const COLOR_BY_METRICS = $derived(filterMetrics(ALL_COLOR_BY_METRICS))
  const COLOR_BY_MODES = [
    { value: 'fill', label: 'Fill under curve' },
    { value: 'line', label: 'Line' },
    { value: 'both', label: 'Fill + line' },
  ]
  // Unit the band thresholds are read in ('' = the metric's default display
  // unit, matching units::resolve on the Rust side).
  const COLOR_BY_UNITS = {
    speed: [{ value: '', label: 'km/h' }, { value: 'mph', label: 'mph' }],
    temperature: [{ value: '', label: '°C' }, { value: 'f', label: '°F' }],
    elevation: [{ value: '', label: 'm' }, { value: 'ft', label: 'ft' }],
  }
  const COLOR_BY_UNIT_HINTS = { gradient: '%', power: 'W', heartrate: 'bpm', cadence: 'rpm' }
  function colorByUnitHint(cb) {
    const metric = cb.value ?? 'gradient'
    const units = COLOR_BY_UNITS[metric]
    if (units) return (units.find((u) => u.value === (cb.unit ?? '')) ?? units[0]).label
    return COLOR_BY_UNIT_HINTS[metric] ?? ''
  }
  const COURSE_MARKER_STYLES = [
    { value: 'checkered', label: 'Checkered finish' },
    { value: 'circle', label: 'Colored circle' },
    { value: 'rectangle', label: 'Colored rectangle' },
  ]
  const DISTANCE_REFERENCES = [
    { value: 'overlay_start', label: 'Since overlay start' },
    { value: 'activity_start', label: 'Since activity start' },
    { value: 'overlay_end', label: 'Until overlay end' },
    { value: 'activity_end', label: 'Until activity end' },
    { value: 'until_custom', label: 'Until custom point' },
    { value: 'since_custom', label: 'Since custom point' },
  ]
  const TIME_REFERENCES = [
    { value: 'overlay_start', label: 'Since overlay start' },
    { value: 'activity_start', label: 'Since activity start' },
    { value: 'overlay_end', label: 'Until overlay end' },
    { value: 'activity_end', label: 'Until activity end' },
    { value: 'until_custom', label: 'Until custom point' },
    { value: 'since_custom', label: 'Since custom point' },
    { value: 'time_of_day', label: 'Time of day' },
  ]
  const TIME_FORMATS = [
    { value: 'hh:mm:ss', label: 'HH:MM:SS' },
    { value: 'hh:mm', label: 'HH:MM' },
    { value: 'mm:ss', label: 'MM:SS' },
    { value: 'h m', label: '2h 34m' },
    { value: 's', label: 'Seconds' },
    { value: 'm', label: 'Minutes' },
    { value: 'h', label: 'Hours' },
  ]
  const isClockFormat = (fmt) => !fmt || fmt === 'hh:mm:ss' || fmt === 'hh:mm'

  // Curated IANA timezone list. Grouped by region for the <optgroup> Select.
  // Rust rendering uses `hours_offset`; this list drives the friendly picker
  // and lets us compute the correct DST-aware offset at the activity's time.
  const TIMEZONES = [
    { tz: 'Pacific/Honolulu',               label: 'Hawaii',                      group: 'Americas' },
    { tz: 'America/Anchorage',              label: 'Alaska',                      group: 'Americas' },
    { tz: 'America/Los_Angeles',            label: 'Pacific Time',                group: 'Americas' },
    { tz: 'America/Denver',                 label: 'Mountain Time',               group: 'Americas' },
    { tz: 'America/Phoenix',                label: 'Arizona (no DST)',             group: 'Americas' },
    { tz: 'America/Chicago',                label: 'Central Time',                group: 'Americas' },
    { tz: 'America/New_York',               label: 'Eastern Time',                group: 'Americas' },
    { tz: 'America/Halifax',                label: 'Atlantic Time',               group: 'Americas' },
    { tz: 'America/St_Johns',               label: 'Newfoundland',                group: 'Americas' },
    { tz: 'America/Sao_Paulo',              label: 'Brasília',                    group: 'Americas' },
    { tz: 'America/Argentina/Buenos_Aires', label: 'Buenos Aires',                group: 'Americas' },
    { tz: 'America/Santiago',               label: 'Santiago',                    group: 'Americas' },
    { tz: 'Atlantic/Reykjavik',             label: 'Iceland (no DST)',             group: 'Europe' },
    { tz: 'Europe/London',                  label: 'London',                      group: 'Europe' },
    { tz: 'Europe/Paris',                   label: 'Paris, Berlin, Madrid',       group: 'Europe' },
    { tz: 'Europe/Helsinki',                label: 'Helsinki, Athens',            group: 'Europe' },
    { tz: 'Europe/Moscow',                  label: 'Moscow (no DST)',              group: 'Europe' },
    { tz: 'Africa/Lagos',                   label: 'Lagos, Casablanca',           group: 'Africa' },
    { tz: 'Africa/Johannesburg',            label: 'Johannesburg',                group: 'Africa' },
    { tz: 'Africa/Nairobi',                 label: 'Nairobi',                     group: 'Africa' },
    { tz: 'Asia/Riyadh',                    label: 'Riyadh',                      group: 'Middle East' },
    { tz: 'Asia/Tehran',                    label: 'Tehran',                      group: 'Middle East' },
    { tz: 'Asia/Dubai',                     label: 'Dubai, Abu Dhabi',            group: 'Middle East' },
    { tz: 'Asia/Kabul',                     label: 'Kabul',                       group: 'Middle East' },
    { tz: 'Asia/Karachi',                   label: 'Karachi',                     group: 'Asia' },
    { tz: 'Asia/Kolkata',                   label: 'Mumbai, Kolkata',             group: 'Asia' },
    { tz: 'Asia/Kathmandu',                 label: 'Kathmandu',                   group: 'Asia' },
    { tz: 'Asia/Dhaka',                     label: 'Dhaka',                       group: 'Asia' },
    { tz: 'Asia/Rangoon',                   label: 'Yangon',                      group: 'Asia' },
    { tz: 'Asia/Bangkok',                   label: 'Bangkok, Jakarta',            group: 'Asia' },
    { tz: 'Asia/Shanghai',                  label: 'Beijing, Shanghai',           group: 'Asia' },
    { tz: 'Asia/Singapore',                 label: 'Singapore',                   group: 'Asia' },
    { tz: 'Asia/Tokyo',                     label: 'Tokyo',                       group: 'Asia' },
    { tz: 'Asia/Seoul',                     label: 'Seoul',                       group: 'Asia' },
    { tz: 'Australia/Perth',                label: 'Perth',                       group: 'Australia / Pacific' },
    { tz: 'Australia/Darwin',               label: 'Darwin (no DST)',              group: 'Australia / Pacific' },
    { tz: 'Australia/Adelaide',             label: 'Adelaide',                    group: 'Australia / Pacific' },
    { tz: 'Australia/Brisbane',             label: 'Brisbane (no DST)',            group: 'Australia / Pacific' },
    { tz: 'Australia/Sydney',               label: 'Sydney, Melbourne',           group: 'Australia / Pacific' },
    { tz: 'Pacific/Auckland',               label: 'Auckland',                    group: 'Australia / Pacific' },
    { tz: 'Pacific/Chatham',                label: 'Chatham Islands',             group: 'Australia / Pacific' },
    { tz: 'Pacific/Apia',                   label: 'Samoa',                       group: 'Australia / Pacific' },
  ]

  /**
   * Compute the UTC offset (decimal hours) for an IANA timezone at a given UTC
   * timestamp. Uses Intl.DateTimeFormat shortOffset to get "GMT+5:30" strings.
   * Falls back to the current time when utcMs is null.
   */
  function getTimezoneOffsetHours(ianaTimezone, utcMs) {
    try {
      const date = new Date(utcMs ?? Date.now())
      const parts = new Intl.DateTimeFormat('en', {
        timeZone: ianaTimezone,
        timeZoneName: 'shortOffset',
      }).formatToParts(date)
      const offsetStr = parts.find(p => p.type === 'timeZoneName')?.value ?? 'GMT+0'
      const m = offsetStr.match(/GMT([+-])(\d+)(?::(\d+))?/)
      if (!m) return 0
      const sign = m[1] === '+' ? 1 : -1
      return sign * (parseInt(m[2], 10) + (parseInt(m[3] ?? '0', 10) / 60))
    } catch {
      return 0
    }
  }

  const tzOptions = TIMEZONES.map(t => ({ value: t.tz, label: t.label, group: t.group }))
  // Per-metric explicit unit options. Metrics absent from this map (gradient,
  // lean, power, cadence, heartrate, time) have no unit choice and render raw.
  // Legacy 'metric'/'imperial' tokens still render — the Rust side normalizes
  // them — but new selections use these precise tokens.
  const UNITS_BY_METRIC = {
    distance: [
      { value: 'km', label: 'Kilometers (km)' },
      { value: 'm', label: 'Meters (m)' },
      { value: 'mi', label: 'Miles (mi)' },
    ],
    speed: [
      { value: 'kmh', label: 'km/h' },
      { value: 'mph', label: 'mph' },
      { value: 'ms', label: 'm/s' },
    ],
    elevation: [
      { value: 'm', label: 'Meters (m)' },
      { value: 'ft', label: 'Feet (ft)' },
    ],
    temperature: [
      { value: 'c', label: 'Celsius (°C)' },
      { value: 'f', label: 'Fahrenheit (°F)' },
    ],
  }
  // Default unit token per metric, used when the element has none set yet
  // (matches the Rust-side default so the picker reflects what renders).
  const DEFAULT_UNIT = { distance: 'km', speed: 'kmh', elevation: 'm', temperature: 'c' }

  // Value suffix modes. "auto" tracks the unit picker (km↔mi, W, bpm, …);
  // "custom" appends the free-text `suffix`; "none" appends nothing.
  const SUFFIX_MODES = [
    { value: 'none', label: 'None' },
    { value: 'auto', label: 'Auto (unit)' },
    { value: 'custom', label: 'Custom' },
  ]
  // Resolve the effective mode for the picker. Templates authored before
  // suffix_mode existed carry a manual `suffix` with no mode → show "Custom".
  function suffixMode(item) {
    if (item.suffix_mode) return item.suffix_mode
    return item.suffix ? 'custom' : 'none'
  }
  function setSuffixMode(v) {
    const s = selected()
    if (!s) return
    // Clear the manual suffix when leaving Custom so it can't linger unseen.
    app.updateElement(s.id, { suffix_mode: v, suffix: v === 'custom' ? s.item.suffix : undefined })
  }
  const markerId = () => `marker-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`
  // Resolve the unit token to show in the picker, mapping legacy
  // metric/imperial (or unset) to the matching precise token.
  function displayUnit(metric, unit) {
    const opts = UNITS_BY_METRIC[metric] ?? []
    if (opts.some((o) => o.value === unit)) return unit
    if (unit === 'imperial') return { distance: 'mi', speed: 'mph', elevation: 'ft', temperature: 'f' }[metric]
    return DEFAULT_UNIT[metric]
  }

  // App-wide unit system ("metric" | "imperial"), default metric — set once
  // in Settings, injected into the scene at render time.
  function sceneUnitSystem() {
    return app.units === 'imperial' ? 'imperial' : 'metric'
  }
  // Concrete token a metric resolves to under the current scene system —
  // used to label the "Auto (…)" picker row so it names what it inherits.
  function sceneUnit(metric) {
    return displayUnit(metric, sceneUnitSystem())
  }
  // Concrete unit an element actually renders in: its explicit unit, or the
  // scene system when it's left on Auto (unit unset).
  function effectiveUnit(metric, unit) {
    return unit == null ? sceneUnit(metric) : displayUnit(metric, unit)
  }
  // Value shown in a unit picker: '' = Auto (inherit scene), else the concrete
  // token (legacy metric/imperial map onto their precise option).
  function unitValue(metric, unit) {
    return unit == null ? '' : displayUnit(metric, unit)
  }
  // Picker options with an Auto row on top that names the inherited unit.
  function unitOptions(metric) {
    return [{ value: '', label: `Auto (${sceneUnit(metric)})` }, ...(UNITS_BY_METRIC[metric] ?? [])]
  }
  // Store a picked unit; '' (Auto) clears the field so the element inherits.
  function changeUnit(v) {
    update('unit', v || undefined)
  }

  let selected = $derived(() => {
    const id = app.selectedElementId
    if (!id || !app.config?.elements) return null
    const item = app.config.elements.find((e) => e.id === id)
    return item ? { id, item, type: item.type } : null
  })

  // Scene color variables — passed to every ColorInput so vars can be selected
  const sceneVars = $derived(app.config?.scene?.vars ?? {})

  function update(field, raw) {
    const s = selected()
    if (!s) return
    const numFields = ['x', 'y', 'width', 'height', 'font_size', 'letter_spacing', 'opacity', 'fill_opacity', 'decimal_rounding', 'rotation', 'distance_target', 'time_target', 'hours_offset', 'radius', 'start_angle', 'sweep_angle', 'arc_width', 'needle_width', 'cap_radius', 'segments', 'gap', 'background_opacity', 'background_margin', 'border_width', 'border_opacity', 'scale_font_size', 'scale_offset', 'scale_tick_length', 'scale_tick_width', 'scale_ticks', 'pulse_bpm', 'pulse_amplitude']
    const rangeBoundFields = ['min', 'max']
    let value = rangeBoundFields.includes(field)
      ? parseRangeBound(raw)
      : numFields.includes(field) ? (raw === '' ? undefined : Number(raw)) : raw
    value = normalizeElementField(field, value)
    app.updateElement(s.id, { [field]: value })
  }

  function parseRangeBound(raw) {
    const trimmed = String(raw ?? '').trim().toLowerCase()
    if (trimmed === '') return undefined
    if (trimmed === 'min' || trimmed === 'max') return trimmed
    const value = Number(trimmed)
    return Number.isFinite(value) ? value : undefined
  }

  const rangeCache = new SvelteMap()

  function metricRangeUnit(item) {
    if (!item?.value) return null
    return UNITS_BY_METRIC[item.value] ? effectiveUnit(item.value, item.unit) : item.unit
  }

  function rangeContext() {
    if (!app.hasActivity || !app.gpxFilename || !app.config?.scene) return null
    return {
      gpx: app.gpxFilename,
      start: app.config.scene.start ?? 0,
      end: app.config.scene.end ?? app.timelineDuration,
      riderWeightKg: app.riderWeightKg,
    }
  }

  // W/kg (power_to_weight) is the only metric whose range depends on rider
  // weight, so weight is part of its cache key; other metrics ignore it.
  const weightAffectsRange = (metric) => metric === 'power_to_weight'

  function rangeKey(metric, unit, context = rangeContext()) {
    if (!context || !metric) return null
    return JSON.stringify([
      context.gpx,
      metric,
      unit ?? null,
      context.start,
      context.end,
      weightAffectsRange(metric) ? (context.riderWeightKg ?? null) : null,
    ])
  }

  function loadRange(metric, unit, context = rangeContext()) {
    const key = rangeKey(metric, unit, context)
    if (!key) return
    const current = rangeCache.get(key)
    if (current?.status === 'loading' || current?.status === 'ready') return

    rangeCache.set(key, { status: 'loading' })
    backend.getActivityMetricRange(
      context.gpx,
      metric,
      unit,
      context.start,
      context.end,
      weightAffectsRange(metric) ? context.riderWeightKg : null,
    )
      .then((range) => {
        rangeCache.set(key, { status: 'ready', range })
      })
      .catch((err) => {
        rangeCache.set(key, {
          status: 'error',
          error: err?.message ?? String(err),
        })
      })
  }

  function formatRangeValue(value) {
    if (!Number.isFinite(value)) return 'unavailable'
    const abs = Math.abs(value)
    if (abs >= 1000) return value.toLocaleString(undefined, { maximumFractionDigits: 1 })
    if (abs >= 100) return value.toLocaleString(undefined, { maximumFractionDigits: 2 })
    if (abs >= 10) return value.toLocaleString(undefined, { maximumFractionDigits: 3 })
    return value.toLocaleString(undefined, { maximumFractionDigits: 4 })
  }

  function rangeBoundTooltip(item, field) {
    const bound = item?.[field]
    if (bound !== 'min' && bound !== 'max') return ''
    if (!app.hasActivity) return 'Load an activity'
    const entry = rangeCache.get(rangeKey(item.value, metricRangeUnit(item)))
    if (entry?.status === 'ready') {
      const value = entry.range?.[bound]
      const issue = metricValueIssue(item.value, metricRangeUnit(item), value)
      if (issue) {
        return `${formatRangeValue(value)}
Looks unrealistic for ${item.value} (expected ${issue.expected}). Enter a manual ${field}.`
      }
      return formatRangeValue(value)
    }
    if (entry?.status === 'error') return entry.error
    return 'Computing...'
  }

  function rangeWarning(item) {
    if (!item) return null
    const entry = rangeCache.get(rangeKey(item.value, metricRangeUnit(item)))
    if (entry?.status !== 'ready') return null
    const issues = metricRangeIssues(item.value, metricRangeUnit(item), entry.range)
    const fields = ['min', 'max']
    const field = fields.find((f) => {
      const bound = item[f]
      return (bound === 'min' || bound === 'max') && issues[bound]
    })
    const issue = field ? issues[item[field]] : null
    if (!issue) return null
    return `Computed ${field} ${formatRangeValue(issue.value)} looks unrealistic for ${item.value}. Enter a manual ${field}.`
  }

  $effect(() => {
    const context = rangeContext()
    if (!context) return

    for (const metric of METER_METRICS) {
      loadRange(metric, metricRangeUnit({ value: metric }), context)
    }

    const s = selected()
    const item = s?.item
    if (s?.type === 'meter' || s?.type === 'gauge') {
      loadRange(item?.value, metricRangeUnit(item), context)
    }
  })

  // Switch the distance unit, converting distance_target to the equivalent
  // value in the new unit so the on-screen target stays the same real distance.
  function changeDistanceUnit(newUnit) {
    const s = selected()
    if (!s) return
    // '' = Auto (inherit scene); convert the target against the concrete unit
    // each side actually resolves to, but persist the raw choice (undefined for
    // Auto) so the element keeps following the scene toggle.
    const oldUnit = effectiveUnit('distance', s.item.unit)
    const newConcrete = newUnit === '' ? sceneUnit('distance') : newUnit
    const updates = { unit: newUnit || undefined }
    const t = s.item.distance_target
    if (oldUnit !== newConcrete && t != null && t !== '' && !Number.isNaN(Number(t))) {
      const toM = (v, u) => (u === 'm' ? v : u === 'mi' ? v * 1609.34 : v * 1000)
      const fromM = (m, u) => (u === 'm' ? m : u === 'mi' ? m / 1609.34 : m / 1000)
      const meters = toM(Number(t), oldUnit)
      const conv = fromM(meters, newConcrete)
      updates.distance_target =
        newConcrete === 'm' ? Math.round(conv) : Math.round(conv * 100) / 100
    }
    app.updateElement(s.id, updates)
  }

  // Update a nested object field: updateNested('line', 'color', '#fff')
  function updateNested(obj, field, raw) {
    const s = selected()
    if (!s) return
    const numFields = ['width', 'opacity', 'past_opacity', 'future_opacity']
    const value = numFields.includes(field) ? (raw === '' ? undefined : Number(raw)) : raw
    const current = s.item[obj] ?? {}
    app.updateElement(s.id, { [obj]: { ...current, [field]: value } })
  }

  // Update point — the single tracking marker. Creates it if absent.
  function updatePoint(field, raw) {
    const s = selected()
    if (!s) return
    const numFields = ['weight', 'opacity', 'edge_width']
    const value = numFields.includes(field) ? (raw === '' ? undefined : Number(raw)) : raw
    const current = s.item.point ?? {}
    app.updateElement(s.id, { point: { ...current, [field]: value } })
  }

  function courseMarkers() {
    return selected()?.item.markers ?? []
  }

  function selectedCourseMarker() {
    const markers = courseMarkers()
    if (markers.length === 0) return null
    return markers.find((m) => m.id === app.selectedCourseMarkerId) ?? markers[0]
  }

  function setCourseMarkers(markers) {
    const s = selected()
    if (!s) return
    app.updateElement(s.id, { markers: markers.length ? markers : undefined })
  }

  async function defaultCourseMarkerDistance(markers) {
    if (markers.length > 0) return markers[markers.length - 1]?.distance ?? 0
    if (!app.hasActivity) return 0
    try {
      const start = app.config?.scene?.start ?? 0
      const end = app.config?.scene?.end ?? app.timelineDuration
      const info = await backend.getActivityDistanceInfo(app.gpxFilename, start, end)
      return info.overlay_end_m ?? info.total_m ?? 0
    } catch {
      return 0
    }
  }

  async function addCourseMarker() {
    const markers = courseMarkers()
    const id = markerId()
    const distance = await defaultCourseMarkerDistance(markers)
    setCourseMarkers([
      ...markers,
      {
        id,
        name: markers.length === 0 ? 'Finish' : `Marker ${markers.length + 1}`,
        style: 'checkered',
        color: '#ef4444',
        distance,
        width: 34,
        height: 10,
        rotation: 0,
        opacity: 1,
      },
    ])
    app.selectedCourseMarkerId = id
  }

  function updateCourseMarker(field, raw) {
    const marker = selectedCourseMarker()
    if (!marker) return
    const numFields = ['distance', 'width', 'height', 'rotation', 'opacity']
    const value = numFields.includes(field) ? (raw === '' ? undefined : Number(raw)) : raw
    setCourseMarkers(courseMarkers().map((m) => (
      m === marker || (marker.id && m.id === marker.id) ? { ...m, [field]: value } : m
    )))
  }

  function removeCourseMarker(id) {
    const marker = selectedCourseMarker()
    const next = courseMarkers().filter((m) => (
      marker ? !(m === marker || (id && m.id === id)) : m.id !== id
    ))
    setCourseMarkers(next)
    app.selectedCourseMarkerId = next[0]?.id ?? null
  }

  // ── Start/finish gate (scene.lap_gate) driving the lap metrics ────────────
  // One gate per scene: every lap element counts crossings of the same line.
  // Race start/finish moments are set on the dual-handle race bar under the
  // preview (PlaybackControls); this panel holds the numeric knobs.

  const lapGate = () => app.config?.scene?.lap_gate ?? null

  // Point label (value text next to the chart marker, e.g. "960 M").
  const POINT_LABEL_DEFAULT = {
    font: 'Furore.otf',
    font_size: 64,
    italic: false,
    color: '#ffffffc8',
    x_offset: 30,
    y_offset: 30,
    units: ['metric', 'imperial'],
    decimal_rounding: 0,
  }

  function togglePointLabel(enabled) {
    const s = selected()
    if (!s) return
    app.updateElement(s.id, {
      point_label: enabled ? { ...POINT_LABEL_DEFAULT } : undefined,
    })
  }

  function updatePL(field, raw) {
    const s = selected()
    if (!s) return
    const numFields = ['font_size', 'x_offset', 'y_offset', 'decimal_rounding']
    let value = numFields.includes(field)
      ? raw === ''
        ? undefined
        : Number(raw)
      : raw
    value = normalizeTemplateIntegerField(field, value)
    const current = s.item.point_label ?? {}
    app.updateElement(s.id, {
      point_label: { ...current, [field]: value },
    })
  }

  // units is an ordered array; keep metric before imperial.
  function toggleUnit(unit, on) {
    const s = selected()
    if (!s) return
    const cur = s.item.point_label?.units ?? []
    const next = on ? [...cur, unit] : cur.filter((u) => u !== unit)
    const units = ['metric', 'imperial'].filter((u) => next.includes(u))
    const current = s.item.point_label ?? {}
    app.updateElement(s.id, {
      point_label: { ...current, units },
    })
  }

  function numVal(item, field) {
    return item[field] ?? ''
  }

  function numericBound(value, fallback) {
    return typeof value === 'number' && Number.isFinite(value) ? value : fallback
  }

  function updateImageSize(field, raw) {
    const s = selected()
    if (!s || s.type !== 'image') return
    const val = raw === '' ? undefined : Number(raw)
    if (!val || val <= 0) { app.updateElement(s.id, { [field]: val }); return }
    const nw = s.item.natural_width ?? s.item.width ?? 200
    const nh = s.item.natural_height ?? s.item.height ?? 200
    const ratio = nw / nh
    if (field === 'width') {
      app.updateElement(s.id, { width: Math.round(val), height: Math.round(val / ratio) })
    } else {
      app.updateElement(s.id, { width: Math.round(val * ratio), height: Math.round(val) })
    }
  }

  async function applyAsset(name) {
    const s = selected()
    if (!s) return
    const updates = { file: name }
    try {
      const size = await backend.imageSize(name)
      const width = s.item.width ?? size.width
      updates.width = Math.round(width)
      updates.height = Math.round(width * (size.height / size.width))
      updates.natural_width = size.width
      updates.natural_height = size.height
    } catch { /* fallback: keep existing dimensions */ }
    app.updateElement(s.id, updates)
  }

  // Color row helper — returns [colorValue, hexDisplay]
  function colorRow(obj, field, fallback = '#ffffff') {
    const s = selected()
    return s?.item[obj]?.[field] ?? fallback
  }

  // The element's representative color, shown by the single basic-mode swatch.
  function primaryColor() {
    const s = selected()
    if (!s) return '#ffffff'
    const it = s.item
    return (
      it.color ??
      it.line?.color ??
      it.point?.color ??
      '#ffffff'
    )
  }

  // Basic mode: one color drives every color on the element. Applied as a
  // single updateElement call so it's one undo step.
  function setAllColors(raw) {
    const s = selected()
    if (!s) return
    if (s.type === 'plot') {
      const it = s.item
      const patch = {
        color: raw,
        line: { ...(it.line ?? {}), color: raw },
        fill: { ...(it.fill ?? {}), color: raw },
      }
      if (it.point) patch.point = { ...it.point, color: raw }
      if (it.point_label) patch.point_label = { ...it.point_label, color: raw }
      app.updateElement(s.id, patch)
    } else {
      app.updateElement(s.id, { color: raw })
    }
  }

  // ── Plot color-by (banded coloring by a second metric) ────────────────────
  function colorBy() {
    return selected()?.item.color_by ?? null
  }
  function setColorBy(next) {
    const s = selected()
    if (!s) return
    app.updateElement(s.id, { color_by: next })
  }
  function toggleColorBy(on) {
    if (!on) return setColorBy(undefined)
    const s = selected()
    if (!s) return
    // Banded fills read best solid; lift a translucent fill to full opacity
    // in the same (single-undo) update.
    const patch = { color_by: { value: 'gradient', bands: defaultColorBands('gradient') } }
    const fillOpacity = s.item.fill?.opacity
    if (s.item.value !== 'course' && typeof fillOpacity === 'number' && fillOpacity < 1) {
      patch.fill = { ...s.item.fill, opacity: 1 }
    }
    app.updateElement(s.id, patch)
  }
  // Changing the driving metric resets bands — thresholds are metric-specific.
  function setColorByMetric(metric) {
    setColorBy({ ...colorBy(), value: metric, unit: undefined, bands: defaultColorBands(metric) })
  }
  function setColorByField(field, value) {
    setColorBy({ ...colorBy(), [field]: value })
  }
  function colorByBands() {
    return colorBy()?.bands ?? []
  }
  function updateColorBand(i, patch) {
    const bands = colorByBands().map((b, idx) => (idx === i ? { ...b, ...patch } : b))
    setColorBy({ ...colorBy(), bands })
  }
  function removeColorBand(i) {
    setColorBy({ ...colorBy(), bands: colorByBands().filter((_, idx) => idx !== i) })
  }
  function addColorBand() {
    const bands = [...colorByBands()]
    const maxes = bands.map((b) => b.max).filter((m) => typeof m === 'number').sort((a, b) => a - b)
    const top = maxes.at(-1)
    const step = maxes.length >= 2 ? maxes.at(-1) - maxes.at(-2) : 5
    const next = { max: top != null ? top + step : 0, color: bands.at(-1)?.color ?? '#ffffff' }
    // Keep the catch-all (no max) band last.
    const catchAll = bands.length && bands.at(-1).max == null ? bands.pop() : null
    bands.push(next)
    if (catchAll) bands.push(catchAll)
    setColorBy({ ...colorBy(), bands })
  }

  // Meter gradient stops (ordered min→max). Absent = solid `color`.
  function meterGradient() {
    return selected()?.item.gradient ?? []
  }
  function setGradient(stops) {
    const s = selected()
    if (!s) return
    app.updateElement(s.id, { gradient: stops.length ? stops : undefined })
  }
  function updateGradientStop(i, val) {
    const stops = [...meterGradient()]
    stops[i] = val
    setGradient(stops)
  }
  function addGradientStop() {
    const stops = meterGradient()
    setGradient([...stops, stops[stops.length - 1] ?? '#ffffff'])
  }
  function removeGradientStop(i) {
    setGradient(meterGradient().filter((_, idx) => idx !== i))
  }

  // Continuous fill gradient: stops[0] = low-value color, stops[1] = high-value color.
  // Initialises both stops from the current solid color when the array is empty.
  function updateContinuousGradientStop(idx, val) {
    const cur = meterGradient()
    const base = selected()?.item.color ?? '#ffffff'
    const stops = cur.length >= 2 ? [...cur] : [base, base]
    stops[idx] = val
    setGradient(stops.slice(0, 2))
  }

  // Scale labels: null = disabled, [] = auto (min/mid/max), [v,...] = explicit values.
  function scaleLabels() { return selected()?.item.scale_labels ?? null }
  function scaleEnabled() { return scaleLabels() !== null && scaleLabels() !== undefined }
  function enableScale() {
    const s = selected(); if (!s) return
    app.updateElement(s.id, { scale_labels: [] })
  }
  function disableScale() {
    const s = selected(); if (!s) return
    app.updateElement(s.id, { scale_labels: undefined })
  }
  function addScaleLabel() {
    const s = selected(); if (!s) return
    const cur = scaleLabels() ?? []
    const min = numericBound(s.item.min, 0)
    const max = numericBound(s.item.max, 1)
    const mid = (min + max) / 2
    app.updateElement(s.id, { scale_labels: [...cur, cur.length === 0 ? min : mid] })
  }
  function updateScaleLabel(i, raw) {
    const s = selected(); if (!s) return
    const cur = [...(scaleLabels() ?? [])]
    cur[i] = raw === '' ? 0 : Number(raw)
    app.updateElement(s.id, { scale_labels: cur })
  }
  function removeScaleLabel(i) {
    const s = selected(); if (!s) return
    app.updateElement(s.id, { scale_labels: (scaleLabels() ?? []).filter((_, idx) => idx !== i) })
  }

  // ── Anchoring ──────────────────────────────────────────────────────────────
  // Anchored elements derive x/y from a point on a target element's box (the
  // Rust pre-pass resolves it before every render). Labels/values/rects/images
  // can be anchored; box elements (plot/meter/gauge/rect/image) are targets.
  const ANCHORABLE_TYPES = ['label', 'value', 'rect', 'image']
  const ANCHOR_TARGET_TYPES = ['plot', 'meter', 'gauge', 'rect', 'image']
  const ANCHOR_POINTS = [
    { value: 'top-left', label: 'Top left' },
    { value: 'top', label: 'Top' },
    { value: 'top-right', label: 'Top right' },
    { value: 'left', label: 'Left' },
    { value: 'center', label: 'Center' },
    { value: 'right', label: 'Right' },
    { value: 'bottom-left', label: 'Bottom left' },
    { value: 'bottom', label: 'Bottom' },
    { value: 'bottom-right', label: 'Bottom right' },
  ]
  // prettier-ignore
  const POINT_FRACS = {
    'top-left': [0, 0], top: [0.5, 0], 'top-right': [1, 0],
    left: [0, 0.5], center: [0.5, 0.5], right: [1, 0.5],
    'bottom-left': [0, 1], bottom: [0.5, 1], 'bottom-right': [1, 1],
  }
  // Text alignment presets per anchor point: keep the text inside the target
  // box (anchor to "left" edge → text grows rightward). Users can override
  // via the Alignment selects afterwards.
  const ANCHOR_TEXT_ALIGN = { 'top-left': 'left', left: 'left', 'bottom-left': 'left', 'top-right': 'right', right: 'right', 'bottom-right': 'right' }
  const ANCHOR_VERTICAL_ALIGN = { 'top-left': 'top', top: 'top', 'top-right': 'top', 'bottom-left': 'bottom', bottom: 'bottom', 'bottom-right': 'bottom' }

  function anchorTargetOpts() {
    const s = selected()
    if (!s) return []
    const els = app.config?.elements ?? []
    // An element whose anchor chain already reaches us can't be our target.
    const chainHitsSelf = (el) => {
      let cur = el
      const seen = []
      while (cur?.anchor?.target && !seen.includes(cur.id)) {
        if (cur.anchor.target === s.id) return true
        seen.push(cur.id)
        cur = els.find((e) => e.id === cur.anchor.target)
      }
      return false
    }
    return els
      .filter((e) => ANCHOR_TARGET_TYPES.includes(e.type) && e.id !== s.id && !chainHitsSelf(e))
      .map((e) => ({ value: e.id, label: e.id }))
  }

  function textAlignPresets(type, point) {
    if (type !== 'label' && type !== 'value') return {}
    return {
      text_align: ANCHOR_TEXT_ALIGN[point] ?? 'center',
      vertical_align: ANCHOR_VERTICAL_ALIGN[point] ?? 'middle',
    }
  }

  function setAnchorTarget(v) {
    const s = selected()
    if (!s) return
    if (!v) {
      detachAnchor()
      return
    }
    const point = s.item.anchor?.point ?? 'center'
    app.updateElement(s.id, {
      anchor: { ...s.item.anchor, target: v, point, offset_x: 0, offset_y: 0 },
      ...textAlignPresets(s.type, point),
    })
  }

  function setAnchorPoint(v) {
    const s = selected()
    if (!s) return
    app.updateElement(s.id, {
      anchor: { ...s.item.anchor, point: v },
      ...textAlignPresets(s.type, v),
    })
  }

  function setAnchorOffset(axis, raw) {
    const s = selected()
    if (!s) return
    const n = raw === '' ? 0 : Math.round(Number(raw))
    if (!Number.isFinite(n)) return
    app.updateElement(s.id, { anchor: { ...s.item.anchor, [axis]: n } })
  }

  // Remove the anchor, baking the resolved position into x/y so the element
  // doesn't jump. Mirrors the Rust resolve math.
  function detachAnchor() {
    const s = selected()
    if (!s) return
    const a = s.item.anchor
    const t = (app.config?.elements ?? []).find((e) => e.id === a?.target)
    const updates = { anchor: undefined }
    if (a && t && t.width != null && t.height != null) {
      const [fx, fy] = POINT_FRACS[a.point ?? 'center'] ?? [0.5, 0.5]
      let x = (t.x ?? 0) + t.width * fx + (a.offset_x ?? 0)
      let y = (t.y ?? 0) + t.height * fy + (a.offset_y ?? 0)
      if (s.type === 'rect' || s.type === 'image') {
        const [sfx, sfy] = POINT_FRACS[a.self_point ?? 'center'] ?? [0.5, 0.5]
        x -= (s.item.width ?? 0) * sfx
        y -= (s.item.height ?? 0) * sfy
      }
      updates.x = Math.round(x)
      updates.y = Math.round(y)
    }
    app.updateElement(s.id, updates)
  }

  // Progressive disclosure: most overlays reuse the same colors/fonts/line
  // weights, so detailed/structural controls hide behind "Advanced".
  let showAdvanced = $state(false)
  let showAssetPicker = $state(false)

  function applyMeterActivityRange() {
    const s = selected()
    if (!s || s.type !== 'meter') return
    app.updateElement(s.id, { min: 'min', max: 'max' })
  }

  function applyGaugeActivityRange() {
    const s = selected()
    if (!s || s.type !== 'gauge') return
    app.updateElement(s.id, { min: 'min', max: 'max' })
  }
</script>

<div class="h-full overflow-y-auto px-4 py-3">
  {#if app.selectedGroupId}
    {@const group = (app.config?.scene?.groups ?? []).find((g) => g.id === app.selectedGroupId)}
    {#if group}
      <div class="mb-3 flex items-center gap-2">
        <Folder size={13} class="shrink-0 text-zinc-400" />
        <p class="text-[10px] font-semibold uppercase tracking-wider text-zinc-500">Group</p>
      </div>
      <div class="space-y-3">
        <div>
          <label for="group-name" class="block text-[10px] text-zinc-500 mb-1">Name</label>
          <input
            id="group-name"
            class="w-full bg-[var(--panel2)] border border-transparent rounded px-2 py-1.5 text-xs text-zinc-100 outline-none focus:border-primary transition-colors"
            value={group.name}
            onchange={(e) => app.renameGroup(group.id, e.currentTarget.value)}
          />
        </div>
        <div class="flex items-center justify-between">
          <span class="text-xs text-zinc-500">{group.element_ids.length} element{group.element_ids.length !== 1 ? 's' : ''}</span>
          <button
            onclick={() => app.deleteGroup(group.id)}
            class="cursor-pointer flex items-center gap-1.5 px-2 py-1 rounded text-xs text-zinc-400 hover:text-destructive hover:bg-[var(--panel2)] transition-colors"
            title="Ungroup (elements remain)"
          >
            <Ungroup size={12} />
            Ungroup
          </button>
        </div>
        <p class="text-[10px] text-zinc-600 italic">Drag any element in this group on the canvas to move them all together.</p>
      </div>
    {/if}
  {:else if !selected()}
    <div class="flex h-full items-center justify-center">
      <p class="text-xs text-zinc-600 italic text-center">Click an element on the canvas<br>or in the list to edit it.</p>
    </div>
  {:else}
    {@const { id, item, type } = selected()}
    {@const meta = elementMeta(item)}
    {@const HeaderIcon = { type: Type, hash: Hash, bar: BarChart2, map: MapIcon, meter: Thermometer, gauge: GaugeIcon, rect: Square, image: ImageIcon }[meta.icon] ?? BarChart2}

    <!-- Header: element icon chip + name + type -->
    <div class="-mx-4 mb-4 flex items-center gap-2.5 border-b border-white/[0.06] px-4 pb-3">
      <span class="flex h-[30px] w-[30px] shrink-0 items-center justify-center rounded-lg bg-primary/15 text-primary">
        <HeaderIcon size={15} />
      </span>
      <div class="min-w-0">
        <p class="truncate text-sm font-semibold capitalize text-zinc-100">{meta.name}</p>
        <p class="truncate font-mono text-[11px] text-[var(--dim)]">{elementTypeName(item).toLowerCase()}</p>
      </div>
    </div>

    <!-- Advanced toggle -->
    <div class="mb-4 flex items-center justify-between">
      <span class="text-[10px] uppercase tracking-wider text-zinc-600">Advanced</span>
      <Switch
        checked={showAdvanced}
        ariaLabel="Advanced options"
        onchange={(checked) => (showAdvanced = checked)}
      />
    </div>

    <!-- Align — snap the element to the exact canvas center -->
    <div class="mb-4 flex items-center gap-2">
      <span class="mr-auto text-[10px] uppercase tracking-wider text-zinc-600">Align</span>
      <Tooltip content="Center horizontally">
        <button
          onclick={() => app.alignSelected('h')}
          aria-label="Center horizontally on canvas"
          class="cursor-pointer rounded-[6px] border border-transparent bg-[var(--panel2)] p-1.5 text-zinc-400 transition-colors hover:bg-[var(--panel3)] hover:text-zinc-100"
        >
          <AlignHorizontalJustifyCenter size={14} />
        </button>
      </Tooltip>
      <Tooltip content="Center vertically">
        <button
          onclick={() => app.alignSelected('v')}
          aria-label="Center vertically on canvas"
          class="cursor-pointer rounded-[6px] border border-transparent bg-[var(--panel2)] p-1.5 text-zinc-400 transition-colors hover:bg-[var(--panel3)] hover:text-zinc-100"
        >
          <AlignVerticalJustifyCenter size={14} />
        </button>
      </Tooltip>
    </div>

    <!-- ═══ LABEL ═══ -->
    {#if type === 'label'}
      <!-- Text: all text properties in one place -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Text</p>
        <Input value={item.text ?? ''} oninput={(e) => update('text', e.target.value)} />
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Font</span>
          <Select value={item.font ?? ''} options={fontOpts(true)} onchange={(v) => update('font', v || undefined)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Size</span>
          <Input type="number" value={numVal(item, 'font_size')} placeholder="Scene default" oninput={(e) => update('font_size', e.target.value)} />
        </label>
        <label class="flex items-center justify-between gap-3 rounded-[6px] border border-transparent bg-[var(--panel2)] px-2.5 py-2">
          <span class="text-xs text-zinc-500">Italic</span>
          <Switch checked={item.italic ?? false} ariaLabel="Italic text" onchange={(v) => update('italic', v ? true : undefined)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          <ColorInput value={item.color ?? '#ffffff'} vars={sceneVars} onchange={(v) => update('color', v)} />
        </label>
        {#if showAdvanced}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Letter spacing (px)</span>
          <Input type="number" value={numVal(item, 'letter_spacing')} placeholder="0" step={0.5} oninput={(e) => update('letter_spacing', e.target.value)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Alignment</span>
          <Select
            value={item.text_align ?? 'left'}
            options={[{value:'left',label:'Left'},{value:'center',label:'Center'},{value:'right',label:'Right'}]}
            onchange={(v) => update('text_align', v)}
          />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Vertical alignment</span>
          <Select
            value={item.vertical_align ?? 'baseline'}
            options={[{value:'baseline',label:'Baseline'},{value:'top',label:'Top'},{value:'middle',label:'Middle'},{value:'bottom',label:'Bottom'}]}
            onchange={(v) => update('vertical_align', v === 'baseline' ? undefined : v)}
          />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.opacity ?? app.config?.scene?.opacity ?? 1} oninput={(e) => update('opacity', e.target.value)} />
        </label>
        {/if}
      </section>
    {/if}

    <!-- ═══ VALUE ═══ -->
    {#if type === 'value'}
      <!-- Metric + formatting -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Metric</p>
        <Select
          value={item.value ?? ''}
          options={VALUE_METRIC_OPTIONS}
          onchange={(v) => update('value', v)}
        />

        {#if isSummaryMetric(item.value)}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Scope</span>
          <Select value={item.summary_scope ?? 'activity'} options={SUMMARY_SCOPES} onchange={(v) => update('summary_scope', v)} />
        </label>
        {#if UNITS_BY_METRIC[SUMMARY_BASE[item.value]]}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Unit</span>
          <Select value={unitValue(SUMMARY_BASE[item.value], item.unit)} options={unitOptions(SUMMARY_BASE[item.value])} onchange={(v) => changeUnit(v)} />
        </label>
        {/if}
        {#if item.value === 'total_time'}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Format</span>
          <Select value={item.time_format ?? 'hh:mm:ss'} options={TIME_FORMATS} onchange={(v) => update('time_format', v)} />
        </label>
        {/if}

        {:else if isRunningMetric(item.value)}
        {#if UNITS_BY_METRIC[RUNNING_BASE[item.value]]}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Unit</span>
          <Select value={unitValue(RUNNING_BASE[item.value], item.unit)} options={unitOptions(RUNNING_BASE[item.value])} onchange={(v) => changeUnit(v)} />
        </label>
        {/if}
        {#if item.value === 'running_time'}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Format</span>
          <Select value={item.time_format ?? 'hh:mm:ss'} options={TIME_FORMATS} onchange={(v) => update('time_format', v)} />
        </label>
        {/if}

        {:else if isLapMetric(item.value)}
        {@const gate = lapGate()}
        <div class="space-y-2 rounded-[6px] border border-transparent bg-[var(--panel2)] p-2">
          <p class="text-[10px] uppercase tracking-wider text-zinc-600">Start / Finish line</p>
          <p class="text-[10px] text-zinc-600">
            Drag the handles on the race bar under the preview: green to the
            moment you first cross the line (race start — that position becomes
            the line), checkered to the final crossing (race finish).
          </p>
          {#if gate}
            <div class="grid grid-cols-2 gap-2 font-mono text-[11px] tabular-nums">
              <div class="space-y-1">
                <p class="font-sans text-xs text-zinc-500">Race start</p>
                <p class="text-emerald-500">{formatTime(gate.start ?? 0)}</p>
              </div>
              <div class="space-y-1">
                <p class="font-sans text-xs text-zinc-500">Race finish</p>
                <p class="text-zinc-300">{gate.end != null ? formatTime(gate.end) : 'activity end'}</p>
              </div>
            </div>
            <label class="space-y-1 block">
              <span class="text-xs text-zinc-500">Total laps (blank = auto-detect)</span>
              <Input type="number" value={gate.total_laps ?? ''} min={1} step={1}
                oninput={(e) => app.updateLapGate({ total_laps: e.target.value === '' ? undefined : Math.max(1, Math.round(Number(e.target.value))) })} />
            </label>
            <label class="space-y-1 block">
              <span class="text-xs text-zinc-500">Detection radius (m)</span>
              <Input type="number" value={gate.radius ?? 25} min={5} step={5}
                oninput={(e) => app.updateLapGate({ radius: e.target.value === '' ? undefined : Math.max(1, Number(e.target.value)) })} />
            </label>
          {/if}
        </div>

        {:else if item.value === 'distance'}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Unit</span>
          <Select value={unitValue('distance', item.unit)} options={unitOptions('distance')} onchange={(v) => changeDistanceUnit(v)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Reference</span>
          <Select value={item.distance_reference ?? 'overlay_start'} options={DISTANCE_REFERENCES} onchange={(v) => update('distance_reference', v)} />
        </label>
        {#if item.distance_reference === 'until_custom' || item.distance_reference === 'since_custom'}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Point ({effectiveUnit('distance', item.unit)})</span>
          {#if effectiveUnit('distance', item.unit) === 'm'}
          <Input type="number"
            value={item.distance_target != null ? Math.round(item.distance_target) : ''}
            min={0} step={1}
            oninput={(e) => app.updateElement(selected().id, { distance_target: e.target.value === '' ? undefined : Math.round(Number(e.target.value)) })} />
          {:else}
          <Input type="number" value={numVal(item, 'distance_target')} min={0} step={0.1}
            oninput={(e) => update('distance_target', e.target.value)} />
          {/if}
        </label>
        {/if}

        {:else if item.value === 'time'}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Reference</span>
          <Select
            value={item.time_reference ?? 'overlay_start'}
            options={TIME_REFERENCES}
            onchange={async (v) => {
              const s = selected()
              if (!s) return
              const updates = { time_reference: v }
              if (v === 'time_of_day' && s.item.time_timezone == null) {
                const ianaTimezone = Intl.DateTimeFormat().resolvedOptions().timeZone
                const startMs = app.gpxFilename
                  ? await backend.getActivityStartTimeMs(app.gpxFilename).catch(() => null)
                  : null
                const match = TIMEZONES.find(t => t.tz === ianaTimezone)
                updates.time_timezone = match ? ianaTimezone : null
                updates.hours_offset = getTimezoneOffsetHours(ianaTimezone, startMs)
              }
              app.updateElement(s.id, updates)
            }}
          />
        </label>
        {#if item.time_reference === 'time_of_day'}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Time zone</span>
          <Select
            value={item.time_timezone ?? ''}
            placeholder="Select time zone…"
            options={tzOptions}
            onchange={async (v) => {
              const s = selected()
              if (!s) return
              const startMs = app.gpxFilename
                ? await backend.getActivityStartTimeMs(app.gpxFilename).catch(() => null)
                : null
              const offset = getTimezoneOffsetHours(v, startMs)
              app.updateElement(s.id, { time_timezone: v, hours_offset: offset })
            }}
          />
          {#if item.time_timezone && item.hours_offset != null}
          {@const h = item.hours_offset}
          {@const sign = h >= 0 ? '+' : '−'}
          {@const ah = Math.abs(h)}
          {@const hr = Math.floor(ah)}
          {@const min = Math.round((ah - hr) * 60)}
          <p class="text-[10px] text-zinc-600">
            Offset: UTC{sign}{hr}{min > 0 ? `:${String(min).padStart(2, '0')}` : ''} at time of recording
          </p>
          {/if}
        </label>
        {/if}
        {#if item.time_reference === 'until_custom' || item.time_reference === 'since_custom'}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Point (seconds)</span>
          <Input type="number" value={numVal(item, 'time_target')} min={0} step={1}
            oninput={(e) => update('time_target', e.target.value)} />
        </label>
        {/if}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Format</span>
          <Select value={item.time_format ?? 'hh:mm:ss'} options={TIME_FORMATS} onchange={(v) => update('time_format', v)} />
        </label>
        {#if isClockFormat(item.time_format)}
        <label class="flex items-center gap-2 cursor-pointer">
          <input type="checkbox" checked={item.time_12h ?? false}
            onchange={(e) => update('time_12h', e.target.checked || undefined)} class="rounded" />
          <span class="text-xs text-zinc-400">12-hour clock</span>
        </label>
        {#if item.time_12h}
        <label class="flex items-center gap-2 cursor-pointer">
          <input type="checkbox" checked={item.time_ampm ?? false}
            onchange={(e) => update('time_ampm', e.target.checked || undefined)} class="rounded" />
          <span class="text-xs text-zinc-400">Show AM/PM</span>
        </label>
        {/if}
        {/if}

        {:else if UNITS_BY_METRIC[item.value]}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Unit</span>
          <Select value={unitValue(item.value, item.unit)} options={unitOptions(item.value)} onchange={(v) => changeUnit(v)} />
        </label>
        {/if}

        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Suffix</span>
          <Select value={suffixMode(item)} options={SUFFIX_MODES} onchange={(v) => setSuffixMode(v)} />
        </label>
        {#if suffixMode(item) === 'custom'}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Custom suffix</span>
          <Input value={item.suffix ?? ''} placeholder="e.g. kilometers" oninput={(e) => update('suffix', e.target.value || undefined)} />
        </label>
        {/if}

        {#if showAdvanced}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Decimal places</span>
          <Input type="number" value={numVal(item, 'decimal_rounding')} min={0} max={4} oninput={(e) => update('decimal_rounding', e.target.value)} />
        </label>
        <label class="flex items-center justify-between cursor-pointer group">
          <span class="text-xs text-zinc-400 group-hover:text-zinc-200 transition-colors duration-[150ms]">Fixed-width digits</span>
          <input
            type="checkbox"
            checked={item.tabular_figures ?? false}
            onchange={(e) => update('tabular_figures', e.target.checked || undefined)}
            class="h-3.5 w-3.5 rounded-sm accent-primary cursor-pointer"
          />
        </label>
        <p class="text-[10px] text-zinc-600 -mt-1">
          Keeps a changing value from jittering — every digit takes equal width, so the number and suffix stay put.
        </p>
        {/if}
      </section>

      <!-- Text: all text styling in one place -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Text</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Font</span>
          <Select value={item.font ?? ''} options={fontOpts(true)} onchange={(v) => update('font', v || undefined)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Size</span>
          <Input type="number" value={numVal(item, 'font_size')} placeholder="Scene default" oninput={(e) => update('font_size', e.target.value)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          <ColorInput value={item.color ?? '#ffffff'} vars={sceneVars} onchange={(v) => update('color', v)} />
        </label>
        {#if showAdvanced}
        <label class="flex items-center justify-between gap-3 rounded-[6px] border border-transparent bg-[var(--panel2)] px-2.5 py-2">
          <span class="text-xs text-zinc-500">Italic</span>
          <Switch checked={item.italic ?? false} ariaLabel="Italic text" onchange={(v) => update('italic', v ? true : undefined)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Alignment</span>
          <Select
            value={item.text_align ?? 'left'}
            options={[{value:'left',label:'Left'},{value:'center',label:'Center'},{value:'right',label:'Right'}]}
            onchange={(v) => update('text_align', v)}
          />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Vertical alignment</span>
          <Select
            value={item.vertical_align ?? 'baseline'}
            options={[{value:'baseline',label:'Baseline'},{value:'top',label:'Top'},{value:'middle',label:'Middle'},{value:'bottom',label:'Bottom'}]}
            onchange={(v) => update('vertical_align', v === 'baseline' ? undefined : v)}
          />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.opacity ?? app.config?.scene?.opacity ?? 1} oninput={(e) => update('opacity', e.target.value)} />
        </label>
        {/if}
      </section>
    {/if}

    <!-- ═══ PLOT ═══ -->
    {#if type === 'plot'}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Metric</p>
        <Select
          value={item.value ?? ''}
          options={PLOT_METRICS.map((m) => ({ value: m, label: m === 'course' ? 'course (map)' : m }))}
          onchange={(v) => update('value', v)}
        />
      </section>

      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Appearance</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          {#if showAdvanced}
          <ColorInput value={colorRow('line', 'color')} vars={sceneVars} onchange={(v) => updateNested('line', 'color', v)} />
          {:else}
          <ColorInput value={primaryColor()} vars={sceneVars} onchange={(v) => setAllColors(v)} />
          {/if}
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Line width (px)</span>
          <Input type="number" value={item.line?.width ?? 1.75} min={0} step={0.25}
            oninput={(e) => updateNested('line', 'width', e.target.value)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Point size (area px²)</span>
          <Input type="number" value={item.point?.weight ?? 80} min={4} step={4}
            oninput={(e) => updatePoint('weight', e.target.value)} />
        </label>
        {#if showAdvanced}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.opacity ?? 1} oninput={(e) => update('opacity', e.target.value)} />
        </label>
        {/if}
      </section>

      <!-- Color by: band-color segments by a second metric (TdF-style profiles) -->
      {@const cb = item.color_by}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Color by Metric</p>
        <label class="flex items-center gap-2 cursor-pointer">
          <input type="checkbox" checked={cb != null}
            onchange={(e) => toggleColorBy(e.target.checked)}
            class="accent-primary" />
          <span class="text-xs text-zinc-400">Color segments by a second metric</span>
        </label>
        {#if cb != null}
          <label class="space-y-1 block">
            <span class="text-xs text-zinc-500">Metric</span>
            <Select
              value={cb.value ?? 'gradient'}
              options={COLOR_BY_METRICS.map((m) => ({ value: m, label: metricLabel(m) }))}
              onchange={setColorByMetric}
            />
          </label>
          {#if item.value === 'course'}
            <p class="text-[10px] text-zinc-600 italic">Colors the route line.</p>
          {:else}
            <label class="space-y-1 block">
              <span class="text-xs text-zinc-500">Apply to</span>
              <Select value={cb.mode ?? 'fill'} options={COLOR_BY_MODES}
                onchange={(v) => setColorByField('mode', v)} />
            </label>
          {/if}
          {#if COLOR_BY_UNITS[cb.value ?? 'gradient']}
            <label class="space-y-1 block">
              <span class="text-xs text-zinc-500">Threshold unit</span>
              <Select value={cb.unit ?? ''} options={COLOR_BY_UNITS[cb.value ?? 'gradient']}
                onchange={(v) => setColorByField('unit', v || undefined)} />
            </label>
          {/if}
          <div class="space-y-1">
            <div class="flex items-center justify-between">
              <span class="text-xs text-zinc-500">Bands — up to ({colorByUnitHint(cb)})</span>
              <button type="button" class="cursor-pointer text-xs text-primary hover:underline"
                onclick={addColorBand}>+ band</button>
            </div>
            {#each colorByBands() as band, i (i)}
              <div class="flex gap-2 items-center">
                {#if band.max != null}
                  <Input type="number" value={band.max} step={1} class="w-20 shrink-0"
                    oninput={(e) => { if (e.target.value !== '') updateColorBand(i, { max: Number(e.target.value) }) }} />
                {:else}
                  <span class="w-20 shrink-0 text-center text-xs text-zinc-500">above</span>
                {/if}
                <ColorInput value={band.color ?? '#ffffff'} vars={sceneVars}
                  onchange={(v) => updateColorBand(i, { color: v })} class="flex-1 min-w-0" />
                <button type="button" class="cursor-pointer text-xs text-zinc-500 hover:text-red-400 px-1 shrink-0"
                  onclick={() => removeColorBand(i)} aria-label="Remove band">✕</button>
              </div>
            {/each}
          </div>
        {/if}
      </section>

      {#if showAdvanced}
      {#if item.value === 'elevation'}
      <!-- X-axis basis: time (sample index) vs distance travelled -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">X-Axis</p>
        <label class="flex items-center justify-between gap-3 rounded-[6px] border border-zinc-800 bg-zinc-900/40 px-2.5 py-2">
          <span class="text-xs text-zinc-500">Distance-based</span>
          <Switch
            checked={item.x_axis === 'distance'}
            ariaLabel="Distance-based x-axis"
            onchange={(v) => update('x_axis', v ? 'distance' : undefined)}
          />
        </label>
        <p class="text-[10px] text-zinc-600 italic">
          Off: evenly spaced by time. On: horizontal position reflects distance travelled.
        </p>
      </section>
      {/if}
      <!-- Fill -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Fill</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          <ColorInput value={colorRow('fill', 'color')} vars={sceneVars} onchange={(v) => updateNested('fill', 'color', v)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.fill?.opacity ?? 0} oninput={(e) => updateNested('fill', 'opacity', e.target.value)} />
        </label>
      </section>

      {#if item.value === 'course'}
      <!-- Course map: traveled / ahead opacity -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Visibility</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Traveled opacity (0–1)</span>
          <OpacityControl value={item.line?.past_opacity ?? 1}
            oninput={(e) => updateNested('line', 'past_opacity', e.target.value)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Ahead opacity (0–1)</span>
          <OpacityControl value={item.line?.future_opacity ?? 1}
            oninput={(e) => updateNested('line', 'future_opacity', e.target.value)} />
        </label>
      </section>
      {/if}

      <!-- Tracking point detail -->
      {@const pt = item.point ?? {}}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Tracking Point</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          <ColorInput value={pt.color ?? '#ffffff'} vars={sceneVars} onchange={(v) => updatePoint('color', v)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Edge color</span>
          <ColorInput value={pt.edge_color ?? '#000000'} vars={sceneVars} onchange={(v) => updatePoint('edge_color', v)} />
        </label>
        {#if !(pt.remove_edge_color ?? false)}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Edge width (px)</span>
          <Input type="number" value={pt.edge_width ?? 1} min={0} step={0.5}
            oninput={(e) => updatePoint('edge_width', e.target.value)} />
        </label>
        {/if}
        <label class="flex items-center gap-2 cursor-pointer">
          <input type="checkbox" checked={pt.remove_edge_color ?? false}
            onchange={(e) => updatePoint('remove_edge_color', e.target.checked)}
            class="accent-primary" />
          <span class="text-xs text-zinc-400">Remove edge</span>
        </label>
      </section>

      <!-- Point label -->
      {@const pl = item.point_label}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Point Label</p>
        <label class="flex items-center gap-2 cursor-pointer">
          <input type="checkbox" checked={pl != null}
            onchange={(e) => togglePointLabel(e.target.checked)}
            class="accent-primary" />
          <span class="text-xs text-zinc-400">Show value at marker</span>
        </label>
        {#if pl != null}
          <div class="flex gap-4">
            <label class="flex items-center gap-2 cursor-pointer">
              <input type="checkbox" checked={(pl.units ?? []).includes('metric')}
                onchange={(e) => toggleUnit('metric', e.target.checked)} class="accent-primary" />
              <span class="text-xs text-zinc-400">Metric</span>
            </label>
            <label class="flex items-center gap-2 cursor-pointer">
              <input type="checkbox" checked={(pl.units ?? []).includes('imperial')}
                onchange={(e) => toggleUnit('imperial', e.target.checked)} class="accent-primary" />
              <span class="text-xs text-zinc-400">Imperial</span>
            </label>
          </div>
          <label class="space-y-1 block">
            <span class="text-xs text-zinc-500">Font</span>
            <Select value={pl.font ?? 'Furore.otf'} options={fontOpts(false)} onchange={(v) => updatePL('font', v)} />
          </label>
          <label class="space-y-1 block">
            <span class="text-xs text-zinc-500">Size</span>
            <Input type="number" value={pl.font_size ?? 64} min={1} oninput={(e) => updatePL('font_size', e.target.value)} />
          </label>
          <label class="flex items-center justify-between gap-3 rounded-[6px] border border-transparent bg-[var(--panel2)] px-2.5 py-2">
            <span class="text-xs text-zinc-500">Italic</span>
            <Switch checked={pl.italic ?? false} ariaLabel="Italic point label" onchange={(v) => updatePL('italic', v ? true : undefined)} />
          </label>
          <label class="space-y-1 block">
            <span class="text-xs text-zinc-500">Color</span>
            <ColorInput value={pl.color ?? '#ffffffc8'} vars={sceneVars} onchange={(v) => updatePL('color', v)} />
          </label>
          <div class="grid grid-cols-2 gap-2">
            <label class="space-y-1">
              <span class="text-xs text-zinc-500">X offset</span>
              <Input type="number" value={pl.x_offset ?? 0} oninput={(e) => updatePL('x_offset', e.target.value)} />
            </label>
            <label class="space-y-1">
              <span class="text-xs text-zinc-500">Y offset</span>
              <Input type="number" value={pl.y_offset ?? 0} oninput={(e) => updatePL('y_offset', e.target.value)} />
            </label>
          </div>
          <label class="space-y-1 block">
            <span class="text-xs text-zinc-500">Decimal places</span>
            <Input type="number" value={pl.decimal_rounding ?? 0} min={0} step={1}
              oninput={(e) => updatePL('decimal_rounding', e.target.value)} />
          </label>
        {/if}
      </section>

      <!-- Size + rotation -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Size</p>
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Width</span>
            <Input type="number" step="1" value={numVal(item, 'width')} oninput={(e) => update('width', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Height</span>
            <Input type="number" step="1" value={numVal(item, 'height')} oninput={(e) => update('height', e.target.value)} />
          </label>
        </div>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Rotation (°)</span>
          <Input type="number" value={item.rotation ?? 0} min={-180} max={180} step={1}
            oninput={(e) => update('rotation', e.target.value)} />
        </label>
      </section>
      {/if}

      <!-- Course Markers — always visible when metric is course -->
      {#if item.value === 'course'}
      <section class="mb-4 space-y-2">
        <div class="flex items-center justify-between">
          <p class="text-[10px] uppercase tracking-wider text-zinc-600">Course Markers</p>
          <button type="button" class="cursor-pointer text-xs text-primary hover:underline" onclick={addCourseMarker}>+ marker</button>
        </div>
        {#if courseMarkers().length === 0}
          <p class="text-[10px] text-zinc-600 italic">No markers.</p>
        {:else}
          <div class="flex flex-wrap gap-1.5">
            {#each courseMarkers() as marker, i (marker.id ?? i)}
              <button
                type="button"
                class="cursor-pointer rounded-[6px] border px-2 py-1 text-[11px] transition-colors
                  {(selectedCourseMarker()?.id ?? courseMarkers()[0]?.id) === marker.id
                    ? 'border-primary bg-primary/10 text-zinc-100'
                    : 'border-transparent bg-[var(--panel2)] text-zinc-400 hover:bg-[var(--panel3)] hover:text-zinc-200'}"
                onclick={() => (app.selectedCourseMarkerId = marker.id)}
              >
                {marker.name || `Marker ${i + 1}`}
              </button>
            {/each}
          </div>
          {@const marker = selectedCourseMarker()}
          {#if marker}
            <label class="space-y-1 block">
              <span class="text-xs text-zinc-500">Name</span>
              <Input value={marker.name ?? ''} oninput={(e) => updateCourseMarker('name', e.target.value)} />
            </label>
            <label class="space-y-1 block">
              <span class="text-xs text-zinc-500">Style</span>
              <Select value={marker.style ?? 'checkered'} options={COURSE_MARKER_STYLES} onchange={(v) => updateCourseMarker('style', v)} />
            </label>
            <label class="space-y-1 block">
              <span class="text-xs text-zinc-500">Distance from start (m)</span>
              <Input type="number" value={marker.distance ?? 0} min={0} step={10}
                oninput={(e) => updateCourseMarker('distance', e.target.value)} />
            </label>
            {#if (marker.style ?? 'checkered') !== 'checkered'}
            <label class="space-y-1 block">
              <span class="text-xs text-zinc-500">Color</span>
              <ColorInput value={marker.color ?? '#ef4444'} vars={sceneVars} onchange={(v) => updateCourseMarker('color', v)} />
            </label>
            {/if}
            <div class="grid grid-cols-2 gap-2">
              <label class="space-y-1">
                <span class="text-xs text-zinc-500">Width (px)</span>
                <Input type="number" value={marker.width ?? 34} min={1} step={1}
                  oninput={(e) => updateCourseMarker('width', e.target.value)} />
              </label>
              <label class="space-y-1">
                <span class="text-xs text-zinc-500">Height (px)</span>
                <Input type="number" value={marker.height ?? 10} min={1} step={1}
                  oninput={(e) => updateCourseMarker('height', e.target.value)} />
              </label>
            </div>
            <div class="grid grid-cols-2 gap-2">
              <label class="space-y-1">
                <span class="text-xs text-zinc-500">Rotation (°)</span>
                <Input type="number" value={marker.rotation ?? 0} step={1}
                  oninput={(e) => updateCourseMarker('rotation', e.target.value)} />
              </label>
              <label class="space-y-1">
                <span class="text-xs text-zinc-500">Opacity (0–1)</span>
                <OpacityControl value={marker.opacity ?? 1}
                  oninput={(e) => updateCourseMarker('opacity', e.target.value)} />
              </label>
            </div>
            <button type="button" class="cursor-pointer text-xs text-zinc-500 hover:text-red-400"
              onclick={() => removeCourseMarker(marker.id)}>Remove marker</button>
          {/if}
        {/if}
      </section>
      {/if}
    {/if}

    <!-- ═══ METER ═══ -->
    {#if type === 'meter'}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Metric</p>
        <Select
          value={item.value ?? ''}
          options={METER_METRICS.map((m) => ({ value: m, label: metricLabel(m) }))}
          onchange={(v) => update('value', v)}
        />
        {#if UNITS_BY_METRIC[item.value]}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Unit</span>
          <Select value={unitValue(item.value, item.unit)} options={unitOptions(item.value)} onchange={(v) => changeUnit(v)} />
        </label>
        {/if}
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Min</span>
            <Tooltip content={rangeBoundTooltip(item, 'min')} side="bottom" class="w-full">
              <Input value={numVal(item, 'min')} placeholder="min / max / #" onchange={(e) => update('min', e.target.value)} />
            </Tooltip>
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Max</span>
            <Tooltip content={rangeBoundTooltip(item, 'max')} side="bottom" class="w-full">
              <Input value={numVal(item, 'max')} placeholder="min / max / #" onchange={(e) => update('max', e.target.value)} />
            </Tooltip>
          </label>
        </div>
        {#if rangeWarning(item)}
          <div class="flex items-start gap-1.5 rounded-[6px] border border-amber-500/30 bg-amber-500/10 px-2 py-1.5 text-[11px] leading-snug text-amber-300">
            <AlertTriangle size={12} class="mt-0.5 shrink-0" />
            <span>{rangeWarning(item)}</span>
          </div>
        {/if}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Direction</span>
          <Select value={item.direction ?? 'up'} options={METER_DIRECTIONS} onchange={(v) => update('direction', v)} />
        </label>
        {#if showAdvanced}
        <button
          type="button"
          onclick={applyMeterActivityRange}
          class="w-full cursor-pointer rounded-[6px] border-0 bg-[var(--panel2)] px-2.5 py-1.5 text-xs font-medium text-zinc-300 transition-colors hover:bg-[var(--panel3)] hover:text-zinc-100"
        >
          Set min/max from activity
        </button>
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Segments</span>
            <Input type="number" value={item.segments ?? ''} min={0} step={1} placeholder="off"
              oninput={(e) => update('segments', e.target.value)} />
          </label>
          {#if (item.segments ?? 0) >= 1}
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Gap (px)</span>
            <Input type="number" value={item.gap ?? 0} min={0} step={1}
              oninput={(e) => update('gap', e.target.value)} />
          </label>
          {/if}
        </div>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Corner radius (px)</span>
          <Input type="number" value={item.radius ?? 0} min={0} step={1}
            oninput={(e) => update('radius', e.target.value)} />
        </label>
        {/if}
      </section>

      <!-- Fill: adapts to segmented vs continuous -->
      {#if (item.segments ?? 0) >= 1}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Fill</p>
        <div class="flex items-center justify-between">
          <span class="text-xs text-zinc-500">Gradient (min → max)</span>
          <button type="button" class="cursor-pointer text-xs text-primary hover:underline"
            onclick={addGradientStop}>+ stop</button>
        </div>
        {#if meterGradient().length === 0}
          <label class="space-y-1 block">
            <span class="text-xs text-zinc-500">Color</span>
            <ColorInput value={item.color ?? '#ffffff'} vars={sceneVars} onchange={(v) => update('color', v)} />
          </label>
        {/if}
        {#each meterGradient() as stop, i (i)}
          <div class="flex gap-2 items-center">
            <ColorInput value={stop ?? '#ffffff'} vars={sceneVars}
              onchange={(v) => updateGradientStop(i, v)} class="flex-1 min-w-0" />
            <button type="button" class="cursor-pointer text-xs text-zinc-500 hover:text-red-400 px-1 shrink-0"
              onclick={() => removeGradientStop(i)} aria-label="Remove stop">✕</button>
          </div>
        {/each}
      </section>
      {:else}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Fill</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Low value</span>
          <ColorInput value={meterGradient()[0] ?? item.color ?? '#ffffff'} vars={sceneVars}
            onchange={(v) => updateContinuousGradientStop(0, v)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">High value</span>
          <ColorInput value={meterGradient()[1] ?? item.color ?? '#ffffff'} vars={sceneVars}
            onchange={(v) => updateContinuousGradientStop(1, v)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.fill_opacity ?? item.opacity ?? 1}
            oninput={(e) => update('fill_opacity', e.target.value)} />
        </label>
      </section>
      {/if}

      <!-- Background (track) -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Background</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          <ColorInput value={item.background ?? ''} vars={sceneVars} placeholder="none"
            onchange={(v) => update('background', v || undefined)} />
        </label>
        {#if item.background}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.background_opacity ?? 1}
            oninput={(e) => update('background_opacity', e.target.value)} />
        </label>
        {/if}
      </section>

      <!-- Border -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Border</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          <ColorInput value={item.border_color ?? ''} vars={sceneVars} placeholder="none"
            onchange={(v) => update('border_color', v || undefined)} />
        </label>
        {#if item.border_color}
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Width (px)</span>
            <Input type="number" value={item.border_width ?? 2} min={0.5} step={0.5}
              oninput={(e) => update('border_width', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Opacity (0–1)</span>
            <OpacityControl value={item.border_opacity ?? item.opacity ?? 1}
              oninput={(e) => update('border_opacity', e.target.value)} />
          </label>
        </div>
        {/if}
      </section>

      <!-- Scale (advanced) -->
      {#if showAdvanced}
      <section class="mb-4 space-y-2">
        <div class="flex items-center justify-between">
          <p class="text-[10px] uppercase tracking-wider text-zinc-600">Scale</p>
          {#if scaleEnabled()}
            <button type="button" class="cursor-pointer text-xs text-zinc-500 hover:text-red-400 transition-colors"
              onclick={disableScale}>Remove</button>
          {:else}
            <button type="button" class="cursor-pointer text-xs text-primary hover:underline"
              onclick={enableScale}>+ Enable</button>
          {/if}
        </div>
        {#if scaleEnabled()}
          <div class="space-y-1">
            <div class="flex items-center justify-between">
              <span class="text-xs text-zinc-500">Labels (empty = auto)</span>
              <button type="button" class="cursor-pointer text-xs text-primary hover:underline"
                onclick={addScaleLabel}>+ add</button>
            </div>
            {#if (scaleLabels() ?? []).length === 0}
              <p class="text-[10px] text-zinc-600 italic">Auto: min, mid, max</p>
            {/if}
            {#each (scaleLabels() ?? []) as val, i (i)}
              <div class="flex gap-2 items-center">
                <Input type="number" value={val} step="any"
                  oninput={(e) => updateScaleLabel(i, e.target.value)}
                  class="flex-1 font-mono text-xs" />
                <button type="button" class="cursor-pointer text-xs text-zinc-500 hover:text-red-400 px-1"
                  onclick={() => removeScaleLabel(i)} aria-label="Remove">✕</button>
              </div>
            {/each}
          </div>
          <label class="space-y-1 block">
            <span class="text-xs text-zinc-500">Label color</span>
            <ColorInput value={item.scale_color ?? ''} vars={sceneVars} placeholder="fill color"
              onchange={(v) => update('scale_color', v || undefined)} />
          </label>
          <label class="space-y-1 block">
            <span class="text-xs text-zinc-500">Suffix</span>
            <Input value={item.scale_suffix ?? ''} placeholder="e.g. mph"
              oninput={(e) => update('scale_suffix', e.target.value || undefined)} />
          </label>
          <label class="space-y-1 block">
            <span class="text-xs text-zinc-500">Font</span>
            <Select value={item.scale_font ?? ''} options={fontOpts(true)} onchange={(v) => update('scale_font', v || undefined)} />
          </label>
          <div class="grid grid-cols-2 gap-2">
            <label class="space-y-1">
              <span class="text-xs text-zinc-500">Font size (px)</span>
              <Input type="number" value={item.scale_font_size ?? 20} min={6} step={1}
                oninput={(e) => update('scale_font_size', e.target.value)} />
            </label>
            <label class="space-y-1">
              <span class="text-xs text-zinc-500">Offset (px)</span>
              <Input type="number" value={item.scale_offset ?? 8} min={0} step={1}
                oninput={(e) => update('scale_offset', e.target.value)} />
            </label>
            <label class="space-y-1">
              <span class="text-xs text-zinc-500">Tick ext. (px)</span>
              <Input type="number" value={item.scale_tick_length ?? 6} min={0} step={1}
                oninput={(e) => update('scale_tick_length', e.target.value)} />
            </label>
            <label class="space-y-1">
              <span class="text-xs text-zinc-500">Tick width (px)</span>
              <Input type="number" value={item.scale_tick_width ?? 1} min={0} step={0.5}
                oninput={(e) => update('scale_tick_width', e.target.value)} />
            </label>
            <label class="space-y-1">
              <span class="text-xs text-zinc-500">Extra ticks</span>
              <Input type="number" value={item.scale_ticks ?? 0} min={0} step={1}
                oninput={(e) => update('scale_ticks', e.target.value || undefined)} />
            </label>
          </div>
        {/if}
      </section>

      <!-- Size + rotation (advanced) -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Size</p>
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Width</span>
            <Input type="number" step="1" value={numVal(item, 'width')} oninput={(e) => update('width', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Height</span>
            <Input type="number" step="1" value={numVal(item, 'height')} oninput={(e) => update('height', e.target.value)} />
          </label>
        </div>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Rotation (°)</span>
          <Input type="number" value={item.rotation ?? 0} min={-180} max={180} step={1}
            oninput={(e) => update('rotation', e.target.value)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.opacity ?? 1} oninput={(e) => update('opacity', e.target.value)} />
        </label>
      </section>
      {/if}
    {/if}

    <!-- ═══ GAUGE ═══ -->
    {#if type === 'gauge'}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Metric</p>
        <Select
          value={item.value ?? ''}
          options={METER_METRICS.map((m) => ({ value: m, label: metricLabel(m) }))}
          onchange={(v) => update('value', v)}
        />
        {#if UNITS_BY_METRIC[item.value]}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Unit</span>
          <Select value={unitValue(item.value, item.unit)} options={unitOptions(item.value)} onchange={(v) => changeUnit(v)} />
        </label>
        {/if}
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Min</span>
            <Tooltip content={rangeBoundTooltip(item, 'min')} side="bottom" class="w-full">
              <Input value={numVal(item, 'min')} placeholder="min / max / #" onchange={(e) => update('min', e.target.value)} />
            </Tooltip>
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Max</span>
            <Tooltip content={rangeBoundTooltip(item, 'max')} side="bottom" class="w-full">
              <Input value={numVal(item, 'max')} placeholder="min / max / #" onchange={(e) => update('max', e.target.value)} />
            </Tooltip>
          </label>
        </div>
        {#if rangeWarning(item)}
          <div class="flex items-start gap-1.5 rounded-[6px] border border-amber-500/30 bg-amber-500/10 px-2 py-1.5 text-[11px] leading-snug text-amber-300">
            <AlertTriangle size={12} class="mt-0.5 shrink-0" />
            <span>{rangeWarning(item)}</span>
          </div>
        {/if}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          <ColorInput value={item.color ?? '#ffffff'} vars={sceneVars} onchange={(v) => update('color', v)} />
        </label>
      </section>

      <!-- Dial geometry + colors (advanced) -->
      {#if showAdvanced}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Dial</p>
        <button
          type="button"
          onclick={applyGaugeActivityRange}
          class="w-full cursor-pointer rounded-[6px] border-0 bg-[var(--panel2)] px-2.5 py-1.5 text-xs font-medium text-zinc-300 transition-colors hover:bg-[var(--panel3)] hover:text-zinc-100"
        >
          Set min/max from activity
        </button>
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Start angle (°)</span>
            <Input type="number" value={item.start_angle ?? 135} step={1}
              oninput={(e) => update('start_angle', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Sweep (°)</span>
            <Input type="number" value={item.sweep_angle ?? 270} step={1}
              oninput={(e) => update('sweep_angle', e.target.value)} />
          </label>
        </div>
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Arc width (px)</span>
            <Input type="number" value={item.arc_width ?? 14} min={0} step={1}
              oninput={(e) => update('arc_width', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Needle width (px)</span>
            <Input type="number" value={item.needle_width ?? 6} min={0} step={1}
              oninput={(e) => update('needle_width', e.target.value)} />
          </label>
        </div>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Track color</span>
          <ColorInput value={item.arc_color ?? ''} vars={sceneVars} placeholder="none"
            onchange={(v) => update('arc_color', v || undefined)} />
        </label>
        <div class="space-y-1">
          <div class="flex items-center justify-between">
            <span class="text-xs text-zinc-500">Progress gradient (start → end)</span>
            <button type="button" class="cursor-pointer text-xs text-primary hover:underline"
              onclick={addGradientStop}>+ stop</button>
          </div>
          {#if meterGradient().length === 0}
            <p class="text-[10px] text-zinc-600 italic">No stops — uses progress color.</p>
          {/if}
          {#each meterGradient() as stop, i (i)}
            <div class="flex gap-2 items-center">
              <ColorInput value={stop ?? '#ffffff'} vars={sceneVars}
                onchange={(v) => updateGradientStop(i, v)} class="flex-1 min-w-0" />
              <button type="button" class="cursor-pointer text-xs text-zinc-500 hover:text-red-400 px-1 shrink-0"
                onclick={() => removeGradientStop(i)} aria-label="Remove stop">✕</button>
            </div>
          {/each}
        </div>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Progress color</span>
          <ColorInput value={item.progress_color ?? ''} vars={sceneVars} placeholder="none"
            onchange={(v) => update('progress_color', v || undefined)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Needle color</span>
          <ColorInput value={item.needle_color ?? ''} vars={sceneVars} placeholder="base color"
            onchange={(v) => update('needle_color', v || undefined)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Cap dot color</span>
          <ColorInput value={item.cap_color ?? ''} vars={sceneVars} placeholder="none"
            onchange={(v) => update('cap_color', v || undefined)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Cap radius (px)</span>
          <Input type="number" value={item.cap_radius ?? ''} min={0} step={1} placeholder="auto"
            oninput={(e) => update('cap_radius', e.target.value)} />
        </label>
        <label class="flex items-center gap-2 cursor-pointer">
          <input type="checkbox" checked={item.hide_track ?? false}
            onchange={(e) => update('hide_track', e.target.checked || undefined)} class="rounded" />
          <span class="text-xs text-zinc-400">Hide unfilled arc</span>
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Background color</span>
          <ColorInput value={item.background_color ?? ''} vars={sceneVars} placeholder="none"
            onchange={(v) => update('background_color', v || undefined)} />
        </label>
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Background opacity</span>
            <OpacityControl value={item.background_opacity ?? 0}
              oninput={(e) => update('background_opacity', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Margin (px)</span>
            <Input type="number" value={numVal(item, 'background_margin')} min={0} step={4}
              oninput={(e) => update('background_margin', e.target.value || undefined)} />
          </label>
        </div>
      </section>

      <!-- Size + rotation (advanced) -->
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Size</p>
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Width</span>
            <Input type="number" step="1" value={numVal(item, 'width')} oninput={(e) => update('width', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Height</span>
            <Input type="number" step="1" value={numVal(item, 'height')} oninput={(e) => update('height', e.target.value)} />
          </label>
        </div>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Rotation (°)</span>
          <Input type="number" value={item.rotation ?? 0} min={-180} max={180} step={1}
            oninput={(e) => update('rotation', e.target.value)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.opacity ?? 1} oninput={(e) => update('opacity', e.target.value)} />
        </label>
      </section>
      {/if}
    {/if}

    <!-- ═══ RECT ═══ -->
    {#if type === 'rect'}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Size</p>
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Width</span>
            <Input type="number" step="1" value={numVal(item, 'width')} oninput={(e) => update('width', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Height</span>
            <Input type="number" step="1" value={numVal(item, 'height')} oninput={(e) => update('height', e.target.value)} />
          </label>
        </div>
        {#if showAdvanced}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Rotation (°)</span>
          <Input type="number" value={item.rotation ?? 0} min={-180} max={180} step={1}
            oninput={(e) => update('rotation', e.target.value)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Corner radius (px)</span>
          <Input type="number" value={item.radius ?? 0} min={0} step={1}
            oninput={(e) => update('radius', e.target.value)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Element opacity (0–1)</span>
          <OpacityControl value={item.opacity ?? 1} oninput={(e) => update('opacity', e.target.value)} />
        </label>
        {/if}
      </section>

      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Fill</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          <ColorInput value={item.color ?? '#ffffff'} vars={sceneVars} onchange={(v) => update('color', v)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.fill_opacity ?? item.opacity ?? 1}
            oninput={(e) => update('fill_opacity', e.target.value)} />
        </label>
      </section>

      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Border</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Color</span>
          <ColorInput value={item.border_color ?? ''} vars={sceneVars} placeholder="none"
            onchange={(v) => update('border_color', v || undefined)} />
        </label>
        {#if item.border_color}
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Width (px)</span>
            <Input type="number" value={item.border_width ?? 2} min={0.5} step={0.5}
              oninput={(e) => update('border_width', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Opacity (0–1)</span>
            <OpacityControl value={item.border_opacity ?? item.opacity ?? 1}
              oninput={(e) => update('border_opacity', e.target.value)} />
          </label>
        </div>
        {/if}
      </section>
    {/if}

    <!-- ═══ IMAGE ═══ -->
    {#if type === 'image'}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Asset</p>
        <div class="flex items-center gap-2">
          <span class="flex-1 truncate text-xs font-mono {item.file ? 'text-zinc-300' : 'text-zinc-600 italic'}">
            {item.file || 'None selected'}
          </span>
          <button
            onclick={() => (showAssetPicker = true)}
            class="cursor-pointer shrink-0 flex items-center gap-1.5 px-2.5 py-1.5 rounded-[6px] text-xs font-medium
                   bg-[var(--panel2)] text-zinc-300 hover:bg-[var(--panel3)] hover:text-zinc-100 transition-colors"
          >
            <FolderOpen size={11} />
            Browse
          </button>
        </div>
      </section>

      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Size</p>
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Width</span>
            <Input type="number" step="1" value={numVal(item, 'width')}
              oninput={(e) => updateImageSize('width', e.target.value)} />
          </label>
          <label class="space-y-1">
            <span class="text-xs text-zinc-500">Height</span>
            <Input type="number" step="1" value={numVal(item, 'height')}
              oninput={(e) => updateImageSize('height', e.target.value)} />
          </label>
        </div>
        {#if showAdvanced}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Rotation (°)</span>
          <Input type="number" value={item.rotation ?? 0} min={-180} max={180} step={1}
            oninput={(e) => update('rotation', e.target.value)} />
        </label>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Opacity (0–1)</span>
          <OpacityControl value={item.opacity ?? 1} oninput={(e) => update('opacity', e.target.value)} />
        </label>
        {/if}
      </section>

      {#if showAdvanced}
      <section class="mb-4 space-y-2">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Pulse animation</p>
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">BPM source</span>
          <Select
            value={item.pulse_metric ?? ''}
            options={[{value:'',label:'Fixed BPM'},{value:'heartrate',label:'Heart rate (live)'}]}
            onchange={(v) => update('pulse_metric', v || undefined)}
          />
        </label>
        {#if !item.pulse_metric}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">BPM</span>
          <Input type="number" value={numVal(item, 'pulse_bpm')} min={0} max={300} step={1}
            placeholder="0 = off"
            oninput={(e) => update('pulse_bpm', e.target.value || undefined)} />
        </label>
        {/if}
        <label class="space-y-1 block">
          <span class="text-xs text-zinc-500">Amplitude (0–1)</span>
          <Input type="number" value={item.pulse_amplitude ?? 0.15} min={0} max={1} step={0.05}
            oninput={(e) => update('pulse_amplitude', e.target.value)} />
        </label>
      </section>
      {/if}

      {#if showAssetPicker}
        <AssetPicker
          current={selected()?.item.file ?? ''}
          onselect={(name) => { applyAsset(name); showAssetPicker = false }}
          oncancel={() => (showAssetPicker = false)}
        />
      {/if}
    {/if}

    <!-- Anchor — pin this element to a point on another element's box -->
    {#if ANCHORABLE_TYPES.includes(type)}
    <section class="mb-4 space-y-2">
      <p class="text-[10px] uppercase tracking-wider text-zinc-600">Anchor</p>
      <label class="space-y-1 block">
        <span class="text-xs text-zinc-500">Anchor to</span>
        <Select
          value={item.anchor?.target ?? ''}
          options={[{ value: '', label: 'None' }, ...anchorTargetOpts()]}
          onchange={(v) => setAnchorTarget(v)}
        />
      </label>
      {#if item.anchor}
      <label class="space-y-1 block">
        <span class="text-xs text-zinc-500">Point</span>
        <Select value={item.anchor.point ?? 'center'} options={ANCHOR_POINTS} onchange={(v) => setAnchorPoint(v)} />
      </label>
      <div class="grid grid-cols-2 gap-2">
        <label class="space-y-1">
          <span class="text-xs text-zinc-500">Offset X</span>
          <Input type="number" step="1" value={item.anchor.offset_x ?? 0} oninput={(e) => setAnchorOffset('offset_x', e.target.value)} />
        </label>
        <label class="space-y-1">
          <span class="text-xs text-zinc-500">Offset Y</span>
          <Input type="number" step="1" value={item.anchor.offset_y ?? 0} oninput={(e) => setAnchorOffset('offset_y', e.target.value)} />
        </label>
      </div>
      <p class="text-[10px] text-zinc-600 italic">Follows {item.anchor.target} — dragging on the canvas adjusts the offset.</p>
      {/if}
    </section>
    {/if}

    <!-- Position — advanced for all types; click-and-drag is the primary interaction -->
    {#if showAdvanced}
    <section class="mb-4 space-y-2">
      <div class="flex items-center justify-between">
        <p class="text-[10px] uppercase tracking-wider text-zinc-600">Position</p>
        <button
          onclick={() => app.updateElement(id, { locked: !item.locked })}
          title={item.locked ? 'Unlock position' : 'Lock position'}
          class="cursor-pointer p-1 rounded transition-colors {item.locked ? 'text-amber-400 hover:text-amber-300' : 'text-zinc-600 hover:text-zinc-300'}"
        >
          {#if item.locked}
            <Lock size={13} />
          {:else}
            <LockOpen size={13} />
          {/if}
        </button>
      </div>
      {#if item.anchor}
      <p class="text-[10px] text-zinc-600 italic">Position is derived from the anchor — use its offset to fine-tune.</p>
      {:else}
      <div class="grid grid-cols-2 gap-2">
        <label class="space-y-1">
          <span class="text-xs text-zinc-500">X</span>
          <Input type="number" step="1" value={numVal(item, 'x')} oninput={(e) => update('x', e.target.value)} />
        </label>
        <label class="space-y-1">
          <span class="text-xs text-zinc-500">Y</span>
          <Input type="number" step="1" value={numVal(item, 'y')} oninput={(e) => update('y', e.target.value)} />
        </label>
      </div>
      {/if}
    </section>
    {/if}

  {/if}
</div>
