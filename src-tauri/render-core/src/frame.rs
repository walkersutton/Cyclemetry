/// Per-frame Skia rendering — draws one video frame to a raw RGBA byte buffer.
use resvg;
use serde::Serialize;
use skia_safe::{
    Canvas, Color, Font, FontMgr, FontStyle, ISize, ImageInfo, Paint, RRect, Rect, Typeface,
};
use std::collections::HashMap;

use crate::activity::{
    ATTR_DISTANCE, ATTR_GEAR, ATTR_POWER_TO_WEIGHT, ATTR_TIME, Activity, LAP_FRACTION,
    LAP_LAPS_TO_GO, RUN_TIME, SUM_TOTAL_TIME, decode_gear, decode_lap_fraction, is_lap_metric,
    is_summary_metric, running_base_metric, unit_base_metric,
};
use crate::chart::ChartCache;
use crate::color::{hex_with_opacity, lerp_gradient};
use crate::template::{
    Element, GaugeConfig, ImageConfig, LabelConfig, MeterConfig, PlotConfig, RangeBound,
    RangeKeyword, RectConfig, SceneConfig, Template, ValueConfig,
};
use crate::units;

const ITALIC_SKEW_X: f32 = -0.25;

fn activity_metric_range(
    activity: &Activity,
    metric: &str,
    unit: Option<&str>,
    rider_weight_kg: Option<f32>,
) -> Option<(f64, f64)> {
    if !activity.valid_attributes.contains(&metric.to_string()) {
        return None;
    }
    let (conversion, _) = units::resolve(metric, unit);
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for idx in 0..activity.data_len() {
        let value = conversion.apply(activity.get_metric(metric, idx, rider_weight_kg));
        if value.is_finite() {
            min = min.min(value);
            max = max.max(value);
        }
    }
    if min.is_finite() && max.is_finite() {
        Some((min, max))
    } else {
        None
    }
}

fn resolve_range_bound(bound: &RangeBound, range: Option<(f64, f64)>, fallback: f64) -> f64 {
    match bound {
        RangeBound::Number(v) => *v,
        RangeBound::Keyword(RangeKeyword::Min) => range.map(|(min, _)| min).unwrap_or(fallback),
        RangeBound::Keyword(RangeKeyword::Max) => range.map(|(_, max)| max).unwrap_or(fallback),
    }
}

/// Pixel-perfect bounding box for a single overlay element in overlay coordinates.
#[derive(Debug, Clone, Serialize)]
pub struct ElementBounds {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Everything an element needs to measure or draw itself for a given frame.
/// Shared across all elements so a new graphic just reads what it needs.
pub struct ElementCtx<'a> {
    pub activity: &'a Activity,
    pub scene: &'a SceneConfig,
    pub typefaces: &'a HashMap<String, Typeface>,
    /// One ChartCache per plot element, keyed by element id.
    pub charts: &'a HashMap<String, ChartCache>,
    pub fonts_dir: &'a str,
    /// Pre-decoded images keyed by element id.
    pub images: &'a HashMap<String, skia_safe::Image>,
}

/// One overlay graphic. Every element family implements this; rendering,
/// measuring, cropping, and font/cache prep all dispatch through it with
/// zero per-type branching at the call sites.
pub trait OverlayElement {
    /// Visual bounding box for this frame, or `None` if nothing is drawn.
    fn measure(&self, ctx: &ElementCtx, frame_idx: usize) -> Option<ElementBounds>;

    /// Draw onto the canvas for this frame.
    fn draw(&self, canvas: &Canvas, ctx: &ElementCtx, frame_idx: usize);

    /// `true` if the bounding box varies frame-to-frame (e.g. changing text
    /// width). Static elements are measured once for the crop computation.
    fn is_dynamic(&self) -> bool {
        false
    }

    /// Font filenames this element needs preloaded into the typeface cache.
    fn fonts(&self, _scene: &SceneConfig) -> Vec<String> {
        Vec::new()
    }

    /// Prebuilt per-element chart cache, if any (plots only).
    fn build_chart(&self, _activity: &Activity, _fonts_dir: &str) -> Option<ChartCache> {
        None
    }

    /// Extent used for crop union: `(x0, y0, x1, y1)`. Defaults to the
    /// measured box; plots override to circumscribe their rotation.
    fn crop_extent(&self, ctx: &ElementCtx, frame_idx: usize) -> Option<(f32, f32, f32, f32)> {
        self.measure(ctx, frame_idx)
            .map(|b| (b.x, b.y, b.x + b.w, b.y + b.h))
    }
}

impl Element {
    pub fn as_overlay(&self) -> &dyn OverlayElement {
        match self {
            Element::Label(c) => c,
            Element::Value(c) => c,
            Element::Plot(c) => c,
            Element::Meter(c) => c,
            Element::Gauge(c) => c,
            Element::Rect(c) => c,
            Element::Image(c) => c,
        }
    }
}

// ─── Label ─────────────────────────────────────────────────────────────────

impl OverlayElement for LabelConfig {
    fn fonts(&self, scene: &SceneConfig) -> Vec<String> {
        vec![
            self.font
                .as_deref()
                .or(scene.font.as_deref())
                .unwrap_or("Arial.ttf")
                .to_string(),
        ]
    }

    fn measure(&self, ctx: &ElementCtx, _frame_idx: usize) -> Option<ElementBounds> {
        let font_name = self
            .font
            .as_deref()
            .or(ctx.scene.font.as_deref())
            .unwrap_or("Arial.ttf");
        let font_size = self.font_size.or(ctx.scene.font_size).unwrap_or(32.0);
        let italic = self.italic.unwrap_or(false);
        let font = ctx
            .typefaces
            .get(font_name)
            .map(|tf| font_from_typeface(tf.clone(), font_size, italic))
            .or_else(|| load_font(font_name, font_size, ctx.fonts_dir, italic))?;
        let letter_spacing = self.letter_spacing.unwrap_or(0.0);
        let (text_w, rect) =
            measure_text_with_letter_spacing(&font, &self.text, None, letter_spacing);
        let draw_x = align_x(self.x as f32, text_w, self.text_align.as_deref());
        let baseline = align_baseline_y(self.y as f32, &font, self.vertical_align.as_deref());
        Some(ElementBounds {
            id: self.id.clone(),
            x: draw_x + rect.left,
            y: baseline + rect.top,
            w: rect.width(),
            h: rect.height(),
        })
    }

    fn draw(&self, canvas: &Canvas, ctx: &ElementCtx, _frame_idx: usize) {
        let font_name = self
            .font
            .as_deref()
            .or(ctx.scene.font.as_deref())
            .unwrap_or("Arial.ttf");
        let font_size = self.font_size.or(ctx.scene.font_size).unwrap_or(32.0);
        let italic = self.italic.unwrap_or(false);
        let color_str = self.color.as_deref().unwrap_or("#ffffff");
        let (r, g, b, a) = hex_with_opacity(color_str, self.opacity);
        let color = Color::from_argb(a, r, g, b);

        let font = ctx
            .typefaces
            .get(font_name)
            .map(|tf| font_from_typeface(tf.clone(), font_size, italic))
            .or_else(|| load_font(font_name, font_size, ctx.fonts_dir, italic));
        if let Some(font) = font {
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(color);
            let letter_spacing = self.letter_spacing.unwrap_or(0.0);
            let text_w =
                measure_text_with_letter_spacing(&font, &self.text, Some(&paint), letter_spacing).0;
            let draw_x = align_x(self.x as f32, text_w, self.text_align.as_deref());
            let baseline = align_baseline_y(self.y as f32, &font, self.vertical_align.as_deref());
            draw_text_with_letter_spacing(
                canvas,
                &self.text,
                (draw_x, baseline),
                &font,
                &paint,
                letter_spacing,
            );
        }
    }
}

// ─── Value ─────────────────────────────────────────────────────────────────

impl ValueConfig {
    fn sample(&self, activity: &Activity, frame_idx: usize, rider_weight_kg: Option<f32>) -> f64 {
        if is_summary_metric(&self.value) {
            return activity.summary_value(&self.value, self.summary_scope.as_deref());
        }
        // Lap counters resolve off the gate pre-pass series, not a source
        // attribute — they read 0 until a start/finish gate is configured.
        if is_lap_metric(&self.value) {
            return activity.get_lap(&self.value, frame_idx);
        }
        // Running counters (running_time / running_distance / running_elevation_*)
        // accumulate to the current frame. They aren't raw source attributes, so
        // resolve them before the valid_attributes gate, checking their base
        // telemetry attribute's availability instead.
        if let Some(base) = running_base_metric(&self.value) {
            if !activity.valid_attributes.iter().any(|a| a == base) {
                return 0.0;
            }
            return activity.get_running(&self.value, frame_idx);
        }
        if !activity.valid_attributes.contains(&self.value) {
            return 0.0;
        }
        if self.value == ATTR_DISTANCE {
            let target_m = self
                .distance_target
                .map(|t| units::distance_target_to_m(t, self.unit.as_deref()));
            activity.get_distance(self.distance_reference.as_deref(), target_m, frame_idx)
        } else if self.value == ATTR_TIME {
            activity.get_time(
                self.time_reference.as_deref(),
                self.time_target,
                self.hours_offset.unwrap_or(0.0),
                frame_idx,
            )
        } else {
            // Resolves W/kg (power_to_weight) from the rider weight; a plain
            // series lookup for every other metric.
            activity.get_metric(&self.value, frame_idx, rider_weight_kg)
        }
    }
}

impl OverlayElement for ValueConfig {
    fn is_dynamic(&self) -> bool {
        true
    }

    fn fonts(&self, scene: &SceneConfig) -> Vec<String> {
        vec![
            self.font
                .as_deref()
                .or(scene.font.as_deref())
                .unwrap_or("Arial.ttf")
                .to_string(),
        ]
    }

    fn measure(&self, ctx: &ElementCtx, frame_idx: usize) -> Option<ElementBounds> {
        let raw = self.sample(ctx.activity, frame_idx, ctx.scene.rider_weight_kg);
        let text = format_value(raw, self, !ctx.activity.laps_completed.is_empty());
        let font_name = self
            .font
            .as_deref()
            .or(ctx.scene.font.as_deref())
            .unwrap_or("Arial.ttf");
        let font_size = self.font_size.or(ctx.scene.font_size).unwrap_or(32.0);
        let italic = self.italic.unwrap_or(false);
        let font = ctx
            .typefaces
            .get(font_name)
            .map(|tf| font_from_typeface(tf.clone(), font_size, italic))
            .or_else(|| load_font(font_name, font_size, ctx.fonts_dir, italic))?;
        let (natural_w, rect) = font.measure_str(&text, None);
        let text_w = if self.tabular_figures.unwrap_or(false) {
            let cell = digit_cell_width(&font, None);
            measure_tabular_str(&font, &text, None, cell)
        } else {
            natural_w
        };
        let draw_x = align_x(self.x as f32, text_w, self.text_align.as_deref());
        let baseline = align_baseline_y(self.y as f32, &font, self.vertical_align.as_deref());
        Some(ElementBounds {
            id: self.id.clone(),
            x: draw_x + rect.left,
            y: baseline + rect.top,
            w: rect.width(),
            h: rect.height(),
        })
    }

    fn draw(&self, canvas: &Canvas, ctx: &ElementCtx, frame_idx: usize) {
        let available = if is_summary_metric(&self.value) {
            ctx.activity.has_summary(&self.value)
        } else if is_lap_metric(&self.value) {
            // Lap counters resolve off the gate pre-pass, not a source
            // attribute — always drawable (they read 0 until a gate is set).
            true
        } else if let Some(base) = running_base_metric(&self.value) {
            ctx.activity.valid_attributes.iter().any(|a| a == base)
        } else {
            ctx.activity.valid_attributes.contains(&self.value)
        };
        if !available {
            return;
        }
        let raw = self.sample(ctx.activity, frame_idx, ctx.scene.rider_weight_kg);
        let display = format_value(raw, self, !ctx.activity.laps_completed.is_empty());

        let font_name = self
            .font
            .as_deref()
            .or(ctx.scene.font.as_deref())
            .unwrap_or("Arial.ttf");
        let font_size = self.font_size.or(ctx.scene.font_size).unwrap_or(32.0);
        let italic = self.italic.unwrap_or(false);
        let color_str = self.color.as_deref().unwrap_or("#ffffff");
        let (r, g, b, a) = hex_with_opacity(color_str, self.opacity);
        let color = Color::from_argb(a, r, g, b);

        if let Some(tf) = ctx.typefaces.get(font_name) {
            let font = font_from_typeface(tf.clone(), font_size, italic);
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(color);
            let baseline = align_baseline_y(self.y as f32, &font, self.vertical_align.as_deref());
            if self.tabular_figures.unwrap_or(false) {
                let cell = digit_cell_width(&font, Some(&paint));
                let text_w = measure_tabular_str(&font, &display, Some(&paint), cell);
                let draw_x = align_x(self.x as f32, text_w, self.text_align.as_deref());
                draw_tabular_str(canvas, &display, (draw_x, baseline), &font, &paint, cell);
            } else {
                let text_w = font.measure_str(&display, Some(&paint)).0;
                let draw_x = align_x(self.x as f32, text_w, self.text_align.as_deref());
                canvas.draw_str(&display, (draw_x, baseline), &font, &paint);
            }
        }
    }
}

// ─── Plot ──────────────────────────────────────────────────────────────────

/// Linearly resample `series` (aligned with monotonic `src_dist`) onto the
/// `dst_dist` grid. Used to carry a per-frame metric series onto the
/// source-density route vertices of a course plot. Endpoints clamp; a
/// length mismatch returns empty (which disables banding downstream).
fn resample_by_distance(series: &[f64], src_dist: &[f64], dst_dist: &[f64]) -> Vec<f64> {
    if series.is_empty() || src_dist.len() != series.len() {
        return Vec::new();
    }
    dst_dist
        .iter()
        .map(|&d| {
            let i = src_dist.partition_point(|&s| s < d);
            if i == 0 {
                return series[0];
            }
            if i >= src_dist.len() {
                return series[series.len() - 1];
            }
            let (d0, d1) = (src_dist[i - 1], src_dist[i]);
            let t = if d1 > d0 { (d - d0) / (d1 - d0) } else { 0.0 };
            series[i - 1] + (series[i] - series[i - 1]) * t
        })
        .collect()
}

impl OverlayElement for PlotConfig {
    fn build_chart(&self, activity: &Activity, fonts_dir: &str) -> Option<ChartCache> {
        let (x_data, y_data) = activity.plot_data(&self.value, self.x_axis.as_deref());
        let is_course = self.value == crate::activity::ATTR_COURSE;
        // `distance_data` is the distance axis of the plotted geometry, so for a
        // course it must align with the source-density route, not the frame grid.
        // `frame_distance` is how far the rider has travelled at each frame, and
        // is what locates them along that route.
        let (distance_data, frame_distance) = if is_course {
            (activity.route_distance.clone(), activity.distance.clone())
        } else {
            (Vec::new(), Vec::new())
        };
        // Metric series are per-frame, but a course's geometry is the
        // source-density route — resample the color series onto each route
        // vertex by travelled distance so banding aligns with `x_data`.
        let color_data = self
            .color_by
            .as_ref()
            .map(|cb| {
                let series = activity.plot_data(cb.attr(), None).1;
                if is_course {
                    resample_by_distance(&series, &activity.distance, &activity.route_distance)
                } else {
                    series
                }
            })
            .unwrap_or_default();
        ChartCache::build(
            self,
            x_data,
            y_data,
            distance_data,
            frame_distance,
            color_data,
            fonts_dir,
        )
    }

    fn measure(&self, _ctx: &ElementCtx, _frame_idx: usize) -> Option<ElementBounds> {
        Some(ElementBounds {
            id: self.id.clone(),
            x: self.x as f32,
            y: self.y as f32,
            w: self.width as f32,
            h: self.height as f32,
        })
    }

    fn crop_extent(&self, _ctx: &ElementCtx, _frame_idx: usize) -> Option<(f32, f32, f32, f32)> {
        // Rotated plots are bounded by their circumscribed circle.
        let rot = self.rotation.unwrap_or(0.0);
        if rot != 0.0 {
            let cx = self.x as f32 + self.width as f32 / 2.0;
            let cy = self.y as f32 + self.height as f32 / 2.0;
            let r = ((self.width as f32).powi(2) + (self.height as f32).powi(2)).sqrt() / 2.0;
            Some((cx - r, cy - r, cx + r, cy + r))
        } else {
            Some((
                self.x as f32,
                self.y as f32,
                self.x as f32 + self.width as f32,
                self.y as f32 + self.height as f32,
            ))
        }
    }

    fn draw(&self, canvas: &Canvas, ctx: &ElementCtx, frame_idx: usize) {
        let Some(chart) = ctx.charts.get(&self.id) else {
            return;
        };
        let rotation = self.rotation.unwrap_or(0.0);
        if rotation != 0.0 {
            let cx = self.x as f32 + self.width as f32 / 2.0;
            let cy = self.y as f32 + self.height as f32 / 2.0;
            canvas.save();
            canvas.rotate(rotation, Some(skia_safe::Point::new(cx, cy)));
        }
        let needs_dynamic_chart = self.has_position_markers()
            || self
                .markers
                .as_ref()
                .map(|markers| !markers.is_empty())
                .unwrap_or(false)
            || self.line_past_opacity().is_some()
            || self.line_future_opacity().is_some();
        if needs_dynamic_chart {
            chart.draw_on_canvas(canvas, frame_idx);
        } else {
            canvas.draw_image(
                &chart.background,
                skia_safe::Point::new(chart.x_offset as f32, chart.y_offset as f32),
                None,
            );
        }
        if rotation != 0.0 {
            canvas.restore();
        }
    }
}

// ─── Meter ─────────────────────────────────────────────────────────────────

impl MeterConfig {
    fn range(&self, activity: &Activity, rider_weight_kg: Option<f32>) -> (f64, f64) {
        let dynamic_range =
            activity_metric_range(activity, &self.value, self.unit.as_deref(), rider_weight_kg);
        (
            resolve_range_bound(&self.min, dynamic_range, 0.0),
            resolve_range_bound(&self.max, dynamic_range, 1.0),
        )
    }

    fn scale_font_name<'a>(&'a self, scene: &'a SceneConfig) -> &'a str {
        self.scale_font
            .as_deref()
            .filter(|font| !font.is_empty())
            .or(scene.font.as_deref())
            .unwrap_or("Arial.ttf")
    }

    /// Current fill fraction in [0, 1] for this frame.
    fn fraction(&self, activity: &Activity, frame_idx: usize, rider_weight_kg: Option<f32>) -> f32 {
        if !activity.valid_attributes.contains(&self.value) {
            return 0.0;
        }
        let raw = activity.get_metric(&self.value, frame_idx, rider_weight_kg);
        let (conv, _) = units::resolve(&self.value, self.unit.as_deref());
        let v = conv.apply(raw);
        let (min, max) = self.range(activity, rider_weight_kg);
        let span = max - min;
        if span.abs() < f64::EPSILON {
            return 0.0;
        }
        (((v - min) / span) as f32).clamp(0.0, 1.0)
    }

    fn rect(&self) -> Rect {
        Rect::from_xywh(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        )
    }

    /// Sub-rect of the track that is filled, given the direction.
    fn fill_rect(&self, frac: f32) -> Rect {
        let (x, y, w, h) = (
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        );
        match self.direction.as_deref().unwrap_or("up") {
            "down" => Rect::from_xywh(x, y, w, h * frac),
            "left" => Rect::from_xywh(x + w * (1.0 - frac), y, w * frac, h),
            "right" => Rect::from_xywh(x, y, w * frac, h),
            // "up" (default): grow from the bottom edge upward.
            _ => Rect::from_xywh(x, y + h * (1.0 - frac), w, h * frac),
        }
    }

    /// Rect for segment `i` of `n`, where segment 0 is anchored at the
    /// meter's `min` end (bottom for "up", top for "down", left for
    /// "right", right for "left").
    fn segment_rect(&self, i: u32, n: u32, gap: f32) -> Rect {
        let (x0, y0, w, h) = (
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        );
        let dir = self.direction.as_deref().unwrap_or("up");
        let vertical = matches!(dir, "up" | "down");
        let axis = if vertical { h } else { w };
        let seg = ((axis - gap * (n as f32 - 1.0)) / n as f32).max(0.0);
        let offset = i as f32 * (seg + gap);
        match dir {
            "down" => Rect::from_xywh(x0, y0 + offset, w, seg),
            "right" => Rect::from_xywh(x0 + offset, y0, seg, h),
            "left" => Rect::from_xywh(x0 + w - offset - seg, y0, seg, h),
            _ => Rect::from_xywh(x0, y0 + h - offset - seg, w, seg),
        }
    }

    /// Draw a border around the full meter rect, entirely outside the bounding
    /// box (same outside-stroke technique as RectConfig).
    fn draw_border(&self, canvas: &Canvas, paint: &mut Paint, radius: f32) {
        let Some(bc) = self.border_color.as_deref() else {
            return;
        };
        let bw = self.border_width.unwrap_or(2.0);
        let half = bw / 2.0;
        let outer = Rect::from_xywh(
            self.x as f32 - half,
            self.y as f32 - half,
            self.width as f32 + bw,
            self.height as f32 + bw,
        );
        let border_op = self.border_opacity.or(self.opacity);
        let (r, g, b, a) = hex_with_opacity(bc, border_op);
        paint.set_shader(None);
        paint.set_color(Color::from_argb(a, r, g, b));
        paint.set_style(skia_safe::paint::Style::Stroke);
        paint.set_stroke_width(bw);
        if radius > 0.0 {
            canvas.draw_rrect(
                RRect::new_rect_xy(outer, radius + half, radius + half),
                paint,
            );
        } else {
            canvas.draw_rect(outer, paint);
        }
    }

    /// Two-pass scale rendering.
    /// `under_fill = true`  → draw unlabeled ticks + mid labeled tick lines (called before fill).
    /// `under_fill = false` → draw end tick lines + all labels (called after fill).
    fn draw_scale(&self, canvas: &Canvas, ctx: &ElementCtx, under_fill: bool) {
        let tick_count = self.scale_ticks.unwrap_or(0);
        let has_labels = self.scale_labels.is_some();
        if tick_count == 0 && !has_labels {
            return;
        }

        let color_str = self
            .scale_color
            .as_deref()
            .or(self.color.as_deref())
            .unwrap_or("#ffffff");
        let (r, g, b, a) = hex_with_opacity(color_str, self.opacity);
        let color = Color::from_argb(a, r, g, b);

        let end_ext = self.scale_tick_length.unwrap_or(6.0);
        let tick_w = self.scale_tick_width.unwrap_or(1.0);
        let offset = self.scale_offset.unwrap_or(8.0);
        let suffix = self.scale_suffix.as_deref().unwrap_or("");
        let (min, max) = self.range(ctx.activity, ctx.scene.rider_weight_kg);

        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(color);

        let dir = self.direction.as_deref().unwrap_or("up");
        let x0 = self.x as f32;
        let y0 = self.y as f32;
        let w = self.width as f32;
        let h = self.height as f32;

        if under_fill {
            // ── Unlabeled ticks (all span bar width, go under fill) ──────────
            if tick_count >= 1 && tick_w > 0.0 {
                paint.set_style(skia_safe::paint::Style::Stroke);
                paint.set_stroke_width(tick_w);
                for i in 0..tick_count {
                    let t = if tick_count == 1 {
                        0.5_f32
                    } else {
                        i as f32 / (tick_count - 1) as f32
                    };
                    if matches!(dir, "up" | "down") {
                        let ref_y = if dir == "down" {
                            y0 + h * t
                        } else {
                            y0 + h * (1.0 - t)
                        };
                        canvas.draw_line((x0, ref_y), (x0 + w, ref_y), &paint);
                    } else {
                        let ref_x = if dir == "left" {
                            x0 + w * (1.0 - t)
                        } else {
                            x0 + w * t
                        };
                        canvas.draw_line((ref_x, y0), (ref_x, y0 + h), &paint);
                    }
                }
            }

            // ── Mid labeled tick lines (no extension, go under fill) ─────────
            if has_labels && tick_w > 0.0 {
                let labels = self.scale_labels.as_ref().unwrap();
                let mid = (min + max) / 2.0;
                let values: Vec<f64> = if labels.is_empty() {
                    vec![min, mid, max]
                } else {
                    labels.clone()
                };
                let span = (max - min) as f32;
                let n = values.len();
                paint.set_style(skia_safe::paint::Style::Stroke);
                paint.set_stroke_width(tick_w);
                for (idx, &v) in values.iter().enumerate() {
                    if idx == 0 || idx == n - 1 {
                        continue; // end ticks drawn in the over pass
                    }
                    let t = if span.abs() < f32::EPSILON {
                        0.0_f32
                    } else {
                        ((v - min) as f32 / span).clamp(0.0, 1.0)
                    };
                    if matches!(dir, "up" | "down") {
                        let ref_y = if dir == "down" {
                            y0 + h * t
                        } else {
                            y0 + h * (1.0 - t)
                        };
                        canvas.draw_line((x0, ref_y), (x0 + w, ref_y), &paint);
                    } else {
                        let ref_x = if dir == "left" {
                            x0 + w * (1.0 - t)
                        } else {
                            x0 + w * t
                        };
                        canvas.draw_line((ref_x, y0), (ref_x, y0 + h), &paint);
                    }
                }
            }
        } else {
            // ── Over fill: end tick lines + all labels ───────────────────────
            if !has_labels {
                return;
            }
            let labels = self.scale_labels.as_ref().unwrap();
            let mid = (min + max) / 2.0;
            let values: Vec<f64> = if labels.is_empty() {
                vec![min, mid, max]
            } else {
                labels.clone()
            };

            let font_name = self.scale_font_name(ctx.scene);
            let font_size = self.scale_font_size.unwrap_or(20.0);
            let font = ctx
                .typefaces
                .get(font_name)
                .map(|tf| font_from_typeface(tf.clone(), font_size, false))
                .or_else(|| load_font(font_name, font_size, ctx.fonts_dir, false));
            let font = match font {
                Some(f) => f,
                None => return,
            };

            let span = (max - min) as f32;
            let (_, metrics) = font.metrics();
            let cap_h = metrics.cap_height.abs();
            let n = values.len();

            for (idx, &v) in values.iter().enumerate() {
                let t = if span.abs() < f32::EPSILON {
                    0.0_f32
                } else {
                    ((v - min) as f32 / span).clamp(0.0, 1.0)
                };
                let is_end = idx == 0 || idx == n - 1;
                let ext = if is_end { end_ext } else { 0.0 };
                let label = if (v.fract()).abs() < 1e-9 {
                    format!("{}{}", v as i64, suffix)
                } else {
                    format!("{:.1}{}", v, suffix)
                };

                if matches!(dir, "up" | "down") {
                    let ref_y = if dir == "down" {
                        y0 + h * t
                    } else {
                        y0 + h * (1.0 - t)
                    };
                    if is_end && tick_w > 0.0 {
                        paint.set_style(skia_safe::paint::Style::Stroke);
                        paint.set_stroke_width(tick_w);
                        canvas.draw_line((x0 - ext, ref_y), (x0 + w + ext, ref_y), &paint);
                    }
                    let label_x = x0 + w + end_ext + offset;
                    let label_y = ref_y + cap_h / 2.0;
                    paint.set_style(skia_safe::paint::Style::Fill);
                    canvas.draw_str(&label, (label_x, label_y), &font, &paint);
                } else {
                    let ref_x = if dir == "left" {
                        x0 + w * (1.0 - t)
                    } else {
                        x0 + w * t
                    };
                    if is_end && tick_w > 0.0 {
                        paint.set_style(skia_safe::paint::Style::Stroke);
                        paint.set_stroke_width(tick_w);
                        canvas.draw_line((ref_x, y0 - ext), (ref_x, y0 + h + ext), &paint);
                    }
                    let text_w = font.measure_str(&label, Some(&paint)).0;
                    let label_x = ref_x - text_w / 2.0;
                    let label_y = y0 + h + end_ext + offset + cap_h;
                    paint.set_style(skia_safe::paint::Style::Fill);
                    canvas.draw_str(&label, (label_x, label_y), &font, &paint);
                }
            }
        }
    }

    fn draw_segmented(&self, canvas: &Canvas, paint: &mut Paint, n: u32, frac: f32, radius: f32) {
        let gap = self.gap.unwrap_or(0.0).max(0.0);
        let lit = (frac * n as f32).round() as u32;
        let grad = self.gradient.as_ref().filter(|g| !g.is_empty());
        for i in 0..n {
            let color = if i < lit {
                let fill_op = self.fill_opacity.or(self.opacity);
                let (r, g, b, a) = match grad {
                    Some(stops) => {
                        let t = (i as f32 + 0.5) / n as f32;
                        lerp_gradient(stops, t, fill_op)
                    }
                    None => hex_with_opacity(self.color.as_deref().unwrap_or("#ffffff"), fill_op),
                };
                Some(Color::from_argb(a, r, g, b))
            } else {
                self.background.as_deref().map(|bg| {
                    let bg_opacity = self.background_opacity;
                    let (r, g, b, a) = hex_with_opacity(bg, bg_opacity);
                    Color::from_argb(a, r, g, b)
                })
            };
            if let Some(c) = color {
                paint.set_color(c);
                let rect = self.segment_rect(i, n, gap);
                if radius > 0.0 {
                    canvas.draw_rrect(RRect::new_rect_xy(rect, radius, radius), paint);
                } else {
                    canvas.draw_rect(rect, paint);
                }
            }
        }
    }
}

impl OverlayElement for MeterConfig {
    fn fonts(&self, scene: &SceneConfig) -> Vec<String> {
        if self.scale_labels.is_none() {
            return Vec::new();
        }

        vec![self.scale_font_name(scene).to_string()]
    }

    fn measure(&self, _ctx: &ElementCtx, _frame_idx: usize) -> Option<ElementBounds> {
        Some(ElementBounds {
            id: self.id.clone(),
            x: self.x as f32,
            y: self.y as f32,
            w: self.width as f32,
            h: self.height as f32,
        })
    }

    fn draw(&self, canvas: &Canvas, ctx: &ElementCtx, frame_idx: usize) {
        let rotation = self.rotation.unwrap_or(0.0);
        if rotation != 0.0 {
            let cx = self.x as f32 + self.width as f32 / 2.0;
            let cy = self.y as f32 + self.height as f32 / 2.0;
            canvas.save();
            canvas.rotate(rotation, Some(skia_safe::Point::new(cx, cy)));
        }

        let radius = self.radius.unwrap_or(0.0);
        let mut paint = Paint::default();
        paint.set_anti_alias(true);

        if let Some(n) = self.segments.filter(|n| *n >= 1) {
            let frac = self.fraction(ctx.activity, frame_idx, ctx.scene.rider_weight_kg);
            self.draw_segmented(canvas, &mut paint, n, frac, radius);
            self.draw_border(canvas, &mut paint, radius);
            self.draw_scale(canvas, ctx, false);
            if rotation != 0.0 {
                canvas.restore();
            }
            return;
        }

        // Track (empty portion), if a background color is set.
        if let Some(bg) = self.background.as_deref() {
            let bg_opacity = self.background_opacity;
            let (r, g, b, a) = hex_with_opacity(bg, bg_opacity);
            paint.set_color(Color::from_argb(a, r, g, b));
            if radius > 0.0 {
                canvas.draw_rrect(RRect::new_rect_xy(self.rect(), radius, radius), &paint);
            } else {
                canvas.draw_rect(self.rect(), &paint);
            }
        }

        // Mid ticks go under the fill so the fill bar overlaps them.
        self.draw_scale(canvas, ctx, true);

        // Fill.
        let frac = self.fraction(ctx.activity, frame_idx, ctx.scene.rider_weight_kg);
        if frac > 0.0 {
            let fr = self.fill_rect(frac);

            if let Some(stops) = self.gradient.as_ref().filter(|g| g.len() >= 2) {
                // Direction-aware gradient anchored to the full element bounds so
                // the gradient position is stable as the fill grows — a half-full
                // "up" meter shows the low-value colors at the bottom.
                let cx = self.x as f32 + self.width as f32 / 2.0;
                let cy = self.y as f32 + self.height as f32 / 2.0;
                let (p0, p1) = match self.direction.as_deref().unwrap_or("up") {
                    "down" => (
                        skia_safe::Point::new(cx, self.y as f32),
                        skia_safe::Point::new(cx, self.y as f32 + self.height as f32),
                    ),
                    "right" => (
                        skia_safe::Point::new(self.x as f32, cy),
                        skia_safe::Point::new(self.x as f32 + self.width as f32, cy),
                    ),
                    "left" => (
                        skia_safe::Point::new(self.x as f32 + self.width as f32, cy),
                        skia_safe::Point::new(self.x as f32, cy),
                    ),
                    // "up": low color at bottom (fill grows upward), high at top.
                    _ => (
                        skia_safe::Point::new(cx, self.y as f32 + self.height as f32),
                        skia_safe::Point::new(cx, self.y as f32),
                    ),
                };
                let fill_op = self.fill_opacity.or(self.opacity);
                let n = stops.len();
                let colors: Vec<Color> = stops
                    .iter()
                    .map(|s| {
                        let (r, g, b, a) = hex_with_opacity(s, fill_op);
                        Color::from_argb(a, r, g, b)
                    })
                    .collect();
                let pos: Vec<f32> = (0..n).map(|i| i as f32 / (n - 1) as f32).collect();
                let shader = skia_safe::gradient_shader::linear(
                    (p0, p1),
                    skia_safe::gradient_shader::GradientShaderColors::Colors(&colors),
                    Some(pos.as_slice()),
                    skia_safe::TileMode::Clamp,
                    None,
                    None,
                );
                paint.set_shader(shader);
                // A shader's output is still modulated by the paint's alpha.
                // The background draw above may have left a low/zero alpha, so
                // reset to opaque — the fill's own alpha is baked into the stops.
                paint.set_alpha(255);
            } else {
                let color_str = self.color.as_deref().unwrap_or("#ffffff");
                let fill_op = self.fill_opacity.or(self.opacity);
                let (r, g, b, a) = hex_with_opacity(color_str, fill_op);
                paint.set_color(Color::from_argb(a, r, g, b));
            }

            if radius > 0.0 {
                canvas.draw_rrect(RRect::new_rect_xy(fr, radius, radius), &paint);
            } else {
                canvas.draw_rect(fr, &paint);
            }
            paint.set_shader(None);
        }

        self.draw_border(canvas, &mut paint, radius);
        // End tick lines + all labels go on top of the fill.
        self.draw_scale(canvas, ctx, false);

        if rotation != 0.0 {
            canvas.restore();
        }
    }
}

// ─── Gauge ─────────────────────────────────────────────────────────────────

impl GaugeConfig {
    fn range(&self, activity: &Activity, rider_weight_kg: Option<f32>) -> (f64, f64) {
        let dynamic_range =
            activity_metric_range(activity, &self.value, self.unit.as_deref(), rider_weight_kg);
        (
            resolve_range_bound(&self.min, dynamic_range, 0.0),
            resolve_range_bound(&self.max, dynamic_range, 1.0),
        )
    }

    fn fraction(&self, activity: &Activity, frame_idx: usize, rider_weight_kg: Option<f32>) -> f32 {
        if !activity.valid_attributes.contains(&self.value) {
            return 0.0;
        }
        let raw = activity.get_metric(&self.value, frame_idx, rider_weight_kg);
        let (conv, _) = units::resolve(&self.value, self.unit.as_deref());
        let v = conv.apply(raw);
        let (min, max) = self.range(activity, rider_weight_kg);
        let span = max - min;
        if span.abs() < f64::EPSILON {
            return 0.0;
        }
        (((v - min) / span) as f32).clamp(0.0, 1.0)
    }

    fn argb(&self, specific: Option<&str>, opacity: Option<f32>) -> Color {
        let s = specific.or(self.color.as_deref()).unwrap_or("#ffffff");
        let (r, g, b, a) = hex_with_opacity(s, opacity);
        Color::from_argb(a, r, g, b)
    }
}

impl OverlayElement for GaugeConfig {
    fn measure(&self, _ctx: &ElementCtx, _frame_idx: usize) -> Option<ElementBounds> {
        Some(ElementBounds {
            id: self.id.clone(),
            x: self.x as f32,
            y: self.y as f32,
            w: self.width as f32,
            h: self.height as f32,
        })
    }

    fn draw(&self, canvas: &Canvas, ctx: &ElementCtx, frame_idx: usize) {
        let cx = self.x as f32 + self.width as f32 / 2.0;
        let cy = self.y as f32 + self.height as f32 / 2.0;

        let rotation = self.rotation.unwrap_or(0.0);
        if rotation != 0.0 {
            canvas.save();
            canvas.rotate(rotation, Some(skia_safe::Point::new(cx, cy)));
        }

        let arc_w = self.arc_width.unwrap_or(8.0);
        let needle_w = self.needle_width.unwrap_or(4.0);
        // Keep the stroked arc fully inside the bounding box.
        let r = (self.width.min(self.height) as f32) / 2.0 - arc_w / 2.0;
        if r <= 0.0 {
            if rotation != 0.0 {
                canvas.restore();
            }
            return;
        }
        let start = self.start_angle.unwrap_or(135.0);
        let sweep = self.sweep_angle.unwrap_or(270.0);
        let frac = self.fraction(ctx.activity, frame_idx, ctx.scene.rider_weight_kg);

        // Background circle.
        if let Some(bg_op) = self.background_opacity.filter(|&op| op > 0.0) {
            let bg_str = self.background_color.as_deref().unwrap_or("#000000");
            let (r2, g2, b2, _) = crate::color::parse_hex_color(bg_str);
            let alpha = (bg_op.clamp(0.0, 1.0) * 255.0) as u8;
            let mut bg_paint = Paint::default();
            bg_paint.set_anti_alias(true);
            bg_paint.set_color(Color::from_argb(alpha, r2, g2, b2));
            bg_paint.set_style(skia_safe::paint::Style::Fill);
            let full_r =
                self.width.min(self.height) as f32 / 2.0 + self.background_margin.unwrap_or(0.0);
            canvas.draw_circle((cx, cy), full_r, &bg_paint);
        }

        let oval = Rect::from_ltrb(cx - r, cy - r, cx + r, cy + r);

        let mut arc_paint = Paint::default();
        arc_paint.set_anti_alias(true);
        arc_paint.set_style(skia_safe::paint::Style::Stroke);
        arc_paint.set_stroke_cap(skia_safe::paint::Cap::Round);

        arc_paint.set_stroke_width(arc_w);

        // Track arc (unfilled portion, start → max). Hidden when hide_track is set.
        if !self.hide_track.unwrap_or(false) {
            arc_paint.set_color(self.argb(self.arc_color.as_deref(), self.opacity));
            canvas.draw_arc(oval, start, sweep, false, &arc_paint);
        }

        // Progress arc (start → current).
        if frac > 0.0 {
            let progress_sweep = sweep * frac;
            let has_gradient = self
                .gradient
                .as_ref()
                .map(|g| g.len() >= 2)
                .unwrap_or(false);

            if has_gradient {
                let grad = self.gradient.as_ref().unwrap();
                let n = grad.len();
                let colors: Vec<Color> = grad
                    .iter()
                    .map(|c| self.argb(Some(c), self.opacity))
                    .collect();
                let pos: Vec<f32> = (0..n).map(|i| i as f32 / (n - 1) as f32).collect();
                // Rotate the canvas so the arc begins at 0° in local space.
                // The sweep gradient range (0, sweep) then maps directly onto the
                // arc without any local-matrix offset and without the 360°-wrap
                // problem that occurs when the arc crosses the East axis.
                canvas.save();
                canvas.rotate(start, Some(skia_safe::Point::new(cx, cy)));
                let shader = skia_safe::gradient_shader::sweep(
                    skia_safe::Point::new(cx, cy),
                    skia_safe::gradient_shader::GradientShaderColors::Colors(&colors),
                    pos.as_slice(),
                    skia_safe::TileMode::Clamp,
                    Some((0.0, sweep)),
                    None,
                    None::<&skia_safe::Matrix>,
                );
                arc_paint.set_shader(shader);
                arc_paint.set_alpha(255);
                canvas.draw_arc(oval, 0.0, progress_sweep, false, &arc_paint);
                arc_paint.set_shader(None);
                // The Cap::Round at the arc start bleeds ~0.5° backward. Skia's
                // sweep gradient normalises angles to [0,360), so -ε° becomes ~360°,
                // which sits past the gradient end and clamps to the last colour.
                // Cover it with a filled dot in colors[0] at the arc start point
                // (local 0° = East = cx+r, cy while the canvas is still rotated).
                let mut start_cap_paint = Paint::default();
                start_cap_paint.set_anti_alias(true);
                start_cap_paint.set_style(skia_safe::paint::Style::Fill);
                start_cap_paint.set_color(colors[0]);
                canvas.draw_circle((cx + r, cy), arc_w / 2.0, &start_cap_paint);
                canvas.restore();
            } else if let Some(pc) = self.progress_color.as_deref() {
                arc_paint.set_color(self.argb(Some(pc), self.opacity));
                canvas.draw_arc(oval, start, progress_sweep, false, &arc_paint);
            }
        }

        // Cap dot at the tip of the progress arc.
        if let Some(cap_str) = self.cap_color.as_deref() {
            let theta = (start + sweep * frac).to_radians();
            let dot_x = cx + theta.cos() * r;
            let dot_y = cy + theta.sin() * r;
            let cap_r = self.cap_radius.unwrap_or(arc_w / 2.0);
            let mut cap_paint = Paint::default();
            cap_paint.set_anti_alias(true);
            cap_paint.set_color(self.argb(Some(cap_str), self.opacity));
            cap_paint.set_style(skia_safe::paint::Style::Fill);
            canvas.draw_circle((dot_x, dot_y), cap_r, &cap_paint);
        }

        // Needle (traditional pointer from center). Skipped when needle_width = 0.
        if needle_w > 0.0 {
            let theta = (start + sweep * frac).to_radians();
            let nx = cx + theta.cos() * r;
            let ny = cy + theta.sin() * r;
            let mut needle = Paint::default();
            needle.set_anti_alias(true);
            needle.set_style(skia_safe::paint::Style::Stroke);
            needle.set_stroke_cap(skia_safe::paint::Cap::Round);
            needle.set_stroke_width(needle_w);
            needle.set_color(self.argb(self.needle_color.as_deref(), self.opacity));
            canvas.draw_line((cx, cy), (nx, ny), &needle);

            // Hub.
            needle.set_style(skia_safe::paint::Style::Fill);
            canvas.draw_circle((cx, cy), needle_w * 1.5, &needle);
        }

        if rotation != 0.0 {
            canvas.restore();
        }
    }
}

// ─── Rect ──────────────────────────────────────────────────────────────────

impl OverlayElement for RectConfig {
    fn measure(&self, _ctx: &ElementCtx, _frame_idx: usize) -> Option<ElementBounds> {
        Some(ElementBounds {
            id: self.id.clone(),
            x: self.x as f32,
            y: self.y as f32,
            w: self.width as f32,
            h: self.height as f32,
        })
    }

    fn crop_extent(&self, _ctx: &ElementCtx, _frame_idx: usize) -> Option<(f32, f32, f32, f32)> {
        let rot = self.rotation.unwrap_or(0.0);
        if rot != 0.0 {
            let cx = self.x as f32 + self.width as f32 / 2.0;
            let cy = self.y as f32 + self.height as f32 / 2.0;
            let r = ((self.width as f32).powi(2) + (self.height as f32).powi(2)).sqrt() / 2.0;
            Some((cx - r, cy - r, cx + r, cy + r))
        } else {
            Some((
                self.x as f32,
                self.y as f32,
                self.x as f32 + self.width as f32,
                self.y as f32 + self.height as f32,
            ))
        }
    }

    fn draw(&self, canvas: &Canvas, _ctx: &ElementCtx, _frame_idx: usize) {
        let rotation = self.rotation.unwrap_or(0.0);
        let rect = Rect::from_xywh(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        );
        if rotation != 0.0 {
            let cx = self.x as f32 + self.width as f32 / 2.0;
            let cy = self.y as f32 + self.height as f32 / 2.0;
            canvas.save();
            canvas.rotate(rotation, Some(skia_safe::Point::new(cx, cy)));
        }

        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        let radius = self.radius.unwrap_or(0.0);

        // fill_opacity is the primary fill control; opacity is the fallback
        // and also gates the border. They are NOT multiplied — set fill_opacity
        // to 0 for a transparent fill while keeping opacity:1 for a visible border.
        let fill_op = self.fill_opacity.or(self.opacity);
        let color_str = self.color.as_deref().unwrap_or("#ffffff");
        let (r, g, b, a) = hex_with_opacity(color_str, fill_op);
        paint.set_color(Color::from_argb(a, r, g, b));
        paint.set_style(skia_safe::paint::Style::Fill);
        if radius > 0.0 {
            canvas.draw_rrect(RRect::new_rect_xy(rect, radius, radius), &paint);
        } else {
            canvas.draw_rect(rect, &paint);
        }

        // Border stroke — drawn outside the fill rect so the element's
        // bounding box is the inner edge of the stroke, not the center.
        // Skia strokes are centered by default, so we expand the rect by
        // half the stroke width on every side to achieve an outside stroke.
        if let Some(bc) = self.border_color.as_deref() {
            let bw = self.border_width.unwrap_or(2.0);
            let half = bw / 2.0;
            let outer = Rect::from_xywh(
                self.x as f32 - half,
                self.y as f32 - half,
                self.width as f32 + bw,
                self.height as f32 + bw,
            );
            let border_op = self.border_opacity.or(self.opacity);
            let (r, g, b, a) = hex_with_opacity(bc, border_op);
            paint.set_color(Color::from_argb(a, r, g, b));
            paint.set_style(skia_safe::paint::Style::Stroke);
            paint.set_stroke_width(bw);
            if radius > 0.0 {
                canvas.draw_rrect(
                    RRect::new_rect_xy(outer, radius + half, radius + half),
                    &paint,
                );
            } else {
                canvas.draw_rect(outer, &paint);
            }
        }

        if rotation != 0.0 {
            canvas.restore();
        }
    }
}

// ─── Measurement ───────────────────────────────────────────────────────────

/// Measure every element in the template for the given frame, returning
/// pixel-perfect bounding boxes using the same Skia font metrics used to render.
pub fn measure_elements(
    frame_idx: usize,
    activity: &Activity,
    template: &Template,
    fonts_dir: &str,
) -> Vec<ElementBounds> {
    let charts: HashMap<String, ChartCache> = HashMap::new();
    let typefaces: HashMap<String, Typeface> = HashMap::new();
    let images: HashMap<String, skia_safe::Image> = HashMap::new();
    let ctx = ElementCtx {
        activity,
        scene: &template.scene,
        typefaces: &typefaces,
        charts: &charts,
        fonts_dir,
        images: &images,
    };
    template
        .elements
        .iter()
        .filter_map(|e| e.as_overlay().measure(&ctx, frame_idx))
        .collect()
}

/// All pre-computed data that stays constant across frames.
pub struct SceneCache {
    /// Pre-rendered base frame as an immutable Skia Image.
    /// Stored as Image (not raw bytes) to avoid a heap allocation + 8 MB copy on every frame.
    pub base_image: skia_safe::Image,
    /// One ChartCache per plot element, keyed by element id.
    pub charts: HashMap<String, ChartCache>,
    pub width: u32,
    pub height: u32,
    /// Pre-loaded typefaces keyed by filename. Eliminates disk I/O inside the per-frame
    /// hot path — Font::new(typeface.clone(), size) is trivially cheap.
    pub typefaces: HashMap<String, Typeface>,
    /// Pre-decoded images keyed by element id.
    pub images: HashMap<String, skia_safe::Image>,
}

impl SceneCache {
    pub fn build(
        activity: &Activity,
        template: &Template,
        fonts_dir: &str,
        assets_dirs: &[&str],
    ) -> Result<Self, String> {
        let w = template.scene.width;
        let h = template.scene.height;

        // --- Pre-load all typefaces referenced by elements ---
        let mut typefaces: HashMap<String, Typeface> = HashMap::new();
        for font_name in template
            .elements
            .iter()
            .flat_map(|e| e.as_overlay().fonts(&template.scene))
        {
            if let std::collections::hash_map::Entry::Vacant(e) = typefaces.entry(font_name.clone())
            {
                if let Some(tf) = load_typeface(&font_name, fonts_dir) {
                    e.insert(tf);
                } else {
                    eprintln!(
                        "Warning: could not load font '{font_name}'; text elements using it will not render"
                    );
                }
            }
        }

        // --- Build chart caches (keyed by element id) ---
        let mut charts: HashMap<String, ChartCache> = HashMap::new();
        for el in &template.elements {
            if let Some(cache) = el.as_overlay().build_chart(activity, fonts_dir) {
                charts.insert(el.id().to_string(), cache);
            }
        }

        // --- Pre-decode images (static assets, loaded once per render session) ---
        let mut images: HashMap<String, skia_safe::Image> = HashMap::new();
        for el in &template.elements {
            if let Element::Image(cfg) = el {
                if cfg.file.trim().is_empty() {
                    log::warn!(
                        "SceneCache: image element '{}' has no asset selected; skipping",
                        cfg.id
                    );
                    continue;
                }
                match load_asset_image(&cfg.file, assets_dirs) {
                    Some(img) => {
                        images.insert(cfg.id.clone(), img);
                    }
                    None => log::warn!(
                        "SceneCache: could not load image asset '{}' for element '{}'; skipping",
                        cfg.file,
                        cfg.id
                    ),
                }
            }
        }

        // --- Pre-render transparent base frame as a Skia Image ---
        let base_image = render_base_frame(w, h)?;

        Ok(SceneCache {
            base_image,
            charts,
            width: w,
            height: h,
            typefaces,
            images,
        })
    }
}

/// Rectangular sub-region of the full overlay frame, in overlay pixel coords.
/// When a render is cropped to the union of all visible elements, only this
/// window is rasterised + piped + encoded — the rest is fully transparent and
/// pure overhead. `x`/`y` is the placement offset the compositor needs.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CropRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// A supersampled sub-view of the scene, in the scene's own pixel coordinates.
/// Used by the zoomed preview: render only the visible window (`vx,vy,vw,vh`)
/// but rasterise it into an `out_w × out_h` surface — so vector draws (text) are
/// re-rasterised crisply at the zoomed resolution while cost stays bounded by the
/// viewport, not the zoom level. Pre-rasterised content (chart backgrounds,
/// images) is bitmap-scaled and so softens past its native resolution.
#[derive(Debug, Clone, Copy)]
pub struct ViewTransform {
    pub vx: f32,
    pub vy: f32,
    pub vw: f32,
    pub vh: f32,
    pub out_w: u32,
    pub out_h: u32,
}

/// Render a single video frame and return raw BGRA bytes.
///
/// `crop`: when `Some`, the surface is sized to the crop window and the canvas
/// is translated so all absolute-coordinate draws (base image, charts, text)
/// land correctly while only the window is captured. `None` = full frame.
///
/// `view`: when `Some`, takes precedence over `crop` — renders the supersampled
/// sub-view described above (zoomed preview path). The cache/template are used at
/// their existing (un-zoomed) scale; the canvas scale does the magnification.
pub fn render_frame(
    frame_idx: usize,
    cache: &SceneCache,
    activity: &Activity,
    template: &Template,
    crop: Option<&CropRect>,
    view: Option<&ViewTransform>,
) -> Vec<u8> {
    if let Some(v) = view {
        return render_view(frame_idx, cache, activity, template, v);
    }
    let (w, h, ox, oy) = match crop {
        Some(c) => (c.w as i32, c.h as i32, c.x, c.y),
        None => (cache.width as i32, cache.height as i32, 0, 0),
    };

    let info = ImageInfo::new(
        ISize::new(w, h),
        skia_safe::ColorType::BGRA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let row_bytes = (w * 4) as usize;

    // Composite straight into the output buffer: wrap a raster surface around
    // `pixels` so Skia draws in place. This drops both the surface's separate
    // backing-store allocation and the full-frame read_pixels copy the old
    // raster()+read_pixels path paid every frame. Render is fully hidden behind
    // encode, so this is a memory-traffic / allocator-churn win, not a speedup.
    // A fresh vec is zeroed = transparent BGRA; the base image covers the whole
    // crop window, so no explicit clear is needed.
    let mut pixels = vec![0u8; (h as usize) * row_bytes];
    {
        let mut surface =
            skia_safe::surfaces::wrap_pixels(&info, &mut pixels, Some(row_bytes), None)
                .expect("Skia surface");
        let canvas = surface.canvas();

        // Shift the world so the crop window maps to (0,0); all draw calls
        // below keep using absolute overlay coordinates unchanged.
        if ox != 0 || oy != 0 {
            canvas.translate((-ox as f32, -oy as f32));
        }

        // 1. Blit transparent base frame.
        //    Drawing an Image reference — no extra allocation or byte copy.
        canvas.draw_image(&cache.base_image, (0, 0), None);

        // 2. Draw all elements back-to-front according to scene.layers.
        let ctx = ElementCtx {
            activity,
            scene: &template.scene,
            typefaces: &cache.typefaces,
            charts: &cache.charts,
            fonts_dir: "",
            images: &cache.images,
        };
        for idx in template.layer_order() {
            if let Some(el) = template.elements.get(idx) {
                el.as_overlay().draw(canvas, &ctx, frame_idx);
            }
        }
    } // surface dropped here → releases the &mut pixels borrow

    pixels
}

/// Render a supersampled sub-view (zoomed preview). Surface is `out_w × out_h`;
/// the canvas is scaled so the scene window `(vx,vy,vw,vh)` fills it, then drawn
/// with absolute scene coordinates unchanged. Text re-rasterises crisply at the
/// magnified scale; the (transparent) base image is skipped — the zeroed buffer
/// already provides transparency.
fn render_view(
    frame_idx: usize,
    cache: &SceneCache,
    activity: &Activity,
    template: &Template,
    v: &ViewTransform,
) -> Vec<u8> {
    let w = v.out_w.max(1) as i32;
    let h = v.out_h.max(1) as i32;
    let info = ImageInfo::new(
        ISize::new(w, h),
        skia_safe::ColorType::BGRA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let row_bytes = (w * 4) as usize;
    let mut pixels = vec![0u8; (h as usize) * row_bytes];
    {
        let mut surface =
            skia_safe::surfaces::wrap_pixels(&info, &mut pixels, Some(row_bytes), None)
                .expect("Skia surface");
        let canvas = surface.canvas();

        // Map the scene window onto the output surface: scale so vw→out_w, then
        // shift the window origin to (0,0). Draw calls keep absolute coords.
        let sx = v.out_w as f32 / v.vw.max(1.0);
        let sy = v.out_h as f32 / v.vh.max(1.0);
        canvas.scale((sx, sy));
        canvas.translate((-v.vx, -v.vy));

        let ctx = ElementCtx {
            activity,
            scene: &template.scene,
            typefaces: &cache.typefaces,
            charts: &cache.charts,
            fonts_dir: "",
            images: &cache.images,
        };
        for idx in template.layer_order() {
            if let Some(el) = template.elements.get(idx) {
                el.as_overlay().draw(canvas, &ctx, frame_idx);
            }
        }
    }
    pixels
}

/// Union bounding box of every element that is ever drawn, across all frames.
///
/// Static elements (plots/labels) are measured once; dynamic elements (value
/// text changes width frame-to-frame) are measured at every frame (cheap: one
/// cached `Font` per config via load_font). The box is padded, clamped to the
/// frame, and rounded to even dimensions. Returns `None` when the box covers
/// ≥95% of the frame (cropping wouldn't pay off) or there is nothing to draw —
/// callers fall back to the full-frame path.
pub fn compute_crop_rect(
    activity: &Activity,
    template: &Template,
    fonts_dir: &str,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> Option<CropRect> {
    let fw = template.scene.width as f32;
    let fh = template.scene.height as f32;

    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut acc = |x0: f32, y0: f32, x1: f32, y1: f32| {
        min_x = min_x.min(x0);
        min_y = min_y.min(y0);
        max_x = max_x.max(x1);
        max_y = max_y.max(y1);
    };

    let charts: HashMap<String, ChartCache> = HashMap::new();
    let mut typefaces: HashMap<String, Typeface> = HashMap::new();
    for font_name in template
        .elements
        .iter()
        .flat_map(|e| e.as_overlay().fonts(&template.scene))
    {
        if let std::collections::hash_map::Entry::Vacant(e) = typefaces.entry(font_name.clone())
            && let Some(tf) = load_typeface(&font_name, fonts_dir)
        {
            e.insert(tf);
        }
    }
    let images: HashMap<String, skia_safe::Image> = HashMap::new();
    let ctx = ElementCtx {
        activity,
        scene: &template.scene,
        typefaces: &typefaces,
        charts: &charts,
        fonts_dir,
        images: &images,
    };

    let n = activity.data_len();
    for el in &template.elements {
        let ov = el.as_overlay();
        if ov.is_dynamic() {
            for i in 0..n {
                if i % 1024 == 0
                    && cancelled
                        .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
                        .unwrap_or(false)
                {
                    return None;
                }
                if let Some((x0, y0, x1, y1)) = ov.crop_extent(&ctx, i) {
                    acc(x0, y0, x1, y1);
                }
            }
        } else if let Some((x0, y0, x1, y1)) = ov.crop_extent(&ctx, 0) {
            acc(x0, y0, x1, y1);
        }
    }

    if !min_x.is_finite() || !max_x.is_finite() || max_x <= min_x || max_y <= min_y {
        return None;
    }

    // Pad, clamp to frame, round origin down / extent up to even dimensions.
    const PAD: f32 = 16.0;
    let x0 = (min_x - PAD).floor().max(0.0);
    let y0 = (min_y - PAD).floor().max(0.0);
    let x1 = (max_x + PAD).ceil().min(fw);
    let y1 = (max_y + PAD).ceil().min(fh);

    let x = x0 as i32 & !1;
    let y = y0 as i32 & !1;
    let w = (((x1 as i32) - x).max(2) as u32 + 1) & !1;
    let h = (((y1 as i32) - y).max(2) as u32 + 1) & !1;
    let w = w.min(template.scene.width - x as u32);
    let h = h.min(template.scene.height - y as u32);

    // Not worth the contract change if the box is essentially the whole frame.
    if (w as f32 * h as f32) >= 0.95 * fw * fh {
        return None;
    }

    Some(CropRect { x, y, w, h })
}

// ─── Base frame pre-renderer ───────────────────────────────────────────────

fn render_base_frame(w: u32, h: u32) -> Result<skia_safe::Image, String> {
    let info = ImageInfo::new(
        ISize::new(w as i32, h as i32),
        skia_safe::ColorType::BGRA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None)
        .ok_or("Failed to create base frame surface")?;
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);

    Ok(surface.image_snapshot())
}

// ─── Font loading ──────────────────────────────────────────────────────────

/// User-installed custom fonts directory, injected by the host application at
/// startup (the core crate has no notion of per-platform app-data paths).
static USER_FONTS_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn set_user_fonts_dir(dir: std::path::PathBuf) {
    let _ = USER_FONTS_DIR.set(dir);
}

pub(crate) fn load_typeface(font_name: &str, fonts_dir: &str) -> Option<Typeface> {
    let mgr = FontMgr::default();
    // Bundled fonts first, then user-installed custom fonts.
    let mut candidates = vec![std::path::PathBuf::from(format!("{fonts_dir}/{font_name}"))];
    if let Some(user_dir) = USER_FONTS_DIR.get() {
        candidates.push(user_dir.join(font_name));
    }
    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            let data = skia_safe::Data::new_copy(&bytes);
            if let Some(tf) = mgr.new_from_data(&data, None) {
                return Some(tf);
            }
        }
    }
    // System font fallback: try the font name as a family, then common Linux
    // families (Arial is not typically installed on Linux).
    let primary = font_name
        .trim_end_matches(".ttf")
        .trim_end_matches(".TTF")
        .trim_end_matches(".otf")
        .trim_end_matches(".OTF");
    let fallbacks: &[&str] = if cfg!(target_os = "linux") {
        &[
            primary,
            "DejaVu Sans",
            "Liberation Sans",
            "Noto Sans",
            "Ubuntu",
            "FreeSans",
            "sans-serif",
        ]
    } else {
        &[primary]
    };
    for family in fallbacks {
        if let Some(tf) = mgr.match_family_style(family, FontStyle::normal()) {
            return Some(tf);
        }
    }
    None
}

pub(crate) fn font_from_typeface(typeface: Typeface, size: f32, italic: bool) -> Font {
    let mut font = Font::new(typeface, size);
    if italic {
        font.set_skew_x(ITALIC_SKEW_X);
    }
    font
}

fn load_font(font_name: &str, size: f32, fonts_dir: &str, italic: bool) -> Option<Font> {
    load_typeface(font_name, fonts_dir).map(|tf| font_from_typeface(tf, size, italic))
}

/// Adjust a base x coordinate for text alignment.
/// `base_x` is the authored position; returns the draw-origin x for Skia (left edge of glyphs).
fn align_x(base_x: f32, text_width: f32, align: Option<&str>) -> f32 {
    match align.unwrap_or("left") {
        "right" => base_x - text_width,
        "center" => base_x - text_width / 2.0,
        _ => base_x, // "left" default
    }
}

/// Convert an authored y into the Skia baseline per `vertical_align`.
/// Uses font metrics (not per-frame glyph bounds) so the baseline stays put
/// as the text changes. "middle" centers on cap height, which optically
/// centers digits — the dominant case for metric values.
fn align_baseline_y(base_y: f32, font: &Font, align: Option<&str>) -> f32 {
    let (_, metrics) = font.metrics();
    // cap_height is 0 when the typeface doesn't report it — approximate with ascent.
    let cap = if metrics.cap_height > 0.0 {
        metrics.cap_height
    } else {
        -metrics.ascent
    };
    match align.unwrap_or("baseline") {
        "top" => base_y + cap,
        "middle" => base_y + cap / 2.0,
        "bottom" => base_y - metrics.descent,
        _ => base_y, // "baseline" default
    }
}

fn measure_text_with_letter_spacing(
    font: &Font,
    text: &str,
    paint: Option<&Paint>,
    letter_spacing: f32,
) -> (f32, skia_safe::Rect) {
    let (base_width, mut rect) = font.measure_str(text, paint);
    if letter_spacing == 0.0 {
        return (base_width, rect);
    }

    let gaps = text.chars().count().saturating_sub(1) as f32;
    let width = base_width + letter_spacing * gaps;
    rect.right = rect.left + width;
    (width, rect)
}

fn draw_text_with_letter_spacing(
    canvas: &Canvas,
    text: &str,
    origin: (f32, f32),
    font: &Font,
    paint: &Paint,
    letter_spacing: f32,
) {
    if letter_spacing == 0.0 {
        canvas.draw_str(text, origin, font, paint);
        return;
    }

    let (mut x, y) = origin;
    for ch in text.chars() {
        let s = ch.to_string();
        canvas.draw_str(&s, (x, y), font, paint);
        x += font.measure_str(&s, Some(paint)).0 + letter_spacing;
    }
}

// ─── Tabular figures (fixed-width digits) ───────────────────────────────────
// A value that changes each frame reflows because proportional digits have
// different advances ('1' is narrow, '0' wide), so the string width — and any
// trailing suffix — jitters. Tabular rendering gives every ASCII digit the
// advance of the widest digit (centering the glyph in that cell), leaving other
// glyphs natural. `measure_tabular_str` mirrors `draw_tabular_str`'s layout
// exactly so alignment stays correct.

/// The tabular cell width: the widest advance among ASCII digits `0`–`9`.
fn digit_cell_width(font: &Font, paint: Option<&Paint>) -> f32 {
    let mut cell = 0.0f32;
    for d in b'0'..=b'9' {
        let adv = font
            .measure_str((d as char).encode_utf8(&mut [0u8; 1]), paint)
            .0;
        if adv > cell {
            cell = adv;
        }
    }
    cell
}

/// Width of `text` with each ASCII digit occupying `cell` and every other glyph
/// its natural advance.
fn measure_tabular_str(font: &Font, text: &str, paint: Option<&Paint>, cell: f32) -> f32 {
    let mut buf = [0u8; 4];
    text.chars().fold(0.0f32, |w, ch| {
        if ch.is_ascii_digit() {
            w + cell
        } else {
            w + font.measure_str(ch.encode_utf8(&mut buf), paint).0
        }
    })
}

/// Draw `text` with each ASCII digit centered in a fixed `cell` advance and
/// other glyphs at natural width.
fn draw_tabular_str(
    canvas: &Canvas,
    text: &str,
    origin: (f32, f32),
    font: &Font,
    paint: &Paint,
    cell: f32,
) {
    let (mut x, y) = origin;
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        let s = ch.encode_utf8(&mut buf);
        let adv = font.measure_str(&*s, Some(paint)).0;
        if ch.is_ascii_digit() {
            // Center the digit glyph within its uniform cell.
            canvas.draw_str(&*s, (x + (cell - adv) / 2.0, y), font, paint);
            x += cell;
        } else {
            canvas.draw_str(&*s, (x, y), font, paint);
            x += adv;
        }
    }
}

// ─── Value formatting ──────────────────────────────────────────────────────

fn format_value(raw: f64, cfg: &ValueConfig, gate_configured: bool) -> String {
    let suffix = resolve_suffix(cfg);
    let append = |text: String| match &suffix {
        Some(s) => format!("{text}{s}"),
        None => text,
    };

    // Race countdown never reads "0" (broadcast style): the bell lap shows
    // LAST LAP and everything after the finish crossing shows FINISH, both
    // replacing the number and its suffix. Without a configured gate the
    // metric stays a plain numeric 0 so the editor state is honest.
    if cfg.value == LAP_LAPS_TO_GO && gate_configured {
        return match raw.round() as i64 {
            n if n <= 0 => "FINISH".to_string(),
            1 => "LAST LAP".to_string(),
            n => append(n.to_string()),
        };
    }

    if cfg.value == ATTR_GEAR {
        let text = decode_gear(raw)
            .map(|(front, rear)| format!("{front}x{rear}"))
            .unwrap_or_else(|| "0x0".to_string());
        return append(text);
    }

    if cfg.value == LAP_FRACTION {
        let (lap, total) = decode_lap_fraction(raw);
        return append(format!("{lap}/{total}"));
    }

    if cfg.value == ATTR_TIME || cfg.value == SUM_TOTAL_TIME || cfg.value == RUN_TIME {
        let text = format_time(
            raw,
            cfg.time_format.as_deref(),
            cfg.decimal_rounding,
            cfg.time_12h.unwrap_or(false),
            cfg.time_ampm.unwrap_or(false),
        );
        return append(text);
    }

    // Convert from the GPX-native unit. Summary metrics (e.g. total_distance,
    // elevation_gain) borrow their base metric's unit table so km/mi/ft all
    // work exactly as on the live value. Value elements do not auto-append a
    // unit suffix; the optional manual `suffix` field is applied below.
    let unit_attr = unit_base_metric(&cfg.value);
    let (conv, _) = units::resolve(unit_attr, cfg.unit.as_deref());
    let v = conv.apply(raw);

    // Decimal rounding. W/kg is fractional by nature, so it defaults to one
    // decimal place when the template doesn't specify a rounding.
    let default_decimals = if cfg.value == ATTR_POWER_TO_WEIGHT {
        1
    } else {
        0
    };
    let decimals = cfg
        .decimal_rounding
        .filter(|&n| n >= 0)
        .unwrap_or(default_decimals);
    let text = if decimals == 0 {
        format!("{}", v.round() as i64)
    } else {
        format!("{:.prec$}", v, prec = decimals as usize)
    };

    append(text)
}

/// Resolve a value element's trailing suffix per its `suffix_mode`:
/// - `"none"` → no suffix
/// - `"auto"` → unit-derived label that tracks the unit picker (e.g. " km",
///   " mph", " W", " bpm"); summary metrics borrow their base metric's label
/// - `"custom"` or absent → the manual `suffix` string if any (absent keeps
///   pre-`suffix_mode` templates rendering their manual suffix unchanged)
fn resolve_suffix(cfg: &ValueConfig) -> Option<String> {
    match cfg.suffix_mode.as_deref() {
        Some("none") => None,
        Some("auto") => {
            let unit_attr = unit_base_metric(&cfg.value);
            units::display_suffix(unit_attr, cfg.unit.as_deref())
        }
        _ => cfg.suffix.clone().filter(|s| !s.is_empty()),
    }
}

/// Format raw seconds into a human-readable string.
/// `fmt`: "hh:mm:ss" | "hh:mm" | "mm:ss" | "h m" | "h" | "m" | "s" (default).
/// `twelve_hour`: convert 24h → 12h for clock formats.
/// `show_ampm`: append " AM"/" PM" suffix (only when `twelve_hour` is true).
fn format_time(
    raw: f64,
    fmt: Option<&str>,
    rounding: Option<i32>,
    twelve_hour: bool,
    show_ampm: bool,
) -> String {
    match fmt.unwrap_or("hh:mm:ss") {
        "hh:mm:ss" | "hms" => {
            let secs = raw.abs() as i64;
            let h24 = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            let (h, ampm) = clock_hour(h24, twelve_hour, show_ampm);
            if h24 >= 1 || twelve_hour {
                format!("{h}:{m:02}:{s:02}{ampm}")
            } else {
                format!("{m}:{s:02}")
            }
        }
        "hh:mm" => {
            let secs = raw.abs() as i64;
            let h24 = secs / 3600;
            let m = (secs % 3600) / 60;
            let (h, ampm) = clock_hour(h24, twelve_hour, show_ampm);
            format!("{h}:{m:02}{ampm}")
        }
        "mm:ss" | "ms" => {
            let secs = raw.abs() as i64;
            let m = secs / 60;
            let s = secs % 60;
            format!("{m}:{s:02}")
        }
        "h m" | "hm" => {
            // Compact "2h 34m"; drop the hours part entirely when it's zero.
            let secs = raw.abs() as i64;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            if h > 0 {
                format!("{h}h {m}m")
            } else {
                format!("{m}m")
            }
        }
        "h" => {
            let v = raw / 3600.0;
            match rounding {
                Some(n) if n > 0 => format!("{:.prec$}", v, prec = n as usize),
                _ => format!("{:.1}", v),
            }
        }
        "m" => {
            let v = raw / 60.0;
            match rounding {
                Some(n) if n > 0 => format!("{:.prec$}", v, prec = n as usize),
                _ => format!("{:.1}", v),
            }
        }
        _ => {
            // "s" or anything else — raw seconds
            match rounding {
                Some(0) => format!("{}", raw.round() as i64),
                Some(n) if n > 0 => format!("{:.prec$}", raw, prec = n as usize),
                _ => format!("{}", raw.round() as i64),
            }
        }
    }
}

/// Returns the display hour and optional AM/PM suffix string.
/// When `twelve_hour` is false, returns (h24, "").
fn clock_hour(h24: i64, twelve_hour: bool, show_ampm: bool) -> (i64, String) {
    if !twelve_hour {
        return (h24, String::new());
    }
    let ampm = if h24 % 24 < 12 { " AM" } else { " PM" };
    let h12 = h24 % 12;
    let h12 = if h12 == 0 { 12 } else { h12 };
    let suffix = if show_ampm { ampm } else { "" };
    (h12, suffix.to_string())
}

// ─── Image element ─────────────────────────────────────────────────────────

/// Beat-pulse envelope: quick attack, smooth quadratic decay. Returns 0..=1.
fn beat_curve(phase: f32) -> f32 {
    const ATTACK: f32 = 0.12;
    if phase < ATTACK {
        phase / ATTACK
    } else {
        ((1.0 - phase) / (1.0 - ATTACK)).powf(2.0)
    }
}

impl ImageConfig {
    fn pulse_scale(&self, ctx: &ElementCtx, frame_idx: usize) -> f32 {
        let amplitude = self.pulse_amplitude.unwrap_or(0.15);
        if amplitude <= 0.0 {
            return 1.0;
        }
        let bpm = if let Some(fixed) = self.pulse_bpm {
            fixed
        } else if let Some(metric) = self.pulse_metric.as_deref() {
            if ctx.activity.valid_attributes.contains(&metric.to_string()) {
                ctx.activity.get_scalar(metric, frame_idx)
            } else {
                return 1.0;
            }
        } else {
            return 1.0;
        };
        if bpm <= 0.0 {
            return 1.0;
        }
        let fps = ctx.scene.fps as f64;
        let time_sec = frame_idx as f64 / fps;
        let phase = ((time_sec * bpm / 60.0).fract() as f32).clamp(0.0, 1.0);
        1.0 + amplitude * beat_curve(phase)
    }
}

impl OverlayElement for ImageConfig {
    fn measure(&self, ctx: &ElementCtx, frame_idx: usize) -> Option<ElementBounds> {
        let max_scale = 1.0 + self.pulse_amplitude.unwrap_or(0.0).max(0.0);
        let w = self.width as f32 * max_scale;
        let h = self.height as f32 * max_scale;
        let cx = self.x as f32 + self.width as f32 / 2.0;
        let cy = self.y as f32 + self.height as f32 / 2.0;
        // Suppress unused warning — frame_idx not needed for measure but
        // signature must match the trait.
        let _ = (ctx, frame_idx);
        Some(ElementBounds {
            id: self.id.clone(),
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
        })
    }

    fn draw(&self, canvas: &Canvas, ctx: &ElementCtx, frame_idx: usize) {
        let Some(img) = ctx.images.get(&self.id) else {
            return;
        };
        let opacity = self.opacity.unwrap_or(1.0);
        let rotation = self.rotation.unwrap_or(0.0);
        let w = self.width as f32;
        let h = self.height as f32;
        let cx = self.x as f32 + w / 2.0;
        let cy = self.y as f32 + h / 2.0;
        let scale = self.pulse_scale(ctx, frame_idx);

        let needs_transform = rotation != 0.0 || scale != 1.0;
        if needs_transform {
            canvas.save();
            if scale != 1.0 {
                canvas.translate((cx, cy));
                canvas.scale((scale, scale));
                canvas.translate((-cx, -cy));
            }
            if rotation != 0.0 {
                canvas.rotate(rotation, Some(skia_safe::Point::new(cx, cy)));
            }
        }

        let dst = Rect::from_xywh(self.x as f32, self.y as f32, w, h);
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_alpha_f(opacity);
        let sampling = skia_safe::SamplingOptions::new(
            skia_safe::FilterMode::Linear,
            skia_safe::MipmapMode::None,
        );
        canvas.draw_image_rect_with_sampling_options(img, None, dst, sampling, &paint);

        if needs_transform {
            canvas.restore();
        }
    }
}

/// Resolve an asset filename against an ordered list of search directories.
/// Checks absolute path first, then each directory in order.
pub fn resolve_asset_path(file: &str, search_dirs: &[&str]) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(file);
    if p.is_absolute() && p.exists() {
        return Some(p.to_path_buf());
    }
    let basename = p.file_name().unwrap_or(p.as_os_str());
    for dir in search_dirs {
        let candidate = std::path::Path::new(dir).join(basename);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Load a PNG/WebP/SVG asset into a Skia Image, searching the given directories.
pub(crate) fn load_asset_image(file: &str, search_dirs: &[&str]) -> Option<skia_safe::Image> {
    if file.trim().is_empty() {
        return None;
    }
    let path = resolve_asset_path(file, search_dirs)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "svg" {
        load_svg_image(&path)
    } else if ext == "webp" {
        load_webp_image(&path)
    } else {
        let bytes = std::fs::read(&path).ok()?;
        let data = skia_safe::Data::new_copy(&bytes);
        skia_safe::Image::from_encoded(data)
    }
}

fn load_webp_image(path: &std::path::Path) -> Option<skia_safe::Image> {
    let bytes = std::fs::read(path).ok()?;
    let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return None;
    }

    let mut decoded = vec![0; decoder.output_buffer_size()?];
    decoder.read_image(&mut decoded).ok()?;

    let rgba = if decoder.has_alpha() {
        decoded
    } else {
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        for px in decoded.chunks_exact(3) {
            out.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
        out
    };

    let info = ImageInfo::new(
        ISize::new(w as i32, h as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let data = skia_safe::Data::new_copy(&rgba);
    skia_safe::images::raster_from_data(&info, data, (w * 4) as usize)
}

fn load_svg_image(path: &std::path::Path) -> Option<skia_safe::Image> {
    let content = std::fs::read_to_string(path).ok()?;
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(&content, &opt).ok()?;
    let size = tree.size().to_int_size();
    let (w, h) = (size.width(), size.height());
    if w == 0 || h == 0 {
        return None;
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    let png_bytes = pixmap.encode_png().ok()?;
    let data = skia_safe::Data::new_copy(&png_bytes);
    skia_safe::Image::from_encoded(data)
}

#[cfg(test)]
mod tests {
    use crate::template::Template;

    /// The laps_to_go countdown never displays "0" during a race: the bell
    /// lap reads LAST LAP and anything past the finish crossing reads FINISH
    /// (both drop the manual suffix). Numbers ≥ 2 keep the suffix, and with
    /// no gate configured the raw 0 stays numeric so the editor is honest.
    #[test]
    fn laps_to_go_formats_last_lap_and_finish_without_zero() {
        use crate::template::Element;

        let raw = serde_json::json!({
            "scene": { "width": 100, "height": 100 },
            "elements": [
                { "type": "value", "id": "v", "value": "laps_to_go",
                  "suffix": " to go", "x": 0, "y": 0 }
            ]
        });
        let t = Template::from_value(raw).unwrap();
        let Element::Value(cfg) = &t.elements[0] else {
            panic!("expected value element")
        };

        assert_eq!(super::format_value(21.0, cfg, true), "21 to go");
        assert_eq!(super::format_value(2.0, cfg, true), "2 to go");
        assert_eq!(super::format_value(1.0, cfg, true), "LAST LAP");
        assert_eq!(super::format_value(0.0, cfg, true), "FINISH");
        // No gate yet: the raw 0 renders as a plain number, not FINISH.
        assert_eq!(super::format_value(0.0, cfg, false), "0 to go");
    }

    /// Lap counters resolve off the gate pre-pass, never a source attribute —
    /// they are absent from `valid_attributes`, and the value draw gate must
    /// still render them (regression: laps_to_go drew blank in the editor).
    #[test]
    fn lap_metric_value_draws_without_source_attribute() {
        use crate::activity::Activity;

        let activity = Activity {
            laps_completed: vec![2.0; 10],
            total_laps: 5.0,
            ..Activity::default()
        };
        // valid_attributes deliberately left empty.

        let raw = serde_json::json!({
            "scene": { "width": 200, "height": 100, "font": "rajdhani-bold.ttf" },
            "elements": [
                { "type": "value", "id": "v", "value": "laps_to_go",
                  "font_size": 60, "x": 10, "y": 80 }
            ]
        });
        let t = Template::from_value(raw).unwrap();
        let fonts_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/fonts");
        let cache = super::SceneCache::build(&activity, &t, fonts_dir, &[]).unwrap();
        let rgba = super::render_frame(0, &cache, &activity, &t, None, None);
        assert!(
            rgba.chunks_exact(4).any(|px| px[3] != 0),
            "laps_to_go value element rendered nothing"
        );
    }

    /// End-to-end `color_by`: banded elevation fill, banded line plot, and a
    /// banded course line (with past/future split) render through
    /// SceneCache::build → render_frame, each producing several distinct band
    /// colors inside its rect. Set CYCLEMETRY_TEST_DUMP=<dir> to write a PNG.
    #[test]
    fn color_by_renders_banded_plots_end_to_end() {
        use super::{SceneCache, render_frame};
        use crate::activity::Activity;
        use crate::template::DEFAULT_GRADIENT_BANDS;

        let mut activity = Activity::synthetic(600);
        // Widen the synthetic gradient (±3%) so the sweep crosses most of the
        // default bands (−12%…12%).
        activity.gradient = (0..600).map(|i| 12.0 * (i as f64 / 40.0).cos()).collect();

        let raw = serde_json::json!({
            "scene": { "width": 1280, "height": 720 },
            "elements": [
                { "type": "rect", "id": "bg", "x": 0, "y": 0, "width": 1280, "height": 720,
                  "color": "#18181b" },
                { "type": "plot", "id": "profile", "value": "elevation",
                  "x": 40, "y": 420, "width": 560, "height": 240,
                  "line": { "color": "#ffffff", "width": 3 },
                  "color_by": { "value": "gradient" } },
                { "type": "plot", "id": "line-plot", "value": "elevation",
                  "x": 680, "y": 420, "width": 560, "height": 240,
                  "line": { "width": 6 },
                  "color_by": { "value": "gradient", "mode": "line" } },
                { "type": "plot", "id": "course", "value": "course",
                  "x": 40, "y": 40, "width": 560, "height": 320,
                  "line": { "width": 8, "past_opacity": 1.0, "future_opacity": 0.25 },
                  "point": { "color": "#ffffff", "weight": 300 },
                  "color_by": { "value": "gradient" } }
            ]
        });
        let mut template = Template::from_value(raw).unwrap();
        // Sample like the real pipeline does — course plots draw the
        // source-density `route`, which is populated at sample time.
        template.scene.fps = 1;
        let activity = activity.sample_for_scene(&template.scene, true).unwrap();
        let cache = SceneCache::build(&activity, &template, "/nonexistent", &[]).unwrap();
        let frame = render_frame(300, &cache, &activity, &template, None, None);

        // Count which band colors appear (exact matches only — interiors of
        // opaque banded draws) within a rect of the BGRA frame.
        let bands_in_rect = |x0: usize, y0: usize, w: usize, h: usize| -> usize {
            DEFAULT_GRADIENT_BANDS
                .iter()
                .filter(|(_, hex)| {
                    let (r, g, b, _) = crate::color::parse_hex_color(hex);
                    (y0..y0 + h).any(|y| {
                        (x0..x0 + w).any(|x| {
                            let i = (y * 1280 + x) * 4;
                            frame[i] == b && frame[i + 1] == g && frame[i + 2] == r
                        })
                    })
                })
                .count()
        };
        assert!(bands_in_rect(40, 420, 560, 240) >= 4, "banded fill");
        assert!(bands_in_rect(680, 420, 560, 240) >= 4, "banded line");
        // Course: only the traveled half is opaque (future is at 0.25 alpha).
        assert!(bands_in_rect(40, 40, 560, 320) >= 3, "banded course line");

        if let Ok(dir) = std::env::var("CYCLEMETRY_TEST_DUMP") {
            let info = skia_safe::ImageInfo::new(
                skia_safe::ISize::new(1280, 720),
                skia_safe::ColorType::BGRA8888,
                skia_safe::AlphaType::Premul,
                None,
            );
            let data = skia_safe::Data::new_copy(&frame);
            let img = skia_safe::images::raster_from_data(&info, data, 1280 * 4).unwrap();
            let png = img
                .encode(None, skia_safe::EncodedImageFormat::PNG, None)
                .unwrap();
            std::fs::write(format!("{dir}/color_by_e2e.png"), png.as_bytes()).unwrap();
        }
    }

    /// Mirrors the native_demo preview path when the user drags the timeline
    /// trim: scaled template parse → sample_for_scene over a changed window →
    /// SceneCache::build → render_frame, with color_by plots present.
    #[test]
    fn time_range_change_rebuilds_color_by_preview() {
        use super::{SceneCache, render_frame};
        use crate::activity::Activity;

        let source = Activity::synthetic(600);
        let raw = serde_json::json!({
            "scene": { "width": 3840, "height": 2160, "start": 60.0, "end": 240.0 },
            "elements": [
                { "type": "plot", "id": "profile", "value": "elevation",
                  "x": 100, "y": 1200, "width": 1600, "height": 700,
                  "line": { "color": "#ffffff", "width": 8 },
                  "point": { "color": "#ffffff", "weight": 900 },
                  "color_by": { "value": "gradient" } },
                { "type": "plot", "id": "course", "value": "course",
                  "x": 2000, "y": 200, "width": 1500, "height": 900,
                  "line": { "width": 20, "past_opacity": 1.0, "future_opacity": 0.3 },
                  "color_by": { "value": "gradient", "bands": [
                      { "max": 0, "color": "#3b82f6" }, { "color": "#dc2626" } ] } }
            ]
        });

        // The preview re-runs this whole flow for every new trim window.
        for (start, end) in [(60.0, 240.0), (10.0, 590.0), (300.0, 302.0), (0.0, 600.0)] {
            let mut config = raw.clone();
            config["scene"]["start"] = serde_json::json!(start);
            config["scene"]["end"] = serde_json::json!(end);
            let mut template =
                crate::template::Template::from_value_scaled(config, Some((1280, 720))).unwrap();
            template.scene.fps = 10;
            template.scene.target_duration = None;
            let activity = source.sample_for_scene(&template.scene, true).unwrap();
            let cache = SceneCache::build(&activity, &template, "/nonexistent", &[]).unwrap();
            assert_eq!(cache.charts.len(), 2, "window {start}..{end}");
            let last = activity.data_len().saturating_sub(1);
            for idx in [0, last / 2, last] {
                let frame = render_frame(idx, &cache, &activity, &template, None, None);
                assert_eq!(frame.len(), 1280 * 720 * 4);
            }
        }
    }

    /// Unified model: explicit `scene.layers` ids drive draw order; elements
    /// not listed fall back to array order after the listed ones.
    #[test]
    fn layer_order_honors_explicit_ids_then_array_order() {
        let raw = serde_json::json!({
            "scene": { "width": 100, "height": 100, "layers": ["plot-0", "label-0"] },
            "elements": [
                { "type": "label", "id": "label-0", "text": "A", "x": 0, "y": 0 },
                { "type": "value", "id": "value-0", "value": "speed", "x": 0, "y": 0 },
                { "type": "plot", "id": "plot-0", "value": "elevation",
                  "x": 0, "y": 0, "width": 10, "height": 10 }
            ]
        });
        let t = Template::from_value(raw).unwrap();
        // plot-0 (idx 2), label-0 (idx 0) listed first; value-0 (idx 1) trails.
        assert_eq!(t.layer_order(), vec![2, 0, 1]);
    }

    /// W/kg = power ÷ rider weight, computed from the render-time weight (never
    /// stored in the template). With no weight it falls back to 0.0, and it
    /// defaults to one decimal place.
    #[test]
    fn power_to_weight_divides_power_by_rider_weight() {
        use crate::activity::{ATTR_POWER, ATTR_POWER_TO_WEIGHT, Activity};
        use crate::template::Element;

        let activity = Activity {
            power: vec![300.0],
            speed: vec![10.0],
            valid_attributes: vec![ATTR_POWER.into(), ATTR_POWER_TO_WEIGHT.into()],
            ..Activity::default()
        };

        let raw = serde_json::json!({
            "scene": { "width": 100, "height": 100 },
            "elements": [
                { "type": "value", "id": "v", "value": "power_to_weight", "x": 0, "y": 0 }
            ]
        });
        let t = Template::from_value(raw).unwrap();
        let Element::Value(cfg) = &t.elements[0] else {
            panic!("expected a value element");
        };

        // No weight set yet → empty-state 0.0, formatted to one decimal.
        assert_eq!(
            super::format_value(cfg.sample(&activity, 0, None), cfg, false),
            "0.0"
        );
        // 300 W / 72 kg ≈ 4.2 W/kg.
        assert_eq!(
            super::format_value(cfg.sample(&activity, 0, Some(72.0)), cfg, false),
            "4.2"
        );
    }

    /// The compact "h m" format renders "2h 34m", truncates seconds, and drops
    /// the hours part when it is zero.
    #[test]
    fn time_format_h_m_is_compact() {
        assert_eq!(
            super::format_time(
                2.0 * 3600.0 + 34.0 * 60.0 + 59.0,
                Some("h m"),
                None,
                false,
                false
            ),
            "2h 34m"
        );
        assert_eq!(
            super::format_time(34.0 * 60.0, Some("h m"), None, false, false),
            "34m"
        );
        assert_eq!(
            super::format_time(0.0, Some("h m"), None, false, false),
            "0m"
        );
    }

    /// Summary metrics resolve to a constant value from the precomputed
    /// summaries, honor `summary_scope`, and format through their base metric's
    /// unit/time rules (km suffix, elevation metres, hh:mm:ss).
    #[test]
    fn summary_metrics_format_via_base_metric() {
        use crate::activity::{
            ATTR_DISTANCE, ATTR_ELEVATION, ATTR_TIME, Activity, ActivitySummary,
        };
        use crate::template::Element;

        let full = ActivitySummary {
            total_distance: 42_195.0, // metres
            total_time: 3661.0,       // 1:01:01
            elevation_gain: 512.0,
            ..Default::default()
        };
        let clip = ActivitySummary {
            total_distance: 1000.0,
            ..Default::default()
        };
        let activity = Activity {
            valid_attributes: vec![
                ATTR_DISTANCE.into(),
                ATTR_ELEVATION.into(),
                ATTR_TIME.into(),
            ],
            activity_summary: full,
            overlay_summary: clip,
            ..Activity::default()
        };

        let raw = serde_json::json!({
            "scene": { "width": 100, "height": 100 },
            "elements": [
                { "type": "value", "id": "d", "value": "total_distance",
                  "unit": "km", "suffix": " km", "decimal_rounding": 1, "x": 0, "y": 0 },
                { "type": "value", "id": "e", "value": "elevation_gain",
                  "unit": "m", "suffix": " m", "x": 0, "y": 0 },
                { "type": "value", "id": "t", "value": "total_time",
                  "time_format": "hh:mm:ss", "x": 0, "y": 0 },
                { "type": "value", "id": "d2", "value": "total_distance",
                  "summary_scope": "overlay", "unit": "m", "x": 0, "y": 0 }
            ]
        });
        let t = Template::from_value(raw).unwrap();
        let fmt = |i: usize| {
            let Element::Value(cfg) = &t.elements[i] else {
                panic!("expected value element")
            };
            super::format_value(cfg.sample(&activity, 0, None), cfg, false)
        };

        assert_eq!(fmt(0), "42.2 km"); // whole activity, km, 1 decimal
        assert_eq!(fmt(1), "512 m"); // elevation gain in metres
        assert_eq!(fmt(2), "1:01:01"); // total time hh:mm:ss
        assert_eq!(fmt(3), "1000"); // overlay-scoped distance, metres, no suffix
    }

    /// Running counters resolve per-frame and format through their base metric:
    /// running_time as a clock, running_distance in km, running_elevation_gain
    /// in metres. They also hide when the base telemetry attribute is absent.
    #[test]
    fn running_metrics_resolve_and_format_via_base_metric() {
        use crate::activity::{ATTR_DISTANCE, ATTR_ELEVATION, ATTR_TIME, Activity};
        use crate::template::Element;

        let activity = Activity {
            elapsed_seconds: vec![0.0, 61.0, 3661.0],
            distance: vec![0.0, 1000.0, 5000.0],
            cumulative_elevation_gain: vec![0.0, 100.0, 250.0],
            valid_attributes: vec![
                ATTR_DISTANCE.into(),
                ATTR_ELEVATION.into(),
                ATTR_TIME.into(),
            ],
            ..Activity::default()
        };

        let raw = serde_json::json!({
            "scene": { "width": 100, "height": 100 },
            "elements": [
                { "type": "value", "id": "t", "value": "running_time",
                  "time_format": "hh:mm:ss", "x": 0, "y": 0 },
                { "type": "value", "id": "d", "value": "running_distance",
                  "unit": "km", "suffix_mode": "auto", "decimal_rounding": 1, "x": 0, "y": 0 },
                { "type": "value", "id": "e", "value": "running_elevation_gain",
                  "unit": "m", "suffix_mode": "auto", "x": 0, "y": 0 }
            ]
        });
        let t = Template::from_value(raw).unwrap();
        let fmt = |i: usize, frame: usize| {
            let Element::Value(cfg) = &t.elements[i] else {
                panic!("expected value element")
            };
            super::format_value(cfg.sample(&activity, frame, None), cfg, false)
        };

        // Frame 2: 3661 s → 1:01:01, 5000 m → 5.0 km, 250 m climbed.
        assert_eq!(fmt(0, 2), "1:01:01");
        assert_eq!(fmt(1, 2), "5.0 km");
        assert_eq!(fmt(2, 2), "250 m");
        // Frame 1 reads the mid-point running totals.
        assert_eq!(fmt(2, 1), "100 m");

        // Without elevation telemetry, running_elevation_gain reads 0.
        let no_elev = Activity {
            valid_attributes: vec![ATTR_DISTANCE.into(), ATTR_TIME.into()],
            ..activity.clone()
        };
        let Element::Value(cfg) = &t.elements[2] else {
            panic!("expected value element")
        };
        assert_eq!(cfg.sample(&no_elev, 1, None), 0.0);
    }

    /// Tabular figures give every same-length number an identical width, so a
    /// counting value (and its trailing suffix) doesn't reflow frame to frame.
    #[test]
    fn tabular_figures_keep_equal_length_numbers_equal_width() {
        let fonts_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/fonts");
        let font = super::load_font("rajdhani-bold.ttf", 100.0, fonts_dir, false)
            .expect("bundled font should load");
        let cell = super::digit_cell_width(&font, None);
        assert!(cell > 0.0);

        // Same digit count → identical tabular width regardless of the digits…
        let a = super::measure_tabular_str(&font, "100", None, cell);
        let b = super::measure_tabular_str(&font, "111", None, cell);
        assert!((a - b).abs() < 1e-3, "tabular widths differ: {a} vs {b}");

        // …and the whole string (digits + suffix) stays put as the value changes.
        let d1 = super::measure_tabular_str(&font, "1.2 mi", None, cell);
        let d2 = super::measure_tabular_str(&font, "9.8 mi", None, cell);
        assert!((d1 - d2).abs() < 1e-3, "suffix reflows: {d1} vs {d2}");
    }

    /// suffix_mode: "auto" tracks the unit (and summary base metric), "none"
    /// drops any suffix, "custom"/absent keep the manual `suffix` verbatim.
    #[test]
    fn suffix_mode_resolves_auto_none_and_custom() {
        use crate::activity::{ATTR_DISTANCE, ATTR_SPEED, Activity, ActivitySummary};
        use crate::template::Element;

        let activity = Activity {
            speed: vec![10.0], // 10 m/s
            valid_attributes: vec![ATTR_SPEED.into(), ATTR_DISTANCE.into()],
            activity_summary: ActivitySummary {
                total_distance: 5000.0,
                ..Default::default()
            },
            ..Activity::default()
        };

        let raw = serde_json::json!({
            "scene": { "width": 100, "height": 100 },
            "elements": [
                // auto on a live metric follows the unit token
                { "type": "value", "id": "a", "value": "speed",
                  "unit": "mph", "suffix_mode": "auto", "x": 0, "y": 0 },
                // auto on a summary metric borrows the base metric's label
                { "type": "value", "id": "b", "value": "total_distance",
                  "unit": "km", "suffix_mode": "auto", "decimal_rounding": 1, "x": 0, "y": 0 },
                // none drops a leftover manual suffix
                { "type": "value", "id": "c", "value": "speed",
                  "unit": "mph", "suffix": " mph", "suffix_mode": "none", "x": 0, "y": 0 },
                // custom keeps the manual suffix verbatim
                { "type": "value", "id": "d", "value": "speed",
                  "unit": "mph", "suffix": " miles/hr", "suffix_mode": "custom", "x": 0, "y": 0 },
                // absent mode with a manual suffix stays custom (back-compat)
                { "type": "value", "id": "e", "value": "speed",
                  "unit": "mph", "suffix": " mph", "x": 0, "y": 0 }
            ]
        });
        let t = Template::from_value(raw).unwrap();
        let fmt = |i: usize| {
            let Element::Value(cfg) = &t.elements[i] else {
                panic!("expected value element")
            };
            super::format_value(cfg.sample(&activity, 0, None), cfg, false)
        };

        assert_eq!(fmt(0), "22 mph"); // 10 m/s → 22.37 mph, auto suffix
        assert_eq!(fmt(1), "5.0 km"); // summary distance, auto suffix
        assert_eq!(fmt(2), "22"); // none → no suffix despite manual field
        assert_eq!(fmt(3), "22 miles/hr"); // custom verbatim
        assert_eq!(fmt(4), "22 mph"); // legacy: manual suffix, no mode
    }

    /// scene.units = "imperial" fills every unit-bearing element that left its
    /// unit unset; explicit per-element units still win, and metrics without a
    /// metric/imperial distinction are untouched.
    #[test]
    fn scene_units_imperial_fills_unset_convertible_metrics() {
        use crate::activity::{ATTR_DISTANCE, ATTR_POWER, ATTR_SPEED, Activity};
        use crate::template::Element;

        let activity = Activity {
            speed: vec![10.0], // 10 m/s
            power: vec![200.0],
            distance: vec![1609.34], // 1 mile
            valid_attributes: vec![ATTR_SPEED.into(), ATTR_POWER.into(), ATTR_DISTANCE.into()],
            ..Activity::default()
        };

        let raw = serde_json::json!({
            "scene": { "width": 100, "height": 100, "units": "imperial" },
            "elements": [
                // unset unit → inherits scene imperial
                { "type": "value", "id": "a", "value": "speed", "suffix_mode": "auto", "x": 0, "y": 0 },
                // explicit metric unit overrides the scene system
                { "type": "value", "id": "b", "value": "speed", "unit": "kmh", "suffix_mode": "auto", "x": 0, "y": 0 },
                // non-convertible metric is untouched by the scene system
                { "type": "value", "id": "c", "value": "power", "suffix_mode": "auto", "x": 0, "y": 0 },
                // running metric inherits the scene system via its base (distance)
                { "type": "value", "id": "d", "value": "running_distance", "suffix_mode": "auto", "decimal_rounding": 1, "x": 0, "y": 0 }
            ]
        });
        let t = Template::from_value(raw).unwrap();
        let fmt = |i: usize| {
            let Element::Value(cfg) = &t.elements[i] else {
                panic!("expected value element")
            };
            super::format_value(cfg.sample(&activity, 0, None), cfg, false)
        };

        assert_eq!(fmt(0), "22 mph"); // inherited imperial
        assert_eq!(fmt(1), "36 km/h"); // explicit override wins (10 m/s = 36 km/h)
        assert_eq!(fmt(2), "200 W"); // power unaffected
        assert_eq!(fmt(3), "1.0 mi"); // running metric follows scene imperial (was "km")
    }

    /// Meters and gauges auto-scale off `activity_metric_range`, which for W/kg
    /// must divide the power range by the rider weight (and yield a degenerate
    /// 0..0 range when no weight is set).
    #[test]
    fn power_to_weight_range_scales_by_rider_weight() {
        use crate::activity::{ATTR_POWER, ATTR_POWER_TO_WEIGHT, Activity};

        let activity = Activity {
            power: vec![100.0, 400.0],
            speed: vec![10.0, 10.0],
            valid_attributes: vec![ATTR_POWER.into(), ATTR_POWER_TO_WEIGHT.into()],
            ..Activity::default()
        };

        // No weight → flat 0..0 range.
        let none = super::activity_metric_range(&activity, ATTR_POWER_TO_WEIGHT, None, None);
        assert_eq!(none, Some((0.0, 0.0)));

        // 100 W / 50 kg = 2.0, 400 W / 50 kg = 8.0.
        let (min, max) =
            super::activity_metric_range(&activity, ATTR_POWER_TO_WEIGHT, None, Some(50.0))
                .unwrap();
        assert!((min - 2.0).abs() < 1e-9);
        assert!((max - 8.0).abs() < 1e-9);
    }

    /// Anchored elements get their x/y rewritten from the target's box; the
    /// anchor offset rides on top. Text elements pin their (x, y) origin to
    /// the resolved point; box elements offset by self_point (default center).
    #[test]
    fn anchors_resolve_to_target_box_points() {
        let raw = serde_json::json!({
            "scene": { "width": 1000, "height": 1000 },
            "elements": [
                { "type": "gauge", "id": "gauge-0", "value": "speed",
                  "x": 100, "y": 200, "width": 300, "height": 300, "min": 0, "max": 50 },
                { "type": "value", "id": "value-0", "value": "speed", "x": 0, "y": 0,
                  "anchor": { "target": "gauge-0", "offset_y": -10.0 } },
                { "type": "image", "id": "image-0", "file": "bolt.svg",
                  "x": 0, "y": 0, "width": 50, "height": 50,
                  "anchor": { "target": "gauge-0", "point": "bottom" } }
            ]
        });
        let t = Template::from_value(raw).unwrap();
        let (vx, vy) = match &t.elements[1] {
            crate::template::Element::Value(c) => (c.x, c.y),
            _ => unreachable!(),
        };
        // Gauge center (250, 350) + offset (0, -10).
        assert_eq!((vx, vy), (250, 340));
        let (ix, iy) = match &t.elements[2] {
            crate::template::Element::Image(c) => (c.x, c.y),
            _ => unreachable!(),
        };
        // Gauge bottom-center (250, 500); image centers its own 50×50 box there.
        assert_eq!((ix, iy), (225, 475));
    }
}
