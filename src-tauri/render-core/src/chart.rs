/// Chart rendering with Skia — the core performance optimization.
///
/// Pre-renders an entire chart to a cached Image once at scene init.
/// Per-frame cost is then: one image blit + one circle draw.
/// This replaces matplotlib's plt.savefig() which was 50–200ms per frame.
use skia_safe::{
    Canvas, Color, ISize, ImageInfo, Paint, PaintStyle, PathBuilder, Point, Rect, Typeface,
};

use crate::color::to_skia_color;
use crate::template::{CourseMarkerConfig, PlotConfig, PointLabelConfig};
use crate::units;

/// Banded per-segment coloring driven by a second attribute (`color_by`).
pub struct Banding {
    /// Color-driving series in display units, aligned with x/y data.
    pub data: Vec<f64>,
    /// (upper bound, hex color) sorted ascending; values at or above the last
    /// bound clamp to it.
    pub bands: Vec<(f64, String)>,
    /// Band-color the under-curve fill (non-course plots only).
    pub fill: bool,
    /// Band-color the line itself.
    pub line: bool,
}

/// Index of the band `v` falls in: first band whose upper bound exceeds `v`,
/// clamping to the last band (also the NaN fallback).
fn band_index(bands: &[(f64, String)], v: f64) -> usize {
    bands
        .iter()
        .position(|(max, _)| v < *max)
        .unwrap_or(bands.len().saturating_sub(1))
}

/// Pixel bounds of the data area inside a chart surface (excluding margins).
#[derive(Debug, Clone)]
pub struct PlotBounds {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl PlotBounds {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

/// Geographic coordinate mapping for course/GPS plots.
/// Pre-computes the cos(lat) correction and a uniform scale with letterbox offsets
/// so the rendered track preserves real geographic proportions.
#[derive(Debug, Clone)]
pub struct GeoMapping {
    pub x_min: f64,
    pub y_min: f64,
    pub cos_lat: f64,
    pub scale: f32,
    /// Letterbox offsets (pixels) so the track is centered within the plot area.
    pub x_off: f32,
    pub y_off: f32,
}

impl GeoMapping {
    fn build(x_min: f64, x_max: f64, y_min: f64, y_max: f64, plot: &PlotBounds) -> Option<Self> {
        let lon_extent = x_max - x_min;
        let lat_extent = y_max - y_min;
        if lon_extent <= 0.0 || lat_extent <= 0.0 {
            return None;
        }
        let mean_lat = (y_min + y_max) / 2.0;
        let cos_lat = mean_lat.to_radians().cos();
        let lon_px = lon_extent * cos_lat;
        let scale = (plot.width() as f64 / lon_px).min(plot.height() as f64 / lat_extent) as f32;
        let x_off = (plot.width() - lon_px as f32 * scale) / 2.0;
        let y_off = (plot.height() - lat_extent as f32 * scale) / 2.0;
        Some(GeoMapping {
            x_min,
            y_min,
            cos_lat,
            scale,
            x_off,
            y_off,
        })
    }

    fn to_pixel(&self, x: f64, y: f64, plot: &PlotBounds) -> Point {
        let px = plot.left + self.x_off + ((x - self.x_min) * self.cos_lat) as f32 * self.scale;
        let py = plot.bottom - self.y_off - (y - self.y_min) as f32 * self.scale;
        Point::new(px, py)
    }
}

/// Immutable, pre-rendered chart background plus the coordinate mapping needed
/// to draw the per-frame position marker.
pub struct ChartCache {
    /// Rendered chart background without any position markers.
    pub background: skia_safe::Image,
    /// X data (frame indices or lon values).
    pub x_data: Vec<f64>,
    /// Y data (elevation, lat, etc.).
    pub y_data: Vec<f64>,
    /// Activity distance in metres for each plotted point. Populated for course
    /// plots, where it is the distance axis of the route geometry.
    pub distance_data: Vec<f64>,
    /// Activity distance in metres at each output frame. Populated for course
    /// plots, where the frame grid is independent of the route geometry, so the
    /// rider's position is found by distance rather than by index.
    pub frame_distance: Vec<f64>,
    /// Pixel position of the chart within the full frame.
    pub x_offset: i32,
    pub y_offset: i32,
    /// Data min/max for mapping data coords → pixel coords.
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
    /// Pixel bounds of the plot area inside the surface.
    pub plot_bounds: PlotBounds,
    /// Point configs from the template (colour, size, etc.).
    pub point_configs: Vec<crate::template::PointConfig>,
    /// Static markers placed along a course by activity distance.
    pub markers: Vec<CourseMarkerConfig>,
    /// Geographic mapping for course plots (None for non-GPS charts).
    pub geo: Option<GeoMapping>,
    /// The plotted attribute (e.g. "elevation") — drives point-label units.
    pub value_attr: String,
    /// Optional value label drawn next to the live marker.
    pub point_label: Option<PointLabelConfig>,
    /// Typeface for the point label, loaded once (None if no label).
    pub label_typeface: Option<Typeface>,
    /// For course plots: draw traveled portion at this opacity per-frame.
    pub past_opacity: Option<f32>,
    /// For course plots: full-route background drawn at this opacity; None = full opacity.
    pub future_opacity: Option<f32>,
    /// Line color string (needed for per-frame past-segment drawing).
    pub line_color: String,
    /// Line width (needed for per-frame past-segment drawing).
    pub line_width: f32,
    /// Banded coloring by a second attribute (None = single-color plot).
    pub banding: Option<Banding>,
}

impl ChartCache {
    pub fn build(
        config: &PlotConfig,
        x_data: Vec<f64>,
        y_data: Vec<f64>,
        distance_data: Vec<f64>,
        frame_distance: Vec<f64>,
        color_data: Vec<f64>,
        fonts_dir: &str,
    ) -> Option<Self> {
        if x_data.is_empty() || y_data.is_empty() {
            return None;
        }

        let surf_w = config.width as i32;
        let surf_h = config.height as i32;
        if surf_w <= 0 || surf_h <= 0 {
            return None;
        }

        let margin = config.margin_fraction();
        let is_course = config.value == crate::activity::ATTR_COURSE;
        // Course plots keep the margin inset so the route line and position dot
        // don't clip at the surface edge. Non-course plots (elevation, etc.) go
        // edge-to-edge so their bounding box aligns exactly with the rendered pixels.
        let plot_bounds = if is_course {
            let m = margin as f32;
            PlotBounds {
                left: m * surf_w as f32,
                right: surf_w as f32 - m * surf_w as f32,
                top: m * surf_h as f32,
                bottom: surf_h as f32 - m * surf_h as f32,
            }
        } else {
            PlotBounds {
                left: 0.0,
                right: surf_w as f32,
                top: 0.0,
                bottom: surf_h as f32,
            }
        };

        let x_min = x_data.iter().cloned().fold(f64::INFINITY, f64::min);
        let x_max = x_data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let y_min = y_data.iter().cloned().fold(f64::INFINITY, f64::min);
        let y_max = y_data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        // Course plots use geographic aspect-ratio correction; all others get vertical padding.
        let is_course = config.value == crate::activity::ATTR_COURSE;
        let geo = if is_course {
            GeoMapping::build(x_min, x_max, y_min, y_max, &plot_bounds)
        } else {
            None
        };

        // Add some vertical padding so the line doesn't hug the top/bottom edges.
        let y_pad = (y_max - y_min) * margin;
        let (y_min_out, y_max_out) = if is_course {
            (y_min, y_max)
        } else {
            (y_min - y_pad, y_max + y_pad)
        };

        // Band thresholds are authored in display units; convert the color
        // series once so lookups compare like with like.
        let banding = config.color_by.as_ref().and_then(|cb| {
            let bands = cb.resolved_bands();
            if bands.is_empty() || color_data.len() != x_data.len() {
                return None;
            }
            let (conv, _) = units::resolve(cb.attr(), cb.unit.as_deref());
            Some(Banding {
                data: color_data.iter().map(|&v| conv.apply(v)).collect(),
                bands,
                fill: cb.color_fill(is_course),
                line: cb.color_line(is_course),
            })
        });

        let background = render_chart_background(
            surf_w,
            surf_h,
            &plot_bounds,
            &x_data,
            &y_data,
            &DataBounds {
                x_min,
                x_max,
                y_min: y_min_out,
                y_max: y_max_out,
            },
            config,
            geo.as_ref(),
            banding.as_ref(),
        );

        // Load the point-label typeface once (None when no label configured).
        let point_label = config.point_label.clone();
        let label_typeface = point_label.as_ref().and_then(|pl| {
            let font = pl.font.as_deref().unwrap_or("Arial.ttf");
            crate::frame::load_typeface(font, fonts_dir)
        });

        let past_opacity = if is_course {
            config.line_past_opacity()
        } else {
            None
        };
        let future_opacity = if is_course {
            config.line_future_opacity()
        } else {
            None
        };

        Some(ChartCache {
            background,
            x_data,
            y_data,
            distance_data: if is_course { distance_data } else { Vec::new() },
            frame_distance: if is_course {
                frame_distance
            } else {
                Vec::new()
            },
            x_offset: config.x,
            y_offset: config.y,
            x_min,
            x_max,
            y_min: y_min_out,
            y_max: y_max_out,
            plot_bounds,
            point_configs: config
                .effective_point()
                .map(|p| vec![p.clone()])
                .unwrap_or_default(),
            markers: if is_course {
                config.markers.clone().unwrap_or_default()
            } else {
                Vec::new()
            },
            geo,
            value_attr: config.value.clone(),
            point_label,
            label_typeface,
            past_opacity,
            future_opacity,
            line_color: config.line_color(),
            line_width: config.line_width(),
            banding,
        })
    }

    /// Map a data coordinate to a pixel position inside the chart surface.
    fn data_to_pixel(&self, x: f64, y: f64) -> Point {
        if let Some(geo) = &self.geo {
            return geo.to_pixel(x, y, &self.plot_bounds);
        }
        let px = self.plot_bounds.left
            + if self.x_max > self.x_min {
                ((x - self.x_min) / (self.x_max - self.x_min)) as f32 * self.plot_bounds.width()
            } else {
                0.0
            };
        let py = self.plot_bounds.bottom
            - if self.y_max > self.y_min {
                ((y - self.y_min) / (self.y_max - self.y_min)) as f32 * self.plot_bounds.height()
            } else {
                0.0
            };
        Point::new(px, py)
    }

    /// Translate a plot-local pixel position into full-frame coordinates.
    fn to_frame(&self, local: Point) -> Point {
        Point::new(
            local.x + self.x_offset as f32,
            local.y + self.y_offset as f32,
        )
    }

    /// Index of the last usable vertex across the plotted series.
    fn last_vertex(&self) -> usize {
        self.x_data
            .len()
            .min(self.y_data.len())
            .min(self.distance_data.len())
            .saturating_sub(1)
    }

    /// Interpolate the route at `target_m` of activity distance. Returns the
    /// data coords of that position and the index of the last route vertex at or
    /// before it — the point where the traveled segment stops.
    fn route_at_distance(&self, target_m: f64) -> Option<(f64, f64, usize)> {
        if self.x_data.is_empty() || self.distance_data.is_empty() {
            return None;
        }
        let last = self.last_vertex();
        if last == 0 {
            return Some((self.x_data[0], self.y_data[0], 0));
        }

        // Cumulative distance is non-decreasing, so the crossing point is a
        // binary search rather than a scan over the (now source-density) route.
        let target = target_m.clamp(self.distance_data[0], self.distance_data[last]);
        let i1 = self.distance_data[..=last]
            .partition_point(|&d| d < target)
            .clamp(1, last);
        let i0 = i1 - 1;
        let (d0, d1) = (self.distance_data[i0], self.distance_data[i1]);
        let frac = if d1 > d0 {
            ((target - d0) / (d1 - d0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let x = self.x_data[i0] + (self.x_data[i1] - self.x_data[i0]) * frac;
        let y = self.y_data[i0] + (self.y_data[i1] - self.y_data[i0]) * frac;
        Some((x, y, i0))
    }

    fn course_marker_point(&self, target_m: f64) -> Option<(Point, f32)> {
        self.geo.as_ref()?;
        let (x, y, i0) = self.route_at_distance(target_m)?;
        let i1 = (i0 + 1).min(self.last_vertex());
        let a = self.data_to_pixel(self.x_data[i0], self.y_data[i0]);
        let b = self.data_to_pixel(self.x_data[i1], self.y_data[i1]);
        let course_angle = (b.y - a.y).atan2(b.x - a.x).to_degrees();
        Some((self.to_frame(self.data_to_pixel(x, y)), course_angle + 90.0))
    }

    /// The rider's position in data coords at `frame_idx`, plus the index of the
    /// last plotted vertex behind them.
    ///
    /// Course plots resolve this by distance travelled: the route is drawn at the
    /// track's source density, so it has no per-frame index to look up. Every
    /// other plot has exactly one vertex per frame and indexes directly.
    fn marker_position(&self, frame_idx: usize) -> Option<(f64, f64, usize)> {
        if self.geo.is_some() {
            let last_frame = self.frame_distance.len().checked_sub(1)?;
            let travelled = self.frame_distance[frame_idx.min(last_frame)];
            return self.route_at_distance(travelled);
        }
        if frame_idx >= self.x_data.len() {
            return None;
        }
        let y = self.y_data.get(frame_idx).copied().unwrap_or(0.0);
        Some((self.x_data[frame_idx], y, frame_idx))
    }

    fn draw_course_marker(&self, canvas: &Canvas, marker: &CourseMarkerConfig) {
        let Some(distance_m) = marker.distance else {
            return;
        };
        let Some((pt, perpendicular_angle)) = self.course_marker_point(distance_m) else {
            return;
        };
        let width = marker.width.unwrap_or(34.0).max(1.0);
        let height = marker.height.unwrap_or(10.0).max(1.0);
        let angle = perpendicular_angle + marker.rotation.unwrap_or(0.0);
        let opacity = Some(marker.opacity.unwrap_or(1.0));

        canvas.save();
        canvas.translate((pt.x, pt.y));
        canvas.rotate(angle, None);

        let left = -width / 2.0;
        let top = -height / 2.0;
        let style = marker.style.as_deref().unwrap_or("checkered");

        if style == "circle" {
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(to_skia_color(
                marker.color.as_deref().unwrap_or("#ef4444"),
                opacity,
            ));
            paint.set_style(PaintStyle::Fill);
            canvas.draw_circle((0.0, 0.0), width.min(height) / 2.0, &paint);
        } else if style == "rectangle" {
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(to_skia_color(
                marker.color.as_deref().unwrap_or("#22c55e"),
                opacity,
            ));
            paint.set_style(PaintStyle::Fill);
            canvas.draw_rect(Rect::from_xywh(left, top, width, height), &paint);
        } else {
            let rect = Rect::from_xywh(left, top, width, height);
            let cell = height.max(1.0) / 2.0;
            let cols = (width / cell).ceil() as i32;
            for row in 0..2 {
                for col in 0..cols {
                    let color = if (row + col) % 2 == 0 {
                        to_skia_color("#000000", opacity)
                    } else {
                        to_skia_color("#ffffff", opacity)
                    };
                    let mut paint = Paint::default();
                    paint.set_anti_alias(false);
                    paint.set_color(color);
                    paint.set_style(PaintStyle::Fill);
                    let x = left + col as f32 * cell;
                    let mut square =
                        Rect::from_xywh(x, top + row as f32 * cell, cell + 0.5, cell + 0.5);
                    if square.intersect(rect) {
                        canvas.draw_rect(square, &paint);
                    }
                }
            }
            let mut border = Paint::default();
            border.set_anti_alias(true);
            border.set_color(to_skia_color("#111111", opacity));
            border.set_style(PaintStyle::Stroke);
            border.set_stroke_width(1.0);
            canvas.draw_rect(rect, &border);
        }
        canvas.restore();
    }

    /// Draw onto a canvas: blit background then draw position marker for frame_idx.
    pub fn draw_on_canvas(&self, canvas: &Canvas, frame_idx: usize) {
        // 1. Composite the cached chart background.
        canvas.draw_image(
            &self.background,
            Point::new(self.x_offset as f32, self.y_offset as f32),
            None,
        );

        let Some((pos_x, pos_y, past_end)) = self.marker_position(frame_idx) else {
            return;
        };

        // 2. Overdraw the traveled (past) segment at past_opacity when split is active.
        if (self.past_opacity.is_some() || self.future_opacity.is_some())
            && let Some(geo) = &self.geo
        {
            let past_opacity = self.past_opacity.unwrap_or(1.0);
            // Every route vertex behind the rider, then a final segment to their
            // exact interpolated position so the line ends under the dot instead
            // of at the last whole vertex.
            let pts: Vec<Point> = self
                .x_data
                .iter()
                .zip(self.y_data.iter())
                .take(past_end + 1)
                .map(|(&x, &y)| (x, y))
                .chain(std::iter::once((pos_x, pos_y)))
                .map(|(x, y)| self.to_frame(geo.to_pixel(x, y, &self.plot_bounds)))
                .collect();
            if let Some(b) = self.banding.as_ref().filter(|b| b.line) {
                // Segment i takes the band of its start vertex; the final
                // interpolated segment (to the rider dot) reuses the last
                // route vertex's band.
                let idx: Vec<usize> = (0..pts.len() - 1)
                    .map(|i| band_index(&b.bands, b.data[i.min(b.data.len() - 1)]))
                    .collect();
                // Bound the opacity layer to the chart area (inflated by the
                // stroke) so the per-frame layer allocation stays small.
                let pad = self.line_width;
                let layer_rect = Rect::from_xywh(
                    self.x_offset as f32 - pad,
                    self.y_offset as f32 - pad,
                    self.plot_bounds.right + 2.0 * pad,
                    self.plot_bounds.bottom + 2.0 * pad,
                );
                draw_banded_polyline(
                    canvas,
                    &pts,
                    &idx,
                    &b.bands,
                    self.line_width,
                    past_opacity,
                    layer_rect,
                );
            } else {
                let past_color = to_skia_color(&self.line_color, Some(past_opacity));
                let mut pb = PathBuilder::new();
                for (i, &pt) in pts.iter().enumerate() {
                    if i == 0 {
                        pb.move_to(pt);
                    } else {
                        pb.line_to(pt);
                    }
                }
                let path = pb.snapshot();
                let mut paint = Paint::default();
                paint.set_anti_alias(true);
                paint.set_color(past_color);
                paint.set_stroke_width(self.line_width);
                paint.set_style(PaintStyle::Stroke);
                canvas.draw_path(&path, &paint);
            }
        }

        // 3. Draw static course markers over the route.
        for marker in &self.markers {
            self.draw_course_marker(canvas, marker);
        }

        // 4. Draw position marker (the per-frame part).
        {
            let abs_pt = self.to_frame(self.data_to_pixel(pos_x, pos_y));

            for pc in &self.point_configs {
                let color = pc
                    .color
                    .as_deref()
                    .map(|c| to_skia_color(c, pc.opacity))
                    .unwrap_or(Color::WHITE);
                let radius = pc.weight.unwrap_or(80.0).sqrt() / 2.0; // weight is area → r = sqrt(w)/2

                let mut paint = Paint::default();
                paint.set_anti_alias(true);
                paint.set_color(color);
                paint.set_style(PaintStyle::Fill);
                canvas.draw_circle(abs_pt, radius, &paint);

                // Optionally draw edge stroke
                if pc.remove_edge_color.unwrap_or(false) {
                    // no edge
                } else if let Some(ec) = &pc.edge_color {
                    let mut ep = Paint::default();
                    ep.set_anti_alias(true);
                    ep.set_color(to_skia_color(ec, None));
                    ep.set_style(PaintStyle::Stroke);
                    ep.set_stroke_width(pc.edge_width.unwrap_or(1.0));
                    canvas.draw_circle(abs_pt, radius, &ep);
                }
            }

            // 5. Optional value label next to the marker (e.g. "960 M" /
            //    "3150 FT"), one line per unit, metric first.
            if let (Some(pl), Some(tf)) = (&self.point_label, &self.label_typeface) {
                let raw = pos_y;
                let size = pl.font_size.unwrap_or(32.0);
                let font =
                    crate::frame::font_from_typeface(tf.clone(), size, pl.italic.unwrap_or(false));
                let color = pl
                    .color
                    .as_deref()
                    .map(|c| to_skia_color(c, None))
                    .unwrap_or(Color::WHITE);
                let mut paint = Paint::default();
                paint.set_anti_alias(true);
                paint.set_color(color);

                let xo = pl.x_offset.unwrap_or(0.0);
                let yo = pl.y_offset.unwrap_or(0.0);
                let dec = pl.decimal_rounding.unwrap_or(0);
                let units = pl
                    .units
                    .clone()
                    .unwrap_or_else(|| vec!["metric".to_string()]);
                let (line_h, _) = font.metrics();
                let n = units.len() as f32;

                // Pre-format all lines and find max width to decide which side
                // to place the label block. Near the right edge the block flips
                // left so it never overflows the chart surface.
                let lines: Vec<String> = units
                    .iter()
                    .map(|unit| format_point_label(raw, &self.value_attr, unit, dec))
                    .collect();
                let max_text_w = lines
                    .iter()
                    .map(|t| font.measure_str(t.as_str(), Some(&paint)).0)
                    .fold(0.0_f32, f32::max);

                let chart_right = self.x_offset as f32 + self.plot_bounds.right;
                let flip_left = abs_pt.x + xo + max_text_w > chart_right;

                // Stack the block above the marker: last line sits `yo` above
                // the dot, earlier lines higher. When flipped, each line is
                // right-aligned to the left side of the marker.
                for (i, text) in lines.iter().enumerate() {
                    let text_w = font.measure_str(text.as_str(), Some(&paint)).0;
                    let label_x = if flip_left {
                        abs_pt.x - xo - text_w
                    } else {
                        abs_pt.x + xo
                    };
                    let baseline_y = abs_pt.y - yo - (n - 1.0 - i as f32) * line_h;
                    canvas.draw_str(text.as_str(), (label_x, baseline_y), &font, &paint);
                }
            }
        }
    }
}

/// Format a plotted value for a point label in the requested unit system,
/// producing "<number> <SUFFIX>" (e.g. "960 M", "3150 FT"). `raw` is the
/// attribute's native unit (elevation: metres, speed: m/s, temp: °C).
fn format_point_label(raw: f64, attr: &str, unit: &str, decimals: i32) -> String {
    let (conv, suffix) = units::resolve(attr, Some(unit));
    format!("{} {}", round_str(conv.apply(raw), decimals), suffix)
}

fn round_str(v: f64, decimals: i32) -> String {
    if decimals > 0 {
        format!("{:.*}", decimals as usize, v)
    } else {
        format!("{}", v.round() as i64)
    }
}

// ─── Background renderer ──────────────────────────────────────────────────

struct DataBounds {
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
}

#[allow(clippy::too_many_arguments)]
fn render_chart_background(
    surf_w: i32,
    surf_h: i32,
    plot_bounds: &PlotBounds,
    x_data: &[f64],
    y_data: &[f64],
    bounds: &DataBounds,
    config: &PlotConfig,
    geo: Option<&GeoMapping>,
    banding: Option<&Banding>,
) -> skia_safe::Image {
    let DataBounds {
        x_min,
        x_max,
        y_min,
        y_max,
    } = *bounds;
    let info = ImageInfo::new(
        ISize::new(surf_w, surf_h),
        skia_safe::ColorType::BGRA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface =
        skia_safe::surfaces::raster(&info, None, None).expect("Failed to create Skia surface");
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);

    let to_px = |x: f64, y: f64| -> Point {
        if let Some(g) = geo {
            return g.to_pixel(x, y, plot_bounds);
        }
        let px = plot_bounds.left
            + if x_max > x_min {
                ((x - x_min) / (x_max - x_min)) as f32 * plot_bounds.width()
            } else {
                0.0
            };
        let py = plot_bounds.bottom
            - if y_max > y_min {
                ((y - y_min) / (y_max - y_min)) as f32 * plot_bounds.height()
            } else {
                0.0
            };
        Point::new(px, py)
    };

    // Build the line path using PathBuilder (Path is immutable in skia-safe 0.93+)
    let mut pb = PathBuilder::new();
    for (i, (&x, &y)) in x_data.iter().zip(y_data.iter()).enumerate() {
        let pt = to_px(x, y);
        if i == 0 {
            pb.move_to(pt);
        } else {
            pb.line_to(pt);
        }
    }
    let line_path = pb.snapshot();

    // Draw fill under the curve
    let usable_banding = banding.filter(|b| b.data.len() == x_data.len() && x_data.len() >= 2);
    if let Some(b) = usable_banding.filter(|b| b.fill) {
        // Banded fill defaults to fully opaque (TdF profiles are solid).
        let alpha = config.fill_opacity().unwrap_or(1.0).clamp(0.0, 1.0);
        if alpha > 0.0 {
            draw_banded_fill(canvas, plot_bounds, x_data, y_data, b, &to_px, y_min, alpha);
        }
    } else if let Some(fill_opacity) = config.fill_opacity() {
        let fill_color_str = config.fill_color();
        let (r, g, b, _) = crate::color::parse_hex_color(&fill_color_str);
        let alpha = (fill_opacity.clamp(0.0, 1.0) * 255.0) as u8;
        let fill_color = Color::from_argb(alpha, r, g, b);

        let y_base = y_min;
        if let (Some(&last_x), Some(&first_x)) = (x_data.last(), x_data.first()) {
            let mut fb = PathBuilder::new_path(&line_path);
            fb.line_to(to_px(last_x, y_base));
            fb.line_to(to_px(first_x, y_base));
            fb.close();
            let fill_path = fb.snapshot();

            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(fill_color);
            paint.set_style(PaintStyle::Fill);
            canvas.draw_path(&fill_path, &paint);
        }
    }

    // Draw the line. When split opacities are configured the background shows
    // the full route at future_opacity; the past segment is overdrawn per-frame.
    let is_course = geo.is_some();
    let split_opacity = if is_course {
        config.line_future_opacity().or_else(|| {
            // past_opacity only → background at full opacity, past overdrawn per-frame
            config.line_past_opacity().map(|_| 1.0)
        })
    } else {
        None
    };
    if let Some(b) = usable_banding.filter(|b| b.line) {
        let pts: Vec<Point> = x_data
            .iter()
            .zip(y_data.iter())
            .map(|(&x, &y)| to_px(x, y))
            .collect();
        let idx: Vec<usize> = b.data.iter().map(|&v| band_index(&b.bands, v)).collect();
        let layer_rect = Rect::from_iwh(surf_w, surf_h);
        draw_banded_polyline(
            canvas,
            &pts,
            &idx,
            &b.bands,
            config.line_width(),
            split_opacity.unwrap_or(1.0),
            layer_rect,
        );
    } else {
        let line_color = to_skia_color(&config.line_color(), split_opacity);
        let mut line_paint = Paint::default();
        line_paint.set_anti_alias(true);
        line_paint.set_color(line_color);
        line_paint.set_stroke_width(config.line_width());
        line_paint.set_style(PaintStyle::Stroke);
        canvas.draw_path(&line_path, &line_paint);
    }

    surface.image_snapshot()
}

/// Fill the area under the curve with per-band colors, TdF-profile style.
///
/// Samples the curve once per pixel column, groups consecutive columns that
/// share a band into run polygons, and draws each run extended one column
/// into both neighbours: the overlap lets the next opaque run cover the
/// previous run's anti-aliased edge, so band boundaries blend over 1px
/// instead of leaving a hairline seam. Translucency is applied by drawing
/// opaque runs inside an alpha layer — overlaps must not double-blend.
#[allow(clippy::too_many_arguments)]
fn draw_banded_fill(
    canvas: &Canvas,
    plot_bounds: &PlotBounds,
    x_data: &[f64],
    y_data: &[f64],
    banding: &Banding,
    to_px: &dyn Fn(f64, f64) -> Point,
    y_base: f64,
    alpha: f32,
) -> Option<()> {
    let n = y_data.len();
    let cols_n = plot_bounds.width().floor() as i32;
    if n < 2 || cols_n < 1 {
        return None;
    }
    let base_y = to_px(*x_data.first()?, y_base).y;

    // Map a pixel column to a fractional position along the series. Under a
    // uniform x-axis index and pixel space agree, but a distance-based axis is
    // non-uniform: stepping by index would under-sample fast descents (chunky
    // fill, coarse band edges) and pile many samples into one column wherever
    // the rider was stopped. Walk the x range instead whenever x is monotonic;
    // a course's x is longitude and doubles back, so it keeps index stepping.
    let monotonic_x = x_data.windows(2).all(|w| w[1] >= w[0]);
    let x_first = *x_data.first()?;
    let span_x = *x_data.last()? - x_first;
    let position_at = |c: i32| -> f64 {
        let frac = c as f64 / cols_n as f64;
        if !monotonic_x || span_x <= 0.0 {
            return frac * (n - 1) as f64;
        }
        let target = x_first + span_x * frac;
        let j = x_data.partition_point(|&x| x < target).clamp(1, n - 1);
        let i = j - 1;
        let (x0, x1) = (x_data[i], x_data[j]);
        i as f64
            + if x1 > x0 {
                (target - x0) / (x1 - x0)
            } else {
                0.0
            }
    };

    // Per pixel column: (x px, curve y px, band index).
    let cols: Vec<(f32, f32, usize)> = (0..=cols_n)
        .map(|c| {
            let pos = position_at(c);
            let i = (pos.floor() as usize).min(n - 1);
            let j = (i + 1).min(n - 1);
            let f = pos - i as f64;
            let x = x_data[i] + (x_data[j] - x_data[i]) * f;
            let y = y_data[i] + (y_data[j] - y_data[i]) * f;
            let v = banding.data[i] + (banding.data[j] - banding.data[i]) * f;
            let pt = to_px(x, y);
            (pt.x, pt.y, band_index(&banding.bands, v))
        })
        .collect();

    let layered = alpha < 1.0;
    if layered {
        canvas.save_layer_alpha_f(
            Rect::from_ltrb(
                plot_bounds.left,
                plot_bounds.top,
                plot_bounds.right,
                plot_bounds.bottom,
            ),
            alpha,
        );
    }
    let mut run_start = 0usize;
    while run_start < cols.len() {
        let band = cols[run_start].2;
        let mut run_end = run_start;
        while run_end + 1 < cols.len() && cols[run_end + 1].2 == band {
            run_end += 1;
        }
        let draw_start = run_start.saturating_sub(1);
        let draw_end = (run_end + 1).min(cols.len() - 1);
        let mut pb = PathBuilder::new();
        pb.move_to(Point::new(cols[draw_start].0, base_y));
        for col in &cols[draw_start..=draw_end] {
            pb.line_to(Point::new(col.0, col.1));
        }
        pb.line_to(Point::new(cols[draw_end].0, base_y));
        pb.close();

        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(to_skia_color(&banding.bands[band].1, None));
        paint.set_style(PaintStyle::Fill);
        canvas.draw_path(&pb.snapshot(), &paint);
        run_start = run_end + 1;
    }
    if layered {
        canvas.restore();
    }
    Some(())
}

/// Stroke a polyline in per-band colors. Segment i takes the band of its
/// start point; consecutive same-band segments are stroked as one path,
/// sharing the boundary point with the next run so the line stays continuous
/// (round caps/joins hide the joint). Translucency is applied via an alpha
/// layer so the overlapping caps at run joints don't double-blend.
fn draw_banded_polyline(
    canvas: &Canvas,
    pts: &[Point],
    band_idx: &[usize],
    bands: &[(f64, String)],
    width: f32,
    alpha: f32,
    layer_rect: Rect,
) {
    if pts.len() < 2 || alpha <= 0.0 {
        return;
    }
    let layered = alpha < 1.0;
    if layered {
        canvas.save_layer_alpha_f(layer_rect, alpha);
    }
    let n_seg = pts.len() - 1;
    let mut seg = 0usize;
    while seg < n_seg {
        let band = band_idx[seg];
        let mut end = seg;
        while end + 1 < n_seg && band_idx[end + 1] == band {
            end += 1;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(pts[seg]);
        for pt in &pts[seg + 1..=end + 1] {
            pb.line_to(*pt);
        }
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(to_skia_color(&bands[band].1, None));
        paint.set_stroke_width(width);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_cap(skia_safe::PaintCap::Round);
        paint.set_stroke_join(skia_safe::PaintJoin::Round);
        canvas.draw_path(&pb.snapshot(), &paint);
        seg = end + 1;
    }
    if layered {
        canvas.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::{ChartCache, band_index, draw_banded_polyline, format_point_label};
    use skia_safe::{Color, ISize, ImageInfo, Point, Rect};

    fn bands() -> Vec<(f64, String)> {
        vec![
            (0.0, "#3b82f6".to_string()),
            (4.0, "#22c55e".to_string()),
            (f64::INFINITY, "#dc2626".to_string()),
        ]
    }

    #[test]
    fn band_index_picks_first_band_above_value_and_clamps() {
        let b = bands();
        assert_eq!(band_index(&b, -5.0), 0);
        assert_eq!(band_index(&b, 0.0), 1); // bounds are exclusive upper
        assert_eq!(band_index(&b, 3.9), 1);
        assert_eq!(band_index(&b, 4.0), 2);
        assert_eq!(band_index(&b, 99.0), 2);
        assert_eq!(band_index(&b, f64::NAN), 2); // NaN clamps to last
    }

    /// Read one BGRA pixel from a Skia image.
    fn pixel(img: &skia_safe::Image, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let info = ImageInfo::new(
            ISize::new(1, 1),
            skia_safe::ColorType::BGRA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = [0u8; 4];
        assert!(img.read_pixels(
            &info,
            &mut buf,
            4,
            (x, y),
            skia_safe::image::CachingHint::Disallow,
        ));
        (buf[0], buf[1], buf[2], buf[3]) // B, G, R, A
    }

    #[test]
    fn banded_fill_colors_under_curve_by_band() {
        // Elevation ramp colored by a gradient series: first half 2.0 (green
        // band of the built-in defaults), second half 8.0 (orange band).
        let config: crate::template::PlotConfig = serde_json::from_value(serde_json::json!({
            "id": "plot-0",
            "type": "plot",
            "value": "elevation",
            "x": 0, "y": 0, "width": 400, "height": 150,
            "color_by": { "value": "gradient" }
        }))
        .unwrap();
        let n = 100;
        let x: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let grade: Vec<f64> = (0..n).map(|i| if i < n / 2 { 2.0 } else { 8.0 }).collect();
        let cache = ChartCache::build(&config, x, y, Vec::new(), Vec::new(), grade, "/nonexistent")
            .unwrap();
        assert!(cache.banding.is_some());

        // Deep under the curve on each half. Defaults: 2% → #22c55e, 8% → #f97316.
        assert_eq!(pixel(&cache.background, 100, 140), (0x5e, 0xc5, 0x22, 0xff));
        assert_eq!(pixel(&cache.background, 300, 140), (0x16, 0x73, 0xf9, 0xff));
        // Above the curve stays transparent.
        assert_eq!(pixel(&cache.background, 100, 10), (0, 0, 0, 0));
    }

    #[test]
    fn banded_fill_resolves_band_edges_on_a_non_uniform_x_axis() {
        // Distance-axis case: 1980 dense samples crammed into the leftmost 1%
        // of the width (a slow climb), then 20 sparse samples covering the
        // remaining 99% (a fast descent). Sampling the fill by index would
        // spend 396 of 400 columns on those first 4 pixels and leave the band
        // edge in the sparse half quantised to the nearest ~100px column.
        let config: crate::template::PlotConfig = serde_json::from_value(serde_json::json!({
            "id": "plot-0",
            "type": "plot",
            "value": "elevation",
            "x": 0, "y": 0, "width": 400, "height": 150,
            "color_by": { "value": "gradient" }
        }))
        .unwrap();
        let n = 2000;
        let dense = 1980;
        let x: Vec<f64> = (0..n)
            .map(|i| {
                if i < dense {
                    i as f64 / dense as f64 * 10.0
                } else {
                    10.0 + (i - dense) as f64 / (n - dense) as f64 * 990.0
                }
            })
            .collect();
        let y: Vec<f64> = (0..n).map(|i| i as f64 / n as f64 * 100.0).collect();
        // Band flips green → orange at index 1988, i.e. x = 406 → pixel 162.
        let grade: Vec<f64> = (0..n).map(|i| if i < 1988 { 2.0 } else { 8.0 }).collect();
        let cache = ChartCache::build(&config, x, y, Vec::new(), Vec::new(), grade, "/nonexistent")
            .unwrap();
        assert!(cache.banding.is_some());

        // Deep under the curve either side of the true edge. Index sampling
        // would colour pixel 130 orange, having last sampled the series at
        // pixel 103 and next at 202.
        assert_eq!(pixel(&cache.background, 130, 140), (0x5e, 0xc5, 0x22, 0xff));
        assert_eq!(pixel(&cache.background, 250, 140), (0x16, 0x73, 0xf9, 0xff));
    }

    #[test]
    fn banded_polyline_strokes_runs_and_applies_layer_alpha() {
        let info = ImageInfo::new(
            ISize::new(100, 100),
            skia_safe::ColorType::BGRA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut surface = skia_safe::surfaces::raster(&info, None, None).unwrap();
        surface.canvas().clear(Color::TRANSPARENT);
        let pts = [
            Point::new(10.0, 50.0),
            Point::new(50.0, 50.0),
            Point::new(90.0, 50.0),
        ];
        // Segment bands: first green (idx 1), second red (idx 2).
        draw_banded_polyline(
            surface.canvas(),
            &pts,
            &[1, 2],
            &bands(),
            10.0,
            1.0,
            Rect::from_iwh(100, 100),
        );
        let img = surface.image_snapshot();
        assert_eq!(pixel(&img, 30, 50), (0x5e, 0xc5, 0x22, 0xff)); // #22c55e
        assert_eq!(pixel(&img, 70, 50), (0x26, 0x26, 0xdc, 0xff)); // #dc2626

        // Translucent variant: alpha applied once via the layer, even where
        // the two runs' round caps overlap at the shared point.
        surface.canvas().clear(Color::TRANSPARENT);
        draw_banded_polyline(
            surface.canvas(),
            &pts,
            &[1, 2],
            &bands(),
            10.0,
            0.5,
            Rect::from_iwh(100, 100),
        );
        let img = surface.image_snapshot();
        let (_, _, _, a_mid) = pixel(&img, 30, 50);
        let (_, _, _, a_joint) = pixel(&img, 50, 50);
        assert!((a_mid as i32 - 128).abs() <= 2, "alpha was {a_mid}");
        assert!(
            (a_joint as i32 - 128).abs() <= 2,
            "joint alpha was {a_joint} (double-blend?)"
        );
    }

    #[test]
    fn elevation_metric_and_imperial_match_screenshot_format() {
        // 960 m -> "960 M"; 960 * 3.28084 = 3149.6 -> "3150 FT"
        assert_eq!(format_point_label(960.0, "elevation", "metric", 0), "960 M");
        assert_eq!(
            format_point_label(960.0, "elevation", "imperial", 0),
            "3150 FT"
        );
    }

    #[test]
    fn speed_temperature_and_decimals() {
        assert_eq!(format_point_label(10.0, "speed", "metric", 0), "36 KM/H");
        assert_eq!(format_point_label(10.0, "speed", "imperial", 0), "22 MPH");
        assert_eq!(
            format_point_label(0.0, "temperature", "imperial", 0),
            "32 F"
        );
        assert_eq!(
            format_point_label(959.74, "elevation", "metric", 1),
            "959.7 M"
        );
    }

    #[test]
    fn unknown_attribute_echoes_unit_uppercased() {
        assert_eq!(format_point_label(5.0, "power", "watts", 0), "5 WATTS");
    }

    #[test]
    fn precise_unit_tokens() {
        assert_eq!(format_point_label(10.0, "speed", "kmh", 0), "36 KM/H");
        assert_eq!(format_point_label(10.0, "speed", "mph", 0), "22 MPH");
        assert_eq!(format_point_label(10.0, "speed", "ms", 0), "10 M/S");
        assert_eq!(format_point_label(5000.0, "distance", "km", 0), "5 KM");
        assert_eq!(format_point_label(5000.0, "distance", "m", 0), "5000 M");
        assert_eq!(format_point_label(1609.34, "distance", "mi", 0), "1 MI");
        assert_eq!(format_point_label(960.0, "elevation", "ft", 0), "3150 FT");
        assert_eq!(format_point_label(0.0, "temperature", "f", 0), "32 F");
        assert_eq!(format_point_label(20.0, "temperature", "c", 0), "20 C");
    }
}
