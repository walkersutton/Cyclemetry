use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub scene: SceneConfig,
    #[serde(default, deserialize_with = "null_seq_as_default")]
    pub elements: Vec<Element>,
}

/// `#[serde(default)]` covers a missing key but NOT an explicit `null`
/// (which errors with "invalid type: null, expected a sequence").
/// Templates are user-editable / community-sourced, so treat an explicit
/// `null` array the same as absent → empty.
fn null_seq_as_default<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneConfig {
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
    pub font_size: Option<f32>,
    pub font: Option<String>,
    pub overlay_filename: Option<String>,
    pub start: Option<f64>,
    pub end: Option<f64>,
    /// Time-lapse output length in seconds. When set, the whole trimmed ride
    /// window (`start`..`end`) is compressed into this many seconds of video
    /// instead of playing at real time — the basis of the ride-summary flyover.
    /// Absent = real time (output length equals the window). Ignored on
    /// stitched exports, where the footage can't be sped up to match.
    pub target_duration: Option<f64>,
    pub decimal_rounding: Option<i32>,
    pub color: Option<String>,
    pub opacity: Option<f32>,
    /// Stable element ids in back-to-front draw order. Unknown/missing ids are
    /// ignored; elements absent from the list fall back to array order.
    pub layers: Option<Vec<String>>,
    /// Named groups of elements for list organisation and bulk repositioning.
    /// Render pipeline ignores this field entirely.
    #[serde(default)]
    pub groups: Vec<GroupConfig>,
    /// Named color variables. Elements reference them as `"$varname"` in any
    /// color field; a pre-pass resolves them before rendering.
    #[serde(default)]
    pub vars: HashMap<String, String>,
    /// Scene-wide unit system: `"metric"` (default) or `"imperial"`. A pre-pass
    /// (`Template::apply_scene_units`) fills this into any value/meter/gauge
    /// element (and plot point label) that hasn't set an explicit `unit`, so a
    /// single toggle flips every unit-bearing readout. Elements with an explicit
    /// unit override the scene system.
    pub units: Option<String>,
    /// Rider weight in kg, used only to compute the `power_to_weight` (W/kg)
    /// metric at render time. `#[serde(skip)]` is deliberate and load-bearing:
    /// weight is sensitive personal data, so it must never be serialized into a
    /// saved/shared template nor read back from one. It is supplied per render
    /// call from a local-only editor setting and set on the scene after parsing.
    #[serde(skip)]
    pub rider_weight_kg: Option<f32>,
    /// Start/finish line for lap counting (crits). When set, a pre-pass counts
    /// GPS crossings of the gate and the `lap` / `laps_to_go` / `lap_fraction`
    /// metrics become live. One gate per scene — every lap element shares it.
    pub lap_gate: Option<LapGateConfig>,
}

/// Start/finish gate for lap counting, expressed as two moments on the ride
/// timeline (the editor's dual race-playhead bar). The rider is on the line at
/// `start`, so that GPS position becomes the gate point; a lap is counted each
/// time the rider re-enters `radius` metres of it after having left it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LapGateConfig {
    /// Ride time (seconds from activity start) of the race start — the moment
    /// the rider crosses the line for lap 1. Defines the gate position;
    /// crossings before this (warm-up laps) don't count.
    pub start: f64,
    /// Ride time of the race finish (the final crossing). Crossings after this
    /// are cooldown and don't count. Absent = end of activity.
    pub end: Option<f64>,
    /// Detection radius in metres around the gate point (default 25).
    pub radius: Option<f64>,
    /// Total race laps for `laps_to_go` / `lap_fraction`. Absent = auto:
    /// every gate crossing between `start` and `end`.
    pub total_laps: Option<u32>,
}

/// A named collection of element IDs used for list organisation and bulk
/// drag in the editor. Has no effect on rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupConfig {
    pub id: String,
    pub name: String,
    pub element_ids: Vec<String>,
}

/// Pins an element's position to a point on another element's bounding box.
/// A pre-pass (`Template::resolve_anchors`) rewrites the element's `x`/`y`
/// before rendering, so anchored elements track their target automatically.
/// Targets must be box elements (plot/meter/gauge/rect/image) — text bounds
/// vary per frame and can't be anchored to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorConfig {
    /// Id of the target element whose box supplies the anchor point.
    pub target: String,
    /// Point on the target box: "center" (default), "top-left", "top",
    /// "top-right", "left", "right", "bottom-left", "bottom", "bottom-right".
    pub point: Option<String>,
    /// For box elements: which point of *this* element lands on the target
    /// point. Default "center". Text elements ignore this — their glyph
    /// placement around the resolved point follows `text_align` /
    /// `vertical_align` instead.
    pub self_point: Option<String>,
    /// Offset in template px applied after the anchor point is resolved.
    pub offset_x: Option<f32>,
    pub offset_y: Option<f32>,
}

/// Fractional position of a named anchor point within a unit box.
fn point_frac(name: &str) -> (f32, f32) {
    match name {
        "top-left" => (0.0, 0.0),
        "top" => (0.5, 0.0),
        "top-right" => (1.0, 0.0),
        "left" => (0.0, 0.5),
        "right" => (1.0, 0.5),
        "bottom-left" => (0.0, 1.0),
        "bottom" => (0.5, 1.0),
        "bottom-right" => (1.0, 1.0),
        _ => (0.5, 0.5), // "center" and anything unrecognised
    }
}

fn default_fps() -> u32 {
    30
}

/// A single overlay element. Internally tagged by `type`; every variant's
/// config carries a stable `id` used for z-order (elements array order) and
/// frontend selection. Adding a new graphic = one new variant + one
/// `OverlayElement` impl (see frame.rs) + one `scale` arm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Element {
    Label(LabelConfig),
    Value(ValueConfig),
    Plot(PlotConfig),
    Meter(MeterConfig),
    Gauge(GaugeConfig),
    Rect(RectConfig),
    Image(ImageConfig),
}

impl Element {
    pub fn id(&self) -> &str {
        match self {
            Element::Label(c) => &c.id,
            Element::Value(c) => &c.id,
            Element::Plot(c) => &c.id,
            Element::Meter(c) => &c.id,
            Element::Gauge(c) => &c.id,
            Element::Rect(c) => &c.id,
            Element::Image(c) => &c.id,
        }
    }

    /// Scale every spatial field by `factor`. Each variant owns the knowledge
    /// of which of its fields are spatial — non-spatial fields (colors, fonts,
    /// opacity, units, decimal_rounding, fractional margins) are
    /// resolution-independent and left untouched.
    pub fn scale(&mut self, factor: f64) {
        let f32f = factor as f32;
        match self {
            Element::Label(c) => {
                c.x = (c.x as f64 * factor).round() as i32;
                c.y = (c.y as f64 * factor).round() as i32;
                c.font_size = c.font_size.map(|v| v * f32f);
                c.letter_spacing = c.letter_spacing.map(|v| v * f32f);
                scale_anchor(&mut c.anchor, f32f);
            }
            Element::Value(c) => {
                c.x = (c.x as f64 * factor).round() as i32;
                c.y = (c.y as f64 * factor).round() as i32;
                c.font_size = c.font_size.map(|v| v * f32f);
                scale_anchor(&mut c.anchor, f32f);
            }
            Element::Plot(c) => {
                c.x = (c.x as f64 * factor).round() as i32;
                c.y = (c.y as f64 * factor).round() as i32;
                c.width = (c.width as f64 * factor).round() as u32;
                c.height = (c.height as f64 * factor).round() as u32;
                if let Some(line) = c.line.as_mut() {
                    line.width = line.width.map(|v| v * f32f);
                }
                if let Some(pl) = c.point_label.as_mut() {
                    pl.font_size = pl.font_size.map(|v| v * f32f);
                    pl.x_offset = pl.x_offset.map(|v| v * f32f);
                    pl.y_offset = pl.y_offset.map(|v| v * f32f);
                }
                if let Some(p) = c.point.as_mut() {
                    p.weight = p.weight.map(|v| v * f32f);
                    p.edge_width = p.edge_width.map(|v| v * f32f);
                }
                if let Some(points) = c.points.as_mut() {
                    for p in points {
                        p.weight = p.weight.map(|v| v * f32f);
                        p.edge_width = p.edge_width.map(|v| v * f32f);
                    }
                }
                if let Some(markers) = c.markers.as_mut() {
                    for marker in markers {
                        marker.width = marker.width.map(|v| v * f32f);
                        marker.height = marker.height.map(|v| v * f32f);
                    }
                }
            }
            Element::Meter(c) => {
                c.x = (c.x as f64 * factor).round() as i32;
                c.y = (c.y as f64 * factor).round() as i32;
                c.width = (c.width as f64 * factor).round() as u32;
                c.height = (c.height as f64 * factor).round() as u32;
                c.radius = c.radius.map(|v| v * f32f);
                c.gap = c.gap.map(|v| v * f32f);
                c.border_width = c.border_width.map(|v| v * f32f);
                c.scale_font_size = c.scale_font_size.map(|v| v * f32f);
                c.scale_offset = c.scale_offset.map(|v| v * f32f);
                c.scale_tick_length = c.scale_tick_length.map(|v| v * f32f);
                c.scale_tick_width = c.scale_tick_width.map(|v| v * f32f);
            }
            Element::Gauge(c) => {
                c.x = (c.x as f64 * factor).round() as i32;
                c.y = (c.y as f64 * factor).round() as i32;
                c.width = (c.width as f64 * factor).round() as u32;
                c.height = (c.height as f64 * factor).round() as u32;
                c.arc_width = c.arc_width.map(|v| v * f32f);
                c.needle_width = c.needle_width.map(|v| v * f32f);
                c.cap_radius = c.cap_radius.map(|v| v * f32f);
                c.background_margin = c.background_margin.map(|v| v * f32f);
            }
            Element::Rect(c) => {
                c.x = (c.x as f64 * factor).round() as i32;
                c.y = (c.y as f64 * factor).round() as i32;
                c.width = (c.width as f64 * factor).round() as u32;
                c.height = (c.height as f64 * factor).round() as u32;
                c.radius = c.radius.map(|v| v * f32f);
                c.border_width = c.border_width.map(|v| v * f32f);
                scale_anchor(&mut c.anchor, f32f);
            }
            Element::Image(c) => {
                c.x = (c.x as f64 * factor).round() as i32;
                c.y = (c.y as f64 * factor).round() as i32;
                c.width = (c.width as f64 * factor).round() as u32;
                c.height = (c.height as f64 * factor).round() as u32;
                scale_anchor(&mut c.anchor, f32f);
            }
        }
    }

    pub fn anchor(&self) -> Option<&AnchorConfig> {
        match self {
            Element::Label(c) => c.anchor.as_ref(),
            Element::Value(c) => c.anchor.as_ref(),
            Element::Rect(c) => c.anchor.as_ref(),
            Element::Image(c) => c.anchor.as_ref(),
            _ => None,
        }
    }

    /// Static bounding box usable as an anchor target, or `None` for text
    /// elements whose bounds vary per frame.
    fn anchor_box(&self) -> Option<(f32, f32, f32, f32)> {
        match self {
            Element::Plot(c) => Some((c.x as f32, c.y as f32, c.width as f32, c.height as f32)),
            Element::Meter(c) => Some((c.x as f32, c.y as f32, c.width as f32, c.height as f32)),
            Element::Gauge(c) => Some((c.x as f32, c.y as f32, c.width as f32, c.height as f32)),
            Element::Rect(c) => Some((c.x as f32, c.y as f32, c.width as f32, c.height as f32)),
            Element::Image(c) => Some((c.x as f32, c.y as f32, c.width as f32, c.height as f32)),
            _ => None,
        }
    }

    /// Place this element so its anchor reference lands on point `(px, py)`.
    /// Text elements treat the point as their (x, y) origin — `text_align` /
    /// `vertical_align` position the glyphs around it. Box elements offset by
    /// `self_point` (default center) against their own size.
    fn place_at(&mut self, px: f32, py: f32, self_point: Option<&str>) {
        let (x, y) = match self {
            Element::Label(_) | Element::Value(_) => (px, py),
            _ => {
                let (_, _, w, h) = self.anchor_box().unwrap_or((0.0, 0.0, 0.0, 0.0));
                let (fx, fy) = point_frac(self_point.unwrap_or("center"));
                (px - w * fx, py - h * fy)
            }
        };
        let (x, y) = (x.round() as i32, y.round() as i32);
        match self {
            Element::Label(c) => (c.x, c.y) = (x, y),
            Element::Value(c) => (c.x, c.y) = (x, y),
            Element::Rect(c) => (c.x, c.y) = (x, y),
            Element::Image(c) => (c.x, c.y) = (x, y),
            _ => {}
        }
    }
}

fn scale_anchor(anchor: &mut Option<AnchorConfig>, factor: f32) {
    if let Some(a) = anchor {
        a.offset_x = a.offset_x.map(|v| v * factor);
        a.offset_y = a.offset_y.map(|v| v * factor);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelConfig {
    pub id: String,
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub font_size: Option<f32>,
    /// Additional tracking between characters in px. Default 0.
    pub letter_spacing: Option<f32>,
    pub font: Option<String>,
    pub italic: Option<bool>,
    pub color: Option<String>,
    pub opacity: Option<f32>,
    pub decimal_rounding: Option<i32>,
    /// Horizontal alignment relative to x. "left" (default) | "center" | "right".
    pub text_align: Option<String>,
    /// Vertical alignment relative to y. "baseline" (default) | "top" |
    /// "middle" | "bottom". "middle" centers on cap height, which optically
    /// centers digits and stays stable as the text changes per frame.
    pub vertical_align: Option<String>,
    pub anchor: Option<AnchorConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueConfig {
    pub id: String,
    pub value: String,
    pub x: i32,
    pub y: i32,
    pub font_size: Option<f32>,
    pub font: Option<String>,
    pub italic: Option<bool>,
    pub color: Option<String>,
    pub opacity: Option<f32>,
    pub unit: Option<String>,
    /// Manual suffix text, used when `suffix_mode` is "custom" (or absent, for
    /// back-compat with templates authored before `suffix_mode` existed).
    pub suffix: Option<String>,
    /// How to derive the value's trailing suffix: "none", "auto" (unit-derived,
    /// tracks the unit picker — e.g. " km"/" mi", " W", " bpm"), or "custom"
    /// (use `suffix` verbatim). When absent, falls back to custom-if-`suffix`-
    /// present-else-none so existing templates render unchanged.
    pub suffix_mode: Option<String>,
    /// Render digits at a uniform (tabular) advance so a value that changes each
    /// frame — a running counter, a clock — doesn't reflow: every digit occupies
    /// the width of the widest digit, so the number and any trailing suffix stay
    /// put. Non-digit glyphs keep their natural width. Default false (proportional
    /// figures), so existing templates are unchanged.
    pub tabular_figures: Option<bool>,
    pub decimal_rounding: Option<i32>,
    pub hours_offset: Option<f32>,
    pub time_format: Option<String>,
    /// IANA timezone name (e.g. "America/Los_Angeles"). When present, the
    /// frontend computes `hours_offset` from this name + the activity's start
    /// time so DST is handled automatically. Rust rendering uses `hours_offset`.
    pub time_timezone: Option<String>,
    /// Use 12-hour clock for "hh:mm:ss" / "hh:mm" formats. Default false (24-hour).
    pub time_12h: Option<bool>,
    /// Show AM/PM suffix when `time_12h` is true. Default false.
    pub time_ampm: Option<bool>,
    /// For `value: "distance"` — which reference point to measure from/to.
    /// Options: "overlay_start" (default), "activity_start", "overlay_end", "activity_end",
    /// "until_custom" (distance until a custom point), "since_custom" (distance since a custom point).
    pub distance_reference: Option<String>,
    /// For `distance_reference: "until_custom"` or `"since_custom"` — the reference distance in the
    /// element's display unit (km, mi, or m per `unit`). Converted to metres at render time.
    pub distance_target: Option<f64>,
    /// For `value: "time"` — which reference point to measure from/to.
    /// Options: "overlay_start" (default), "activity_start", "overlay_end", "activity_end",
    /// "until_custom", "since_custom", "time_of_day" (wall-clock time, requires GPS timestamps).
    pub time_reference: Option<String>,
    /// For `time_reference: "until_custom"` or `"since_custom"` — the reference time in seconds.
    pub time_target: Option<f64>,
    /// For summary metrics (`total_distance`, `elevation_gain`, `avg_speed`, …):
    /// which window to aggregate over. "activity" (default, whole ride) or
    /// "overlay" (the trimmed segment being rendered). Ignored by live metrics.
    pub summary_scope: Option<String>,
    /// Horizontal alignment relative to x. "left" (default) | "center" | "right".
    pub text_align: Option<String>,
    /// Vertical alignment relative to y. "baseline" (default) | "top" |
    /// "middle" | "bottom". "middle" centers on cap height, which optically
    /// centers digits and stays stable as the text changes per frame.
    pub vertical_align: Option<String>,
    pub anchor: Option<AnchorConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotConfig {
    pub id: String,
    pub value: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpi: Option<f32>,
    pub color: Option<String>,
    pub opacity: Option<f32>,
    pub line: Option<LineConfig>,
    pub fill: Option<FillConfig>,
    /// Color plot segments by a second attribute (e.g. gradient) using
    /// ordered value bands — Tour-de-France-style climb profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_by: Option<ColorByConfig>,
    pub margin: Option<f64>,
    /// Single position marker for the current-value dot (preferred schema).
    pub point: Option<PointConfig>,
    /// Legacy: a list of markers — only the first entry is used. Kept for
    /// backward-compat with user templates authored before the schema change;
    /// new saves always write `point` instead. See `effective_point()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PointConfig>>,
    pub markers: Option<Vec<CourseMarkerConfig>>,
    pub point_label: Option<PointLabelConfig>,
    pub rotation: Option<f32>,
    /// X-axis basis for non-course scalar plots: "time" (default; evenly spaced
    /// samples) or "distance" (horizontal position = distance travelled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_axis: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineConfig {
    pub width: Option<f32>,
    pub color: Option<String>,
    /// Opacity for the portion of the course line before the current position.
    pub past_opacity: Option<f32>,
    /// Opacity for the portion of the course line after the current position.
    pub future_opacity: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillConfig {
    pub opacity: Option<f32>,
    pub color: Option<String>,
}

/// Built-in gradient bands (percent): descent, then TdF-style climb
/// categories. Used when a gradient `color_by` has no explicit bands.
pub const DEFAULT_GRADIENT_BANDS: &[(f64, &str)] = &[
    (0.0, "#3b82f6"),
    (4.0, "#22c55e"),
    (7.0, "#eab308"),
    (10.0, "#f97316"),
    (14.0, "#dc2626"),
    (f64::INFINITY, "#7f1d1d"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorByConfig {
    /// Attribute driving the color (default "gradient").
    pub value: Option<String>,
    /// Unit token for band thresholds (e.g. "mph" to author speed bands in
    /// mph). Absent = the attribute's metric display unit (km/h, °C, m).
    pub unit: Option<String>,
    /// What gets band-colored: "fill" (under-curve), "line", or "both".
    /// Default "fill". Course plots always color the route line only.
    pub mode: Option<String>,
    /// Ordered color bands; a value falls in the first band whose `max`
    /// exceeds it. Absent/empty falls back to the built-in gradient bands
    /// (non-gradient attributes require explicit bands).
    pub bands: Option<Vec<ColorBand>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorBand {
    /// Upper bound (exclusive) in display units. Absent = catch-all top band.
    pub max: Option<f64>,
    pub color: String,
}

impl ColorByConfig {
    pub fn attr(&self) -> &str {
        self.value
            .as_deref()
            .unwrap_or(crate::activity::ATTR_GRADIENT)
    }

    /// Whether the under-curve fill is band-colored. Never for course plots,
    /// which have no fill.
    pub fn color_fill(&self, is_course: bool) -> bool {
        !is_course && !matches!(self.mode.as_deref(), Some("line"))
    }

    /// Whether the line is band-colored. Always for course plots (the route
    /// line is the only paintable surface), else when mode is "line"/"both".
    pub fn color_line(&self, is_course: bool) -> bool {
        is_course || matches!(self.mode.as_deref(), Some("line") | Some("both"))
    }

    /// Bands as (upper bound, color) sorted ascending. Values above the last
    /// bound clamp to it. Empty when no bands apply (disables the feature).
    pub fn resolved_bands(&self) -> Vec<(f64, String)> {
        match &self.bands {
            Some(bands) if !bands.is_empty() => {
                let mut out: Vec<(f64, String)> = bands
                    .iter()
                    .map(|b| (b.max.unwrap_or(f64::INFINITY), b.color.clone()))
                    .collect();
                out.sort_by(|a, b| a.0.total_cmp(&b.0));
                out
            }
            _ if self.attr() == crate::activity::ATTR_GRADIENT => DEFAULT_GRADIENT_BANDS
                .iter()
                .map(|&(max, color)| (max, color.to_string()))
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointConfig {
    pub color: Option<String>,
    pub weight: Option<f32>,
    pub opacity: Option<f32>,
    pub edge_color: Option<String>,
    /// Edge stroke width in px. Default 1.
    pub edge_width: Option<f32>,
    pub remove_edge_color: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseMarkerConfig {
    pub id: Option<String>,
    pub name: Option<String>,
    /// Marker position in metres from the activity start.
    pub distance: Option<f64>,
    /// Visual style: "checkered" (default), "circle", or "rectangle".
    pub style: Option<String>,
    /// Primary color for circle/rectangle markers. Default red.
    pub color: Option<String>,
    /// Long side of the marker in px. Default 34.
    pub width: Option<f32>,
    /// Short side of the marker in px. Default 10.
    pub height: Option<f32>,
    /// Additional clockwise rotation in degrees after the default perpendicular-to-course angle.
    pub rotation: Option<f32>,
    /// Master opacity (0-1). Default 1.
    pub opacity: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointLabelConfig {
    pub font_size: Option<f32>,
    pub color: Option<String>,
    pub font: Option<String>,
    pub italic: Option<bool>,
    pub x_offset: Option<f32>,
    pub y_offset: Option<f32>,
    pub units: Option<Vec<String>>,
    pub decimal_rounding: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RangeBound {
    Number(f64),
    Keyword(RangeKeyword),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RangeKeyword {
    Min,
    Max,
}

/// A bar that fills proportionally to the current value of a metric, mapped
/// linearly between `min` and `max` (both in the element's display `unit`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeterConfig {
    pub id: String,
    pub value: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub min: RangeBound,
    pub max: RangeBound,
    /// Fill growth direction: "up" (default), "down", "left", "right".
    pub direction: Option<String>,
    pub unit: Option<String>,
    /// Fill color (used when `gradient` is unset).
    pub color: Option<String>,
    /// Optional ordered color stops interpolated across the meter's value
    /// range (min→max). When set, each lit portion takes the gradient color
    /// sampled at its position; overrides `color`.
    pub gradient: Option<Vec<String>>,
    /// Optional track (empty portion) color; omitted = no track drawn.
    pub background: Option<String>,
    /// Opacity for the track only (0–1), fully independent of the fill.
    /// Unset = fully opaque (the track color's own alpha applies).
    pub background_opacity: Option<f32>,
    /// Opacity for the filled portion only (0–1). Primary fill-alpha control;
    /// falls back to `opacity` when unset. Independent of `background_opacity`.
    pub fill_opacity: Option<f32>,
    pub opacity: Option<f32>,
    /// Corner radius in px (rounded rect). Default 0 (sharp corners).
    pub radius: Option<f32>,
    /// When >= 1, render as that many discrete segments instead of one
    /// continuous fill. Segments light up proportionally to the value.
    pub segments: Option<u32>,
    /// Gap in px between segments (only used when `segments` is set).
    pub gap: Option<f32>,
    /// Clockwise rotation in degrees around the element center. Default 0.
    pub rotation: Option<f32>,
    /// When set, draws a border around the entire meter bounding box.
    pub border_color: Option<String>,
    /// Border stroke width in px. Default 2.
    pub border_width: Option<f32>,
    /// Border opacity (0–1). Falls back to `opacity` when unset.
    pub border_opacity: Option<f32>,
    /// When set, draws tick marks + value labels beside the meter.
    /// Empty vec → auto labels at min, mid, max. Non-empty → those exact values.
    pub scale_labels: Option<Vec<f64>>,
    /// Color for scale ticks and labels. Falls back to fill color.
    pub scale_color: Option<String>,
    /// Font size for scale labels in px. Default 20.
    pub scale_font_size: Option<f32>,
    /// Typeface for scale labels. Falls back to the scene font.
    pub scale_font: Option<String>,
    /// Gap between the bar edge and the label text in px. Default 8.
    pub scale_offset: Option<f32>,
    /// How far end ticks (min/max) extend beyond the bar edge in px. Default 6. Set 0 for flush.
    pub scale_tick_length: Option<f32>,
    /// Stroke width of tick lines in px. Default 1.
    pub scale_tick_width: Option<f32>,
    /// Optional suffix appended to every tick label (e.g. "mph").
    pub scale_suffix: Option<String>,
    /// Number of evenly-spaced unlabeled tick marks to draw across the full bar range.
    pub scale_ticks: Option<u32>,
}

/// A circular dial: an arc track plus a needle that points to the current
/// value, mapped linearly between `min` and `max` (in the display `unit`)
/// across an angular sweep. Angles are degrees, 0° = east, clockwise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaugeConfig {
    pub id: String,
    pub value: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub min: RangeBound,
    pub max: RangeBound,
    pub unit: Option<String>,
    /// Needle angle at `min`. Default 135° (lower-left).
    pub start_angle: Option<f32>,
    /// Total sweep from `min` to `max`, clockwise. Default 270°.
    pub sweep_angle: Option<f32>,
    /// Base color used for arc/needle when their specific colors are unset.
    pub color: Option<String>,
    pub arc_color: Option<String>,
    pub arc_width: Option<f32>,
    /// Optional filled arc from `start_angle` to the current value.
    pub progress_color: Option<String>,
    /// Gradient color stops for the progress arc (min→max). Overrides `progress_color` when set.
    pub gradient: Option<Vec<String>>,
    pub needle_color: Option<String>,
    pub needle_width: Option<f32>,
    /// Filled circle drawn at the tip of the progress arc. Omit for no cap.
    pub cap_color: Option<String>,
    /// Radius of the cap dot in pixels. Default = arc_width / 2.
    pub cap_radius: Option<f32>,
    /// Filled circle behind the whole gauge. Requires `background_opacity > 0`.
    pub background_color: Option<String>,
    pub background_opacity: Option<f32>,
    /// Extra radius beyond the gauge bounds for the background circle in px. Default 0.
    pub background_margin: Option<f32>,
    /// When true, the unfilled portion of the arc (current value → max) is not drawn.
    pub hide_track: Option<bool>,
    pub opacity: Option<f32>,
    /// Clockwise rotation in degrees around the element center. Default 0.
    pub rotation: Option<f32>,
}

/// A static rectangle. Supports solid fill, stroke border, rounded corners,
/// and rotation. Fill and border opacities are independently controllable:
/// `fill_opacity` × `opacity` gives effective fill alpha; `opacity` alone
/// gates the border — so setting `fill_opacity: 0` with `opacity: 1` draws
/// an outline-only rectangle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RectConfig {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Fill color. Default white.
    pub color: Option<String>,
    /// Master element opacity (0–1). Multiplies fill_opacity for the fill;
    /// applied directly to the border. Default 1.
    pub opacity: Option<f32>,
    /// Fill-specific opacity multiplier (0–1). Effective fill alpha =
    /// fill_opacity × opacity. Default 1 (fill fully visible).
    pub fill_opacity: Option<f32>,
    /// Border stroke color. When unset no border is drawn.
    pub border_color: Option<String>,
    /// Border stroke width in px. Default 2.
    pub border_width: Option<f32>,
    /// Border opacity (0–1). Falls back to `opacity` when unset.
    pub border_opacity: Option<f32>,
    /// Corner radius in px. Default 0 (sharp corners).
    pub radius: Option<f32>,
    /// Clockwise rotation in degrees around the element center. Default 0.
    pub rotation: Option<f32>,
    pub anchor: Option<AnchorConfig>,
}

/// A static image asset (PNG, WebP, or SVG) placed at an absolute position.
/// `file` is resolved against the assets search path at render time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfig {
    pub id: String,
    /// Asset filename (e.g. "bolt.svg") resolved via the assets search path,
    /// or an absolute filesystem path.
    pub file: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Master opacity (0–1). Default 1.
    pub opacity: Option<f32>,
    /// Clockwise rotation in degrees around the element center. Default 0.
    pub rotation: Option<f32>,
    /// Activity metric to read BPM from (e.g. "heartrate"). When set, the
    /// image pulses at the live metric value. Overridden by `pulse_bpm`.
    pub pulse_metric: Option<String>,
    /// Fixed BPM for the pulse animation. Takes priority over `pulse_metric`.
    pub pulse_bpm: Option<f64>,
    /// Peak scale added on each beat (e.g. 0.2 = 20% larger). Default 0.15.
    pub pulse_amplitude: Option<f32>,
    pub anchor: Option<AnchorConfig>,
}

impl PlotConfig {
    /// Returns the single tracking-point config, preferring `point` over the
    /// legacy `points[0]` so both old and new template schemas work.
    pub fn effective_point(&self) -> Option<&PointConfig> {
        self.point
            .as_ref()
            .or_else(|| self.points.as_ref()?.first())
    }

    pub fn has_position_markers(&self) -> bool {
        self.effective_point().is_some()
    }

    pub fn line_color(&self) -> String {
        self.line
            .as_ref()
            .and_then(|l| l.color.clone())
            .or_else(|| self.color.clone())
            .unwrap_or_else(|| "#ffffff".to_string())
    }

    pub fn line_width(&self) -> f32 {
        self.line.as_ref().and_then(|l| l.width).unwrap_or(1.75)
    }

    pub fn fill_opacity(&self) -> Option<f32> {
        self.fill.as_ref().and_then(|f| f.opacity)
    }

    pub fn fill_color(&self) -> String {
        self.fill
            .as_ref()
            .and_then(|f| f.color.clone())
            .or_else(|| self.color.clone())
            .unwrap_or_else(|| "#ffffff".to_string())
    }

    pub fn margin_fraction(&self) -> f64 {
        self.margin.unwrap_or(0.1)
    }

    pub fn line_past_opacity(&self) -> Option<f32> {
        self.line.as_ref().and_then(|l| l.past_opacity)
    }

    pub fn line_future_opacity(&self) -> Option<f32> {
        self.line.as_ref().and_then(|l| l.future_opacity)
    }
}

impl Template {
    #[cfg(test)]
    pub fn from_value(raw: serde_json::Value) -> Result<Self, serde_json::Error> {
        Self::from_value_scaled(raw, None)
    }

    /// Parse a template, optionally retargeting it to a chosen output
    /// resolution. Templates are authored at one resolution; we scale every
    /// spatial field by a single uniform factor derived from **height**
    /// (`target_height / authored_height`) and set the canvas to the exact
    /// target. Non-16:9 targets keep their aspect: the canvas is the target
    /// width/height and elements positioned past the new width simply fall
    /// off-screen (acceptable until aspect-specific template variants exist).
    pub fn from_value_scaled(
        mut raw: serde_json::Value,
        target: Option<(u32, u32)>,
    ) -> Result<Self, serde_json::Error> {
        // Apply scene defaults before deserializing.
        if let Some(scene) = raw.get_mut("scene") {
            if scene.get("fps").is_none() {
                scene["fps"] = serde_json::json!(30);
            }
            if scene.get("font").is_none() {
                scene["font"] = serde_json::json!("Arial.ttf");
            }
        }

        // Resolve scene-level color variables ($varname → hex) throughout the
        // element tree before any field is deserialized.
        let vars: HashMap<String, String> = raw["scene"]["vars"]
            .as_object()
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        if !vars.is_empty()
            && let Some(items) = raw.get_mut("elements").and_then(|v| v.as_array_mut())
        {
            for item in items.iter_mut() {
                resolve_vars(item, &vars);
            }
        }

        // Inherit scene-level defaults (font, color, opacity, …) into each
        // element that doesn't set them explicitly. Generic — copies any
        // absent scene key into the element object; element configs ignore
        // keys they don't declare.
        let scene_snapshot = raw["scene"].clone();
        if let Some(items) = raw.get_mut("elements").and_then(|v| v.as_array_mut()) {
            for item in items.iter_mut() {
                merge_scene_into_item(&scene_snapshot, item);
            }
        }

        let mut template: Template = serde_json::from_value(raw)?;

        if let Some((tw, th)) = target {
            let authored_h = template.scene.height;
            template.scene.width = tw;
            template.scene.height = th;
            if authored_h > 0 {
                // Always retarget (even when factor ≈ 1) so the canvas adopts
                // the chosen width/height; e.g. 3840×2160 → 2160×2160 has
                // factor 1 but still needs the new scene dimensions above.
                let factor = th as f64 / authored_h as f64;
                template.scene.font_size = template.scene.font_size.map(|s| s * factor as f32);
                for el in &mut template.elements {
                    el.scale(factor);
                }
            }
        }

        template.apply_scene_units();
        template.resolve_anchors();
        Ok(template)
    }

    /// Fill the scene-wide unit system (`scene.units`) into every unit-bearing
    /// element that hasn't chosen an explicit `unit`, so one toggle flips every
    /// readout. Metric is `resolve`'s default already, so only "imperial" needs
    /// filling. Only metrics with a metric/imperial distinction are touched;
    /// elements with an explicit `unit` keep it (per-element override).
    fn apply_scene_units(&mut self) {
        let system = match self.scene.units.as_deref() {
            Some("imperial") => "imperial",
            _ => return,
        };
        for el in &mut self.elements {
            match el {
                Element::Value(c) => fill_scene_unit(&c.value, &mut c.unit, system),
                Element::Meter(c) => fill_scene_unit(&c.value, &mut c.unit, system),
                Element::Gauge(c) => fill_scene_unit(&c.value, &mut c.unit, system),
                Element::Plot(c) => {
                    if let Some(pl) = &mut c.point_label
                        && pl.units.is_none()
                        && convertible_metric(&c.value)
                    {
                        pl.units = Some(vec![system.to_string()]);
                    }
                }
                _ => {}
            }
        }
    }

    /// Rewrite the `x`/`y` of every anchored element from its target's box.
    /// Runs after scaling so all coordinates are in output space. Chains
    /// (rect anchored to a rect anchored to a gauge) resolve over multiple
    /// passes; cycles and invalid targets keep their authored position.
    fn resolve_anchors(&mut self) {
        let mut pending: Vec<usize> = (0..self.elements.len())
            .filter(|&i| self.elements[i].anchor().is_some())
            .collect();

        while !pending.is_empty() {
            let mut next = Vec::new();
            for &i in &pending {
                let anchor = self.elements[i].anchor().unwrap().clone();
                let target_idx = self
                    .elements
                    .iter()
                    .position(|e| e.id() == anchor.target && e.id() != self.elements[i].id());
                let Some(j) = target_idx else { continue }; // dangling target
                if pending.contains(&j) || next.contains(&j) {
                    next.push(i); // target not settled yet — retry next pass
                    continue;
                }
                let Some((bx, by, bw, bh)) = self.elements[j].anchor_box() else {
                    continue; // text target unsupported
                };
                let (fx, fy) = point_frac(anchor.point.as_deref().unwrap_or("center"));
                let px = bx + bw * fx + anchor.offset_x.unwrap_or(0.0);
                let py = by + bh * fy + anchor.offset_y.unwrap_or(0.0);
                self.elements[i].place_at(px, py, anchor.self_point.as_deref());
            }
            if next.len() == pending.len() {
                break; // pure cycle — nothing resolvable
            }
            pending = next;
        }
    }

    /// Element indices in back-to-front draw order: explicit `scene.layers`
    /// ids first (deduped, unknown ids skipped), then any remaining elements
    /// in array order.
    pub fn layer_order(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.elements.len());
        let mut seen = vec![false; self.elements.len()];

        if let Some(layers) = &self.scene.layers {
            for id in layers {
                if let Some(idx) = self.elements.iter().position(|e| e.id() == id)
                    && !seen[idx]
                {
                    seen[idx] = true;
                    out.push(idx);
                }
            }
        }
        for (idx, slot) in seen.iter().enumerate() {
            if !slot {
                out.push(idx);
            }
        }
        out
    }
}

/// Recursively replace `"$varname"` strings with their resolved color values.
fn resolve_vars(value: &mut serde_json::Value, vars: &HashMap<String, String>) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(name) = s.strip_prefix('$')
                && let Some(resolved) = vars.get(name)
            {
                *s = resolved.clone();
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                resolve_vars(v, vars);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                resolve_vars(v, vars);
            }
        }
        _ => {}
    }
}

/// Whether a metric (or the base metric of a summary/running metric) has a
/// metric/imperial distinction the scene unit system should drive.
fn convertible_metric(metric: &str) -> bool {
    use crate::activity::unit_base_metric;
    crate::units::has_unit_system(unit_base_metric(metric))
}

/// Set `unit` to the scene system token when the element left it unset and its
/// metric is unit-convertible. `resolve` accepts the "imperial"/"metric" tokens
/// directly, so no per-metric mapping is needed here.
fn fill_scene_unit(metric: &str, unit: &mut Option<String>, system: &str) {
    if unit.is_none() && convertible_metric(metric) {
        *unit = Some(system.to_string());
    }
}

fn merge_scene_into_item(scene: &serde_json::Value, item: &mut serde_json::Value) {
    if let (Some(scene_obj), Some(item_obj)) = (scene.as_object(), item.as_object_mut()) {
        for (k, v) in scene_obj {
            item_obj.entry(k).or_insert_with(|| v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_by_defaults_to_gradient_bands_sorted() {
        let cb: ColorByConfig = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(cb.attr(), "gradient");
        let bands = cb.resolved_bands();
        assert_eq!(bands.len(), DEFAULT_GRADIENT_BANDS.len());
        assert!(bands.windows(2).all(|w| w[0].0 <= w[1].0));
        assert_eq!(bands.last().unwrap().0, f64::INFINITY);
        // Default mode: fill for profiles, line for course.
        assert!(cb.color_fill(false) && !cb.color_line(false));
        assert!(!cb.color_fill(true) && cb.color_line(true));
    }

    #[test]
    fn color_by_explicit_bands_sort_and_catch_all() {
        let cb: ColorByConfig = serde_json::from_value(serde_json::json!({
            "value": "power",
            "mode": "both",
            "bands": [
                { "color": "#111111" },
                { "max": 300, "color": "#222222" },
                { "max": 150, "color": "#333333" }
            ]
        }))
        .unwrap();
        let bands = cb.resolved_bands();
        assert_eq!(bands[0], (150.0, "#333333".to_string()));
        assert_eq!(bands[1], (300.0, "#222222".to_string()));
        assert_eq!(bands[2].0, f64::INFINITY);
        assert!(cb.color_fill(false) && cb.color_line(false));
    }

    #[test]
    fn color_by_non_gradient_without_bands_disables() {
        let cb: ColorByConfig =
            serde_json::from_value(serde_json::json!({ "value": "power" })).unwrap();
        assert!(cb.resolved_bands().is_empty());
    }

    /// The bundled ride-summary flyover template (shipped as the Strava
    /// template) must always parse through the real deserializer and carry
    /// the time-lapse + running-metric wiring, so a schema change can't
    /// silently break the shipped template.
    #[test]
    fn bundled_flyover_template_parses() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../templates/strava/strava.json"
        );
        let text = std::fs::read_to_string(path).expect("flyover template should exist");
        let raw: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let template = Template::from_value(raw).expect("flyover template should deserialize");

        assert_eq!(template.scene.target_duration, Some(3.0));
        let values: Vec<&str> = template
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Value(c) => Some(c.value.as_str()),
                _ => None,
            })
            .collect();
        assert!(values.contains(&"running_time"));
        assert!(values.contains(&"running_distance"));
        assert!(values.contains(&"running_elevation_gain"));
        // The course plot draws on progressively (future segment hidden).
        let plot = template.elements.iter().find_map(|e| match e {
            Element::Plot(c) => Some(c),
            _ => None,
        });
        let plot = plot.expect("flyover has a course plot");
        assert_eq!(plot.value, "course");
        assert_eq!(plot.line.as_ref().and_then(|l| l.future_opacity), Some(0.0));
    }
}
