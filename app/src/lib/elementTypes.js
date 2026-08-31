/**
 * Element-type registry. The single place that knows about element kinds.
 *
 * Adding a new graphic = one new ADD_PRESETS entry + a branch in
 * `elementMeta`/`elementTypeName` here, plus its Rust `OverlayElement` impl.
 * No edits scattered across appState / list / inspector.
 *
 * Element `type` tokens match the Rust serde tag exactly: 'label' | 'value'
 * | 'plot'. Icon tokens are resolved to components by the consuming view.
 */

// ─── Sub-config types ─────────────────────────────────────────────────────────

/**
 * @typedef {Object} LineConfig
 * @property {number} [width]
 * @property {string} [color]
 * @property {number} [past_opacity]
 * @property {number} [future_opacity]
 */

/**
 * @typedef {Object} FillConfig
 * @property {number} [opacity]
 * @property {string} [color]
 */

/**
 * @typedef {Object} PointConfig
 * @property {string} [color]
 * @property {number} [weight]
 * @property {number} [opacity]
 * @property {string} [edge_color]
 * @property {number} [edge_width]
 * @property {boolean} [remove_edge_color]
 */

/**
 * @typedef {Object} CourseMarkerConfig
 * @property {string} [id]
 * @property {string} [name]
 * @property {number} [distance]
 * @property {'checkered'|'circle'|'rectangle'} [style]
 * @property {string} [color]
 * @property {number} [width]
 * @property {number} [height]
 * @property {number} [rotation]
 * @property {number} [opacity]
 */

/**
 * @typedef {Object} PointLabelConfig
 * @property {number} [font_size]
 * @property {string} [color]
 * @property {string} [font]
 * @property {boolean} [italic]
 * @property {number} [x_offset]
 * @property {number} [y_offset]
 * @property {string[]} [units]
 * @property {number} [decimal_rounding]
 */

// ─── Scene / Template ─────────────────────────────────────────────────────────

/**
 * @typedef {Object} GroupConfig
 * @property {string} id
 * @property {string} name
 * @property {string[]} element_ids
 */

/**
 * Start/finish line for lap counting (crits), set from the dual race-playhead
 * bar. `start` = ride seconds of the race start (the rider is on the line at
 * that moment — that GPS position becomes the gate); `end` = ride seconds of
 * the race finish, bounding lap counting so cooldown crossings don't count.
 * One gate per scene.
 * @typedef {Object} LapGateConfig
 * @property {number} start
 * @property {number} [end] Absent = end of activity
 * @property {number} [radius] Detection radius in metres (default 25)
 * @property {number} [total_laps] Manual race total; absent = auto-detect
 */

/** Lap counter metrics driven by the scene-level start/finish gate. */
export const LAP_METRICS = ['lap', 'laps_to_go', 'lap_fraction']

/** Whether a value-element metric token is a lap counter. */
export const isLapMetric = (m) => LAP_METRICS.includes(m)

/**
 * @typedef {Object} SceneConfig
 * @property {number} width
 * @property {number} height
 * @property {number} fps
 * @property {number} [font_size]
 * @property {string} [font]
 * @property {string} [overlay_filename]
 * @property {number} [start]
 * @property {number} [end]
 * @property {number} [decimal_rounding]
 * @property {string} [color]
 * @property {number} [opacity]
 * @property {string[]} [layers]
 * @property {GroupConfig[]} [groups]
 * @property {'metric'|'imperial'} [units]
 * @property {LapGateConfig} [lap_gate]
 */

/**
 * @typedef {Object} Template
 * @property {SceneConfig} scene
 * @property {Element[]} elements
 */

// ─── Element variants ─────────────────────────────────────────────────────────

/**
 * @typedef {Object} LabelElement
 * @property {'label'} type
 * @property {string} id
 * @property {string} text
 * @property {number} x
 * @property {number} y
 * @property {number} [font_size]
 * @property {number} [letter_spacing]
 * @property {string} [font]
 * @property {boolean} [italic]
 * @property {string} [color]
 * @property {number} [opacity]
 * @property {number} [decimal_rounding]
 * @property {'left'|'center'|'right'} [text_align]
 */

/**
 * @typedef {Object} ValueElement
 * @property {'value'} type
 * @property {string} id
 * @property {string} value
 * @property {number} x
 * @property {number} y
 * @property {number} [font_size]
 * @property {string} [font]
 * @property {boolean} [italic]
 * @property {string} [color]
 * @property {number} [opacity]
 * @property {string} [unit]
 * @property {string} [suffix]
 * @property {'none'|'auto'|'custom'} [suffix_mode]
 * @property {number} [decimal_rounding]
 * @property {number} [hours_offset]
 * @property {string} [time_timezone]
 * @property {string} [time_format]
 * @property {boolean} [time_12h]
 * @property {boolean} [time_ampm]
 * @property {'overlay_start'|'activity_start'|'overlay_end'|'activity_end'|'until_custom'|'since_custom'} [distance_reference]
 * @property {number} [distance_target]
 * @property {'overlay_start'|'activity_start'|'overlay_end'|'activity_end'|'until_custom'|'since_custom'|'time_of_day'} [time_reference]
 * @property {number} [time_target]
 * @property {'activity'|'overlay'} [summary_scope] For summary metrics (total_distance, elevation_gain, avg_speed, …): aggregate over the whole activity (default) or the overlay window.
 * @property {'left'|'center'|'right'} [text_align]
 */

/**
 * @typedef {Object} ColorBand
 * @property {number} [max] Upper bound (exclusive) in display units; absent = catch-all top band.
 * @property {string} color
 */

/**
 * @typedef {Object} ColorByConfig Band-colors plot segments by a second metric (TdF-style climb profiles).
 * @property {string} [value] Metric driving the color (default 'gradient').
 * @property {string} [unit] Unit token for band thresholds (e.g. 'mph'); absent = metric display unit.
 * @property {'fill'|'line'|'both'} [mode] What gets colored (default 'fill'; course plots always color the line).
 * @property {ColorBand[]} [bands] Ordered bands; absent falls back to built-in gradient bands.
 */

/**
 * @typedef {Object} PlotElement
 * @property {'plot'} type
 * @property {string} id
 * @property {string} value
 * @property {number} x
 * @property {number} y
 * @property {number} width
 * @property {number} height
 * @property {number} [dpi]
 * @property {string} [color]
 * @property {number} [opacity]
 * @property {LineConfig} [line]
 * @property {FillConfig} [fill]
 * @property {ColorByConfig} [color_by]
 * @property {number} [margin]
 * @property {PointConfig} [point]
 * @property {CourseMarkerConfig[]} [markers]
 * @property {PointLabelConfig} [point_label]
 * @property {number} [rotation]
 * @property {'distance'} [x_axis] Horizontal axis basis for non-course plots; absent = evenly spaced by time.
 */

// Mirrors DEFAULT_GRADIENT_BANDS in src-tauri/src/render/template.rs: descent,
// then TdF-style climb categories. Thresholds in percent grade.
export const GRADIENT_COLOR_BANDS = [
  { max: 0, color: '#3b82f6' },
  { max: 4, color: '#22c55e' },
  { max: 7, color: '#eab308' },
  { max: 10, color: '#f97316' },
  { max: 14, color: '#dc2626' },
  { color: '#7f1d1d' },
]

// Cold→hot ramp used to seed bands for non-gradient color-by metrics.
const COLOR_BY_RAMP = ['#3b82f6', '#22c55e', '#eab308', '#f97316', '#dc2626']

// Starter thresholds per metric, in the unit band values are authored in
// (speed km/h, power W, heartrate bpm, cadence rpm, temperature °C, elevation m).
const COLOR_BY_THRESHOLDS = {
  speed: [10, 20, 30, 40],
  power: [150, 220, 290, 360],
  heartrate: [120, 140, 160, 180],
  cadence: [60, 75, 90, 105],
  temperature: [5, 15, 25, 32],
  elevation: [250, 750, 1500, 2500],
}

/**
 * Fresh default bands for a color-by metric — a starting point the user
 * customizes. Returns new objects each call so edits never share state.
 * @param {string} metric
 * @returns {ColorBand[]}
 */
export function defaultColorBands(metric) {
  if (metric === 'gradient' || !(metric in COLOR_BY_THRESHOLDS)) {
    return GRADIENT_COLOR_BANDS.map((b) => ({ ...b }))
  }
  const bands = COLOR_BY_THRESHOLDS[metric].map((max, i) => ({
    max,
    color: COLOR_BY_RAMP[i],
  }))
  bands.push({ color: COLOR_BY_RAMP[COLOR_BY_RAMP.length - 1] })
  return bands
}

/**
 * @typedef {Object} MeterElement
 * @property {'meter'} type
 * @property {string} id
 * @property {string} value
 * @property {number} x
 * @property {number} y
 * @property {number} width
 * @property {number} height
 * @property {number|'min'|'max'} min
 * @property {number|'min'|'max'} max
 * @property {'up'|'down'|'left'|'right'} [direction]
 * @property {string} [unit]
 * @property {string} [color]
 * @property {string[]} [gradient]
 * @property {string} [background]
 * @property {number} [background_opacity]
 * @property {number} [fill_opacity]
 * @property {number} [opacity]
 * @property {number} [radius]
 * @property {number} [segments]
 * @property {number} [gap]
 * @property {number} [rotation]
 * @property {string} [border_color]
 * @property {number} [border_width]
 * @property {number} [border_opacity]
 * @property {number[]} [scale_labels]
 * @property {string} [scale_color]
 * @property {number} [scale_font_size]
 * @property {string} [scale_font]
 * @property {number} [scale_offset]
 * @property {number} [scale_tick_length]
 * @property {number} [scale_tick_width]
 * @property {string} [scale_suffix]
 * @property {number} [scale_ticks]
 */

/**
 * @typedef {Object} GaugeElement
 * @property {'gauge'} type
 * @property {string} id
 * @property {string} value
 * @property {number} x
 * @property {number} y
 * @property {number} width
 * @property {number} height
 * @property {number|'min'|'max'} min
 * @property {number|'min'|'max'} max
 * @property {string} [unit]
 * @property {number} [start_angle]
 * @property {number} [sweep_angle]
 * @property {string} [color]
 * @property {string} [arc_color]
 * @property {number} [arc_width]
 * @property {string} [progress_color]
 * @property {string[]} [gradient]
 * @property {string} [needle_color]
 * @property {number} [needle_width]
 * @property {string} [cap_color]
 * @property {number} [cap_radius]
 * @property {string} [background_color]
 * @property {number} [background_opacity]
 * @property {number} [background_margin]
 * @property {boolean} [hide_track]
 * @property {number} [opacity]
 * @property {number} [rotation]
 */

/**
 * @typedef {Object} RectElement
 * @property {'rect'} type
 * @property {string} id
 * @property {number} x
 * @property {number} y
 * @property {number} width
 * @property {number} height
 * @property {string} [color]
 * @property {number} [opacity]
 * @property {number} [fill_opacity]
 * @property {string} [border_color]
 * @property {number} [border_width]
 * @property {number} [border_opacity]
 * @property {number} [radius]
 * @property {number} [rotation]
 */

/**
 * @typedef {Object} ImageElement
 * @property {'image'} type
 * @property {string} id
 * @property {string} file
 * @property {number} x
 * @property {number} y
 * @property {number} width
 * @property {number} height
 * @property {number} [opacity]
 * @property {number} [rotation]
 * @property {string} [pulse_metric]
 * @property {number} [pulse_bpm]
 * @property {number} [pulse_amplitude]
 */

/**
 * Any overlay element. Discriminated union on the `type` field.
 * @typedef {LabelElement|ValueElement|PlotElement|MeterElement|GaugeElement|RectElement|ImageElement} Element
 */

/**
 * @typedef {Object} ElementMeta
 * @property {string} icon
 * @property {string} name
 * @property {'label'|'value'|'plot'|'map'|'meter'|'gauge'|'rect'|'image'} kind
 * @property {string|null} unit
 */

export const ADD_PRESETS = [
  {
    key: 'label',
    type: 'label',
    icon: 'type',
    title: 'Add text label',
    defaults: (scene) => ({
      text: 'LABEL',
      x: 100,
      y: 100,
      font_size: scene?.font_size ?? 32,
      italic: false,
      color: '#ffffff',
      opacity: 1,
    }),
  },
  {
    key: 'value',
    type: 'value',
    icon: 'hash',
    title: 'Add metric value',
    defaults: (scene) => ({
      value: 'speed',
      x: 100,
      y: 200,
      font_size: scene?.font_size ?? 48,
      italic: false,
      opacity: 1,
    }),
  },
  {
    key: 'chart',
    type: 'plot',
    icon: 'bar',
    title: 'Add chart',
    defaults: () => ({
      value: 'elevation',
      x: 50,
      y: 800,
      width: 500,
      height: 120,
      opacity: 1,
      line: { color: '#ffffff', width: 1.5 },
      fill: { opacity: 0.25, color: '#ffffff' },
      point: { color: '#ffffff', weight: 80, remove_edge_color: true },
    }),
  },
  {
    key: 'map',
    type: 'plot',
    icon: 'map',
    title: 'Add map (GPS route)',
    defaults: () => ({
      value: 'course',
      x: 50,
      y: 580,
      width: 200,
      height: 200,
      opacity: 1,
      line: { color: '#ffffff', width: 1.5 },
      point: { color: '#ef4444', weight: 80, edge_color: '#ffffff' },
    }),
  },
  {
    key: 'meter',
    type: 'meter',
    icon: 'meter',
    title: 'Add fill meter',
    defaults: () => ({
      value: 'speed',
      x: 80,
      y: 300,
      width: 60,
      height: 400,
      min: 0,
      max: 60,
      direction: 'up',
      color: '#ffffff',
      fill_opacity: 1,
      background: '#ffffff',
      background_opacity: 0.2,
      opacity: 1,
      radius: 8,
    }),
  },
  {
    key: 'gauge',
    type: 'gauge',
    icon: 'gauge',
    title: 'Add gauge (dial)',
    defaults: () => ({
      value: 'speed',
      x: 80,
      y: 300,
      width: 360,
      height: 360,
      min: 0,
      max: 60,
      start_angle: 135,
      sweep_angle: 270,
      color: '#ffffff',
      arc_color: '#ffffff55',
      arc_width: 14,
      progress_color: '#ffffff',
      needle_color: '#ef4444',
      needle_width: 6,
      opacity: 1,
    }),
  },
  {
    key: 'rect',
    type: 'rect',
    icon: 'rect',
    title: 'Add rectangle',
    defaults: () => ({
      x: 100,
      y: 100,
      width: 300,
      height: 200,
      color: '#ffffff',
      fill_opacity: 0.3,
      opacity: 1,
      radius: 0,
    }),
  },
  {
    key: 'image',
    type: 'image',
    icon: 'image',
    title: 'Add image',
    defaults: () => ({
      file: '',
      x: 100,
      y: 100,
      width: 200,
      height: 200,
      opacity: 1,
    }),
  },
]

/**
 * List/handle display metadata for an element instance.
 * `kind` is a display discriminant ('map' splits out of 'plot' for icons).
 * @param {Element|null|undefined} el
 * @returns {ElementMeta}
 */
export function elementMeta(el) {
  if (!el) return { icon: 'bar', name: '', kind: 'plot', unit: null }
  if (el.type === 'label')
    return { icon: 'type', name: el.text || 'Label', kind: 'label', unit: null }
  if (el.type === 'value')
    return {
      icon: 'hash',
      name: el.value || 'value',
      kind: 'value',
      unit: el.unit ?? null,
    }
  if (el.type === 'meter')
    return {
      icon: 'meter',
      name: `${el.value || 'value'} meter`,
      kind: 'meter',
      unit: el.unit ?? null,
    }
  if (el.type === 'gauge')
    return {
      icon: 'gauge',
      name: `${el.value || 'value'} gauge`,
      kind: 'gauge',
      unit: el.unit ?? null,
    }
  if (el.type === 'rect')
    return { icon: 'rect', name: 'rectangle', kind: 'rect', unit: null }
  if (el.type === 'image')
    return {
      icon: 'image',
      name: el.file || 'image',
      kind: 'image',
      unit: null,
    }
  // plot
  if (el.value === 'course')
    return { icon: 'map', name: 'map', kind: 'map', unit: null }
  return { icon: 'bar', name: `${el.value} chart`, kind: 'plot', unit: null }
}

/**
 * Human label for the properties-panel header / status bar.
 * @param {Element|null|undefined} el
 * @returns {string}
 */
export function elementTypeName(el) {
  if (el?.type === 'label') return 'Text Label'
  if (el?.type === 'value') return 'Metric Value'
  if (el?.type === 'meter') return 'Fill Meter'
  if (el?.type === 'gauge') return 'Gauge'
  if (el?.type === 'rect') return 'Rectangle'
  if (el?.type === 'image') return 'Image'
  return el?.value === 'course' ? 'Map' : 'Chart'
}
