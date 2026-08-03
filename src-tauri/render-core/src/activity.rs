use quick_xml::events::Event;
use quick_xml::reader::Reader;

pub const ATTR_CADENCE: &str = "cadence";
pub const ATTR_COURSE: &str = "course";
pub const ATTR_DISTANCE: &str = "distance";
pub const ATTR_ELEVATION: &str = "elevation";
pub const ATTR_FRONT_GEAR: &str = "front_gear";
pub const ATTR_GEAR: &str = "gear";
pub const ATTR_GRADIENT: &str = "gradient";
pub const ATTR_HEARTRATE: &str = "heartrate";
pub const ATTR_LEAN: &str = "lean";
pub const ATTR_POWER: &str = "power";
/// Derived metric: power (W) ÷ rider weight (kg). Has no backing sample vec —
/// it is computed on demand from [`ATTR_POWER`] and a render-time rider weight
/// that is deliberately never part of the template (see `SceneConfig`).
pub const ATTR_POWER_TO_WEIGHT: &str = "power_to_weight";
pub const ATTR_REAR_GEAR: &str = "rear_gear";
pub const ATTR_SPEED: &str = "speed";
pub const ATTR_TIME: &str = "time";
pub const ATTR_TEMPERATURE: &str = "temperature";

// ─── Summary (aggregate) metrics ───────────────────────────────────────────
// Whole-window totals/averages rendered as a single constant value across every
// frame (e.g. a ride-summary stats card). Unlike the live telemetry attributes
// above they have no per-sample series; each resolves through
// [`Activity::summary_value`] against a precomputed [`ActivitySummary`].
pub const SUM_TOTAL_DISTANCE: &str = "total_distance";
pub const SUM_TOTAL_TIME: &str = "total_time";
pub const SUM_ELEVATION_GAIN: &str = "elevation_gain";
pub const SUM_ELEVATION_LOSS: &str = "elevation_loss";
pub const SUM_MAX_ELEVATION: &str = "max_elevation";
pub const SUM_MIN_ELEVATION: &str = "min_elevation";
pub const SUM_AVG_SPEED: &str = "avg_speed";
pub const SUM_MAX_SPEED: &str = "max_speed";
pub const SUM_AVG_POWER: &str = "avg_power";
pub const SUM_MAX_POWER: &str = "max_power";
pub const SUM_AVG_HEARTRATE: &str = "avg_heartrate";
pub const SUM_MAX_HEARTRATE: &str = "max_heartrate";
pub const SUM_AVG_CADENCE: &str = "avg_cadence";

// ─── Running (cumulative-to-current-point) metrics ─────────────────────────
// Live counters that accumulate from the activity start up to the current
// frame, so they tick upward as the render sweeps the ride — the readouts for
// a time-lapse ride-summary flyover. Unlike summary metrics they vary per
// frame; unlike plain telemetry they aren't a raw source attribute. Time and
// distance reuse the existing reference machinery (`get_time`/`get_distance`
// with `activity_start`); elevation gain/loss read precomputed cumulative
// series. Each resolves through [`Activity::get_running`].
pub const RUN_TIME: &str = "running_time";
pub const RUN_DISTANCE: &str = "running_distance";
pub const RUN_ELEVATION_GAIN: &str = "running_elevation_gain";
pub const RUN_ELEVATION_LOSS: &str = "running_elevation_loss";

// ─── Lap metrics ────────────────────────────────────────────────────────────
// Crit lap counters derived from a scene-level start/finish gate
// (`SceneConfig::lap_gate`). A pre-pass in `sample_for_scene` counts GPS
// crossings of the gate over the full activity (so crossings before a trimmed
// overlay window still count) into `Activity::laps_completed`; each token
// resolves through [`Activity::get_lap`]. All are unitless counts.
pub const ATTR_LAP: &str = "lap";
pub const LAP_LAPS_TO_GO: &str = "laps_to_go";
pub const LAP_FRACTION: &str = "lap_fraction";

/// Whether `name` is a lap metric token.
pub fn is_lap_metric(name: &str) -> bool {
    matches!(name, ATTR_LAP | LAP_LAPS_TO_GO | LAP_FRACTION)
}

/// Default start/finish detection radius in metres.
pub const LAP_GATE_DEFAULT_RADIUS_M: f64 = 25.0;

/// Minimum sustained altitude change (metres) that counts toward elevation
/// gain/loss. Elevation is already Savitzky-Golay smoothed at parse time, so a
/// gentle floor here just rejects residual jitter without swallowing real hills.
const ELEVATION_NOISE_THRESHOLD_M: f64 = 1.0;

/// The base telemetry attribute a summary metric derives from. Drives unit
/// resolution (via `units::resolve`) and availability checks. Returns `None`
/// for any token that is not a summary metric.
pub fn summary_base_metric(name: &str) -> Option<&'static str> {
    Some(match name {
        SUM_TOTAL_DISTANCE => ATTR_DISTANCE,
        SUM_TOTAL_TIME => ATTR_TIME,
        SUM_ELEVATION_GAIN | SUM_ELEVATION_LOSS | SUM_MAX_ELEVATION | SUM_MIN_ELEVATION => {
            ATTR_ELEVATION
        }
        SUM_AVG_SPEED | SUM_MAX_SPEED => ATTR_SPEED,
        SUM_AVG_POWER | SUM_MAX_POWER => ATTR_POWER,
        SUM_AVG_HEARTRATE | SUM_MAX_HEARTRATE => ATTR_HEARTRATE,
        SUM_AVG_CADENCE => ATTR_CADENCE,
        _ => return None,
    })
}

/// Whether `name` is a summary/aggregate metric token.
pub fn is_summary_metric(name: &str) -> bool {
    summary_base_metric(name).is_some()
}

/// The base telemetry attribute a running metric derives from. Drives unit
/// resolution (running metrics format identically to their base — km/mi for
/// distance, hh:mm:ss for time, m/ft for elevation) and availability checks.
/// Returns `None` for any token that is not a running metric.
pub fn running_base_metric(name: &str) -> Option<&'static str> {
    Some(match name {
        RUN_TIME => ATTR_TIME,
        RUN_DISTANCE => ATTR_DISTANCE,
        RUN_ELEVATION_GAIN | RUN_ELEVATION_LOSS => ATTR_ELEVATION,
        _ => return None,
    })
}

/// The base telemetry attribute whose unit table a value token formats through:
/// its own name for plain live metrics, or the underlying attribute for summary
/// and running derivatives. Used by the render formatter for unit conversion and
/// suffix selection.
pub fn unit_base_metric(name: &str) -> &str {
    summary_base_metric(name)
        .or_else(|| running_base_metric(name))
        .unwrap_or(name)
}

/// Accumulate total ascent and descent over an elevation series, counting only
/// moves that reach `threshold` metres from the last counted point.
fn elevation_gain_loss(elevation: &[f64], threshold: f64) -> (f64, f64) {
    if elevation.len() < 2 {
        return (0.0, 0.0);
    }
    let mut gain = 0.0;
    let mut loss = 0.0;
    let mut anchor = elevation[0];
    for &e in &elevation[1..] {
        let delta = e - anchor;
        if delta >= threshold {
            gain += delta;
            anchor = e;
        } else if delta <= -threshold {
            loss += -delta;
            anchor = e;
        }
    }
    (gain, loss)
}

/// Running ascent/descent aligned per index: element `i` is the cumulative gain
/// (resp. loss) over `elevation[0..=i]`, using the same anchor-threshold logic
/// as [`elevation_gain_loss`], so the final element equals that function's
/// total over the same series. Both arrays are the length of `elevation`.
fn cumulative_elevation_gain_loss(elevation: &[f64], threshold: f64) -> (Vec<f64>, Vec<f64>) {
    let n = elevation.len();
    let mut gains = vec![0.0; n];
    let mut losses = vec![0.0; n];
    if n < 2 {
        return (gains, losses);
    }
    let mut gain = 0.0;
    let mut loss = 0.0;
    let mut anchor = elevation[0];
    for i in 1..n {
        let delta = elevation[i] - anchor;
        if delta >= threshold {
            gain += delta;
            anchor = elevation[i];
        } else if delta <= -threshold {
            loss += -delta;
            anchor = elevation[i];
        }
        gains[i] = gain;
        losses[i] = loss;
    }
    (gains, losses)
}

/// Whole-window aggregate metrics, constant across every frame. Computed once
/// over the full activity and once over the trimmed overlay window; a Value
/// element chooses which via its `summary_scope`.
#[derive(Debug, Clone, Default)]
pub struct ActivitySummary {
    /// Distance covered in metres.
    pub total_distance: f64,
    /// Elapsed duration in seconds.
    pub total_time: f64,
    /// Cumulative ascent in metres.
    pub elevation_gain: f64,
    /// Cumulative descent in metres.
    pub elevation_loss: f64,
    pub max_elevation: f64,
    pub min_elevation: f64,
    /// Overall average speed in m/s (distance ÷ time).
    pub avg_speed: f64,
    pub max_speed: f64,
    pub avg_power: f64,
    pub max_power: f64,
    pub avg_heartrate: f64,
    pub max_heartrate: f64,
    pub avg_cadence: f64,
}

pub const MPH_CONVERSION: f64 = 2.23694;
pub const KMH_CONVERSION: f64 = 3.6;
pub const FT_CONVERSION: f64 = 3.28084;
pub const MI_CONVERSION: f64 = 0.001 / 1.60934; // metres to miles
pub const GRADIENT_SCALE: f64 = 1.747;

#[derive(Debug, Clone, Default)]
pub struct Activity {
    /// Seconds since the first recorded timestamp for each raw sample.
    /// Empty when the source file has no complete, monotonic timestamp axis.
    pub elapsed_seconds: Vec<f64>,
    pub course: Vec<(f64, f64)>,
    pub distance: Vec<f64>,
    /// The GPS track at its source recording density, covering the rendered
    /// window. Unlike `course` — which is resampled onto the output frame grid
    /// and so has exactly one entry per frame — this keeps every recorded point,
    /// so the drawn route stays faithful even when the frame grid is far coarser
    /// than the track (a 3-second time-lapse of a two-hour ride is 90 frames).
    /// Course plots draw from this and locate the rider along it by distance.
    pub route: Vec<(f64, f64)>,
    /// Cumulative activity distance (metres) at each `route` vertex, aligned 1:1.
    /// Maps a frame's `distance` onto a position along the route.
    pub route_distance: Vec<f64>,
    pub elevation: Vec<f64>,
    pub gradient: Vec<f64>,
    pub heartrate: Vec<f64>,
    pub lean: Vec<f64>,
    pub speed: Vec<f64>,
    pub cadence: Vec<f64>,
    pub power: Vec<f64>,
    pub temperature: Vec<f64>,
    pub front_gear: Vec<f64>,
    pub rear_gear: Vec<f64>,
    pub gear: Vec<f64>,
    /// Running ascent (metres) accumulated from the first sample up to each
    /// index, aligned 1:1 with the sample series. Backs the
    /// `running_elevation_gain` metric. Built during resampling; empty until
    /// then (readouts fall back to 0).
    pub cumulative_elevation_gain: Vec<f64>,
    /// Running descent (metres), counterpart to `cumulative_elevation_gain`.
    pub cumulative_elevation_loss: Vec<f64>,
    /// Completed lap count at each sample (0 during lap 1), from the
    /// start/finish gate pre-pass (`compute_laps`). Empty when the scene has no
    /// lap gate — lap metrics then read 0.
    pub laps_completed: Vec<f64>,
    /// Total race laps for `laps_to_go` / `lap_fraction`: the gate's manual
    /// override, else auto-detected as every crossing from the gate anchor to
    /// the end of the activity. 0 when no gate is set.
    pub total_laps: f64,
    pub valid_attributes: Vec<String>,
    /// Total cumulative distance (metres) of the full activity before any trim.
    pub total_activity_distance: f64,
    /// Total elapsed seconds of the full activity before any trim.
    pub total_activity_elapsed: f64,
    /// Unix millis of the first recorded sample, or `None` for sources without
    /// timestamps (some screen-recorded TCX, manually authored GPX). Used by
    /// the alignment timeline to map activity time → wall-clock for matching
    /// against a video's container `creation_time`.
    pub start_time_ms: Option<i64>,
    /// Aggregate metrics over the full activity (before overlay trimming).
    /// Preserved through `resample_wall_clock` so whole-ride totals stay
    /// available even when only a clip is rendered.
    pub activity_summary: ActivitySummary,
    /// Aggregate metrics over the trimmed overlay window actually being
    /// rendered. Equals `activity_summary` until the activity is resampled.
    pub overlay_summary: ActivitySummary,
}

impl Activity {
    pub fn from_gpx(path: &str) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read GPX file: {e}"))?;
        Self::parse_gpx(&content)
    }

    /// Dispatch to the correct parser based on file extension.
    pub fn from_file(path: &str) -> Result<Self, String> {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        match ext.as_str() {
            "fit" => Self::from_fit(path),
            "tcx" => {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| format!("Failed to read TCX file: {e}"))?;
                Self::parse_tcx(&content)
            }
            _ => Self::from_gpx(path),
        }
    }

    pub fn from_fit(path: &str) -> Result<Self, String> {
        use std::fs::File;
        use std::io::BufReader;

        let file = File::open(path).map_err(|e| format!("Failed to open FIT file: {e}"))?;
        let mut reader = BufReader::new(file);
        let records = fitparser::from_reader(&mut reader)
            .map_err(|e| format!("Failed to parse FIT file: {e}"))?;
        Self::from_fit_records(records)
    }

    /// Build an activity from already-parsed FIT records. Split out from
    /// `from_fit` so the FIT parser can be unit-tested without a binary
    /// `.fit` fixture.
    fn from_fit_records(records: Vec<fitparser::FitDataRecord>) -> Result<Self, String> {
        use fitparser::profile::MesgNum;

        let mut points: Vec<TrackPoint> = Vec::new();
        let mut cur_front_gear: Option<f64> = None;
        let mut cur_rear_gear: Option<f64> = None;

        for record in records {
            match record.kind() {
                MesgNum::Event => {
                    let mut front_teeth: Option<f64> = None;
                    let mut rear_teeth: Option<f64> = None;
                    let mut front_num: Option<f64> = None;
                    let mut rear_num: Option<f64> = None;
                    for field in record.fields() {
                        match field.name() {
                            "front_gear" => {
                                front_teeth = fit_f64(field.value());
                            }
                            "rear_gear" => {
                                rear_teeth = fit_f64(field.value());
                            }
                            "front_gear_num" => {
                                front_num = fit_f64(field.value());
                            }
                            "rear_gear_num" => {
                                rear_num = fit_f64(field.value());
                            }
                            _ => {}
                        }
                    }
                    if front_teeth.is_some() || front_num.is_some() {
                        cur_front_gear = front_teeth.or(front_num);
                    }
                    if rear_teeth.is_some() || rear_num.is_some() {
                        cur_rear_gear = rear_teeth.or(rear_num);
                    }
                }
                MesgNum::BikeProfile => {
                    for field in record.fields() {
                        match field.name() {
                            "front_gear" | "front_gear_num" => {
                                cur_front_gear = cur_front_gear.or_else(|| fit_f64(field.value()));
                            }
                            "rear_gear" | "rear_gear_num" => {
                                cur_rear_gear = cur_rear_gear.or_else(|| fit_f64(field.value()));
                            }
                            _ => {}
                        }
                    }
                }
                MesgNum::Record => {
                    let mut lat: Option<f64> = None;
                    let mut lon: Option<f64> = None;
                    let mut elevation: Option<f64> = None;
                    let mut heartrate: Option<f64> = None;
                    let mut cadence: Option<f64> = None;
                    let mut power: Option<f64> = None;
                    let mut temperature: Option<f64> = None;
                    let mut speed: Option<f64> = None;
                    let mut enhanced_speed: Option<f64> = None;
                    let mut time_str: Option<String> = None;
                    let mut front_gear = cur_front_gear;
                    let mut rear_gear = cur_rear_gear;
                    let mut front_teeth: Option<f64> = None;
                    let mut rear_teeth: Option<f64> = None;
                    let mut front_num: Option<f64> = None;
                    let mut rear_num: Option<f64> = None;

                    for field in record.fields() {
                        match field.name() {
                            "position_lat" => {
                                // FIT stores lat/lon as semicircles (SInt32); convert to degrees.
                                lat = fit_f64(field.value()).map(|v| v * SEMICIRCLES_TO_DEG);
                            }
                            "position_long" => {
                                lon = fit_f64(field.value()).map(|v| v * SEMICIRCLES_TO_DEG);
                            }
                            "altitude" | "enhanced_altitude" if elevation.is_none() => {
                                elevation = fit_f64(field.value());
                            }
                            "heart_rate" => heartrate = fit_f64(field.value()),
                            "cadence" => cadence = fit_f64(field.value()),
                            "power" => power = fit_f64(field.value()),
                            "temperature" => temperature = fit_f64(field.value()),
                            "speed" => speed = fit_f64(field.value()),
                            "enhanced_speed" => enhanced_speed = fit_f64(field.value()),
                            "front_gear" => {
                                front_teeth = fit_f64(field.value());
                            }
                            "rear_gear" => {
                                rear_teeth = fit_f64(field.value());
                            }
                            "front_gear_num" => {
                                front_num = fit_f64(field.value());
                            }
                            "rear_gear_num" => {
                                rear_num = fit_f64(field.value());
                            }
                            "timestamp" => {
                                if let fitparser::Value::Timestamp(dt) = field.value() {
                                    time_str = Some(dt.to_rfc3339());
                                }
                            }
                            _ => {}
                        }
                    }
                    let speed = enhanced_speed.or(speed);

                    if front_teeth.is_some() || front_num.is_some() {
                        front_gear = front_teeth.or(front_num);
                    }
                    if rear_teeth.is_some() || rear_num.is_some() {
                        rear_gear = rear_teeth.or(rear_num);
                    }

                    if front_gear.is_some() {
                        cur_front_gear = front_gear;
                    }
                    if rear_gear.is_some() {
                        cur_rear_gear = rear_gear;
                    }

                    if let (Some(lat), Some(lon)) = (lat, lon) {
                        points.push(TrackPoint {
                            lat,
                            lon,
                            elevation,
                            time_str,
                            heartrate,
                            lean: None,
                            cadence,
                            power,
                            temperature,
                            front_gear,
                            rear_gear,
                            speed,
                        });
                    }
                }
                _ => {}
            }
        }

        if points.is_empty() {
            return Err("No GPS track points found in FIT file. \
                 Indoor activities without GPS are not supported."
                .to_string());
        }

        Self::build_from_points(points)
    }

    pub fn parse_tcx(content: &str) -> Result<Self, String> {
        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);

        let mut points: Vec<TrackPoint> = Vec::new();
        let mut current: Option<TrackPoint> = None;
        let mut in_trackpoint = false;
        let mut in_position = false;
        let mut in_heartrate = false;
        let mut in_extensions = false;
        let mut has_position = false;
        let mut current_text = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let ename = e.name();
                    let local = local_name(ename.as_ref());
                    match local {
                        "Trackpoint" => {
                            current = Some(TrackPoint::default());
                            in_trackpoint = true;
                            has_position = false;
                            in_position = false;
                            in_heartrate = false;
                            in_extensions = false;
                        }
                        "Position" if in_trackpoint => in_position = true,
                        "HeartRateBpm" if in_trackpoint => in_heartrate = true,
                        "Extensions" if in_trackpoint => in_extensions = true,
                        _ => {}
                    }
                    current_text.clear();
                }
                Ok(Event::Text(e)) => {
                    if let Ok(t) = e.unescape() {
                        current_text = t.to_string();
                    }
                }
                Ok(Event::End(ref e)) => {
                    let ename = e.name();
                    let local = local_name(ename.as_ref());
                    if let Some(ref mut pt) = current {
                        match local {
                            "LatitudeDegrees" if in_position => {
                                pt.lat = current_text.parse().unwrap_or(0.0);
                                has_position = true;
                            }
                            "LongitudeDegrees" if in_position => {
                                pt.lon = current_text.parse().unwrap_or(0.0);
                            }
                            "AltitudeMeters" => {
                                pt.elevation = current_text.parse().ok();
                            }
                            "Time" if in_trackpoint => {
                                pt.time_str = Some(current_text.clone());
                            }
                            "Value" if in_heartrate => {
                                pt.heartrate = current_text.parse().ok();
                            }
                            "Cadence" | "RunCadence" => {
                                pt.cadence = current_text.parse().ok();
                            }
                            "Watts" | "PowerInWatts" if in_extensions => {
                                pt.power = current_text.parse().ok();
                            }
                            "Speed" if in_extensions => {
                                pt.speed =
                                    current_text.parse::<f64>().ok().filter(|v| v.is_finite());
                            }
                            "front_gear" | "frontGear" | "front_gear_num" if in_extensions => {
                                pt.front_gear = current_text.parse().ok();
                            }
                            "rear_gear" | "rearGear" | "rear_gear_num" if in_extensions => {
                                pt.rear_gear = current_text.parse().ok();
                            }
                            "Position" => in_position = false,
                            "HeartRateBpm" => in_heartrate = false,
                            "Extensions" => in_extensions = false,
                            "Trackpoint" => {
                                in_trackpoint = false;
                                if let Some(pt) = current.take()
                                    && has_position
                                {
                                    points.push(pt);
                                }
                            }
                            _ => {}
                        }
                    }
                    current_text.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(format!("XML parse error: {e}")),
                _ => {}
            }
            buf.clear();
        }

        if points.is_empty() {
            return Err("No track points found in TCX file".to_string());
        }

        Self::build_from_points(points)
    }

    /// Plausible sample ride used for the WYSIWYG preview when no GPX is
    /// loaded. Avoids shipping a bundled demo file: every metric is populated
    /// so any template element has something to render.
    pub fn synthetic(secs: usize) -> Self {
        let n = secs.max(1);
        let mut a = Activity::default();
        let mut cum_dist = 0.0;
        for i in 0..n {
            let t = i as f64;
            let spd = 8.0 + 3.0 * (t / 10.0).sin();
            if i > 0 {
                cum_dist += spd; // 1-second intervals
            }
            a.speed.push(spd);
            a.power.push(200.0 + 60.0 * (t / 8.0).sin());
            a.heartrate.push(140.0 + 15.0 * (t / 12.0).sin());
            a.lean.push(18.0 * (t / 5.0).sin());
            a.cadence.push(88.0 + 6.0 * (t / 6.0).sin());
            a.elevation.push(100.0 + 20.0 * (t / 15.0).sin());
            a.gradient.push(3.0 * (t / 15.0).cos());
            a.temperature.push(21.0);
            a.front_gear.push(2.0);
            a.rear_gear.push(5.0 + ((t / 12.0).sin() + 1.0) * 4.0);
            a.gear.push(encode_gear(2.0, *a.rear_gear.last().unwrap()));
            a.course
                .push((37.0 + t * 1.0e-4, -122.0 + (t / 20.0).sin() * 1.0e-3));
            a.distance.push(cum_dist);
            a.elapsed_seconds.push(t);
        }
        a.total_activity_distance = cum_dist;
        a.total_activity_elapsed = a.elapsed_seconds.last().copied().unwrap_or(0.0);
        a.valid_attributes = [
            ATTR_COURSE,
            ATTR_DISTANCE,
            ATTR_SPEED,
            ATTR_ELEVATION,
            ATTR_GRADIENT,
            ATTR_HEARTRATE,
            ATTR_LEAN,
            ATTR_CADENCE,
            ATTR_POWER,
            ATTR_POWER_TO_WEIGHT,
            ATTR_TEMPERATURE,
            ATTR_FRONT_GEAR,
            ATTR_REAR_GEAR,
            ATTR_GEAR,
            ATTR_TIME,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        a.activity_summary = a.compute_summary();
        a.overlay_summary = a.activity_summary.clone();
        a
    }

    pub fn parse_gpx(content: &str) -> Result<Self, String> {
        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);

        // `wpt`s are standalone waypoints (POIs, cue sheet entries), not part of
        // the recorded track: they carry no timestamps and sit wherever the
        // author dropped them. Folding them in would both warp the route and
        // break the timestamp axis, so they are only used as the track when the
        // file has no `trkpt`s at all (hand-authored, waypoint-only GPX).
        let mut track_points: Vec<TrackPoint> = Vec::new();
        let mut way_points: Vec<TrackPoint> = Vec::new();
        let mut current: Option<TrackPoint> = None;
        let mut in_extensions = false;
        let mut in_tpx = false; // inside TrackPointExtension container
        let mut current_point_tag = String::new(); // "trkpt" or "wpt"
        let mut current_text = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let ename = e.name();
                    let local = local_name(ename.as_ref());

                    match local {
                        "trkpt" | "wpt" => {
                            let lat = attr_f64(e, b"lat").unwrap_or(0.0);
                            let lon = attr_f64(e, b"lon").unwrap_or(0.0);
                            current = Some(TrackPoint {
                                lat,
                                lon,
                                ..Default::default()
                            });
                            current_point_tag = local.to_string();
                            in_extensions = false;
                            in_tpx = false;
                        }
                        "extensions" => {
                            in_extensions = true;
                        }
                        "TrackPointExtension" => {
                            in_tpx = true;
                        }
                        _ => {}
                    }
                    current_text.clear();
                }
                Ok(Event::Text(e)) => {
                    if let Ok(t) = e.unescape() {
                        current_text = t.to_string();
                    }
                }
                Ok(Event::End(ref e)) => {
                    let ename = e.name();
                    let local = local_name(ename.as_ref());

                    if let Some(ref mut pt) = current {
                        match local {
                            "ele" if !in_extensions => {
                                pt.elevation = current_text.parse().ok();
                            }
                            "time" if !in_extensions => {
                                pt.time_str = Some(current_text.clone());
                            }
                            "hr" if in_tpx => {
                                pt.heartrate = current_text.parse().ok();
                            }
                            "cad" if in_tpx => {
                                pt.cadence = current_text.parse().ok();
                            }
                            "atemp" if in_tpx => {
                                pt.temperature = current_text.parse().ok();
                            }
                            "lean" if in_extensions => {
                                pt.lean = current_text.parse::<f64>().ok().map(f64::to_degrees);
                            }
                            "front_gear" | "frontGear" | "front_gear_num" if in_extensions => {
                                pt.front_gear = current_text.parse().ok();
                            }
                            "rear_gear" | "rearGear" | "rear_gear_num" if in_extensions => {
                                pt.rear_gear = current_text.parse().ok();
                            }
                            // Power appears both as bare <power> or <PowerInWatts> in extensions
                            "power" | "PowerInWatts" | "watts" => {
                                pt.power = current_text.parse().ok();
                            }
                            "speed" => {
                                pt.speed =
                                    current_text.parse::<f64>().ok().filter(|v| v.is_finite());
                            }
                            "TrackPointExtension" => {
                                in_tpx = false;
                            }
                            "extensions" => {
                                in_extensions = false;
                            }
                            tag if !current_point_tag.is_empty() && tag == current_point_tag => {
                                if let Some(pt) = current.take() {
                                    if tag == "wpt" {
                                        way_points.push(pt);
                                    } else {
                                        track_points.push(pt);
                                    }
                                }
                                current_point_tag.clear();
                            }
                            _ => {}
                        }
                    }
                    current_text.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(format!("XML parse error: {e}")),
                _ => {}
            }
            buf.clear();
        }

        let points = if track_points.is_empty() {
            way_points
        } else {
            track_points
        };
        if points.is_empty() {
            return Err("No track points found in GPX file".to_string());
        }

        Self::build_from_points(points)
    }

    fn build_from_points(points: Vec<TrackPoint>) -> Result<Self, String> {
        let n = points.len();
        let mut activity = Activity::default();

        // Detect valid attributes by scanning all points (any() short-circuits).
        // Sampling only 3 indices was unreliable — an attribute absent at the sample
        // points but present elsewhere would be wrongly excluded, causing trim() panics
        // after interpolate() expanded speed but not the missed attribute's vec.
        let mut valid: std::collections::HashSet<String> = std::collections::HashSet::new();
        valid.insert(ATTR_COURSE.into());
        valid.insert(ATTR_DISTANCE.into());
        valid.insert(ATTR_SPEED.into());
        if points.iter().any(|p| p.elevation.is_some()) {
            valid.insert(ATTR_ELEVATION.into());
        }
        if points.iter().any(|p| p.time_str.is_some()) {
            valid.insert(ATTR_TIME.into());
        }
        if points.iter().any(|p| p.heartrate.is_some()) {
            valid.insert(ATTR_HEARTRATE.into());
        }
        if points.iter().any(|p| p.lean.is_some()) {
            valid.insert(ATTR_LEAN.into());
        }
        if points.iter().any(|p| p.cadence.is_some()) {
            valid.insert(ATTR_CADENCE.into());
        }
        if points.iter().any(|p| p.power.is_some()) {
            valid.insert(ATTR_POWER.into());
            // W/kg is available wherever power is; the weight comes in at render
            // time (or from the editor's local rider-weight setting).
            valid.insert(ATTR_POWER_TO_WEIGHT.into());
        }
        if points.iter().any(|p| p.temperature.is_some()) {
            valid.insert(ATTR_TEMPERATURE.into());
        }
        if points.iter().any(|p| p.front_gear.is_some()) {
            valid.insert(ATTR_FRONT_GEAR.into());
        }
        if points.iter().any(|p| p.rear_gear.is_some()) {
            valid.insert(ATTR_REAR_GEAR.into());
        }
        if points
            .iter()
            .any(|p| p.front_gear.is_some() && p.rear_gear.is_some())
        {
            valid.insert(ATTR_GEAR.into());
        }

        if valid.contains(ATTR_COURSE) && valid.contains(ATTR_ELEVATION) {
            valid.insert(ATTR_GRADIENT.into());
        }

        activity.valid_attributes = valid.into_iter().collect();
        activity.valid_attributes.sort(); // deterministic order

        let parsed_ms: Vec<Option<i64>> = points
            .iter()
            .map(|p| parse_timestamp_millis(p.time_str.as_deref()))
            .collect();
        if parsed_ms.iter().all(Option::is_some) {
            let base = parsed_ms[0].unwrap();
            let elapsed: Vec<f64> = parsed_ms
                .iter()
                .map(|ms| (ms.unwrap() - base) as f64 / 1000.0)
                .collect();
            if elapsed.windows(2).all(|w| w[1] >= w[0]) {
                activity.elapsed_seconds = elapsed;
                activity.start_time_ms = Some(base);
            }
        }

        // Build raw data arrays
        let mut raw_gradient: Vec<f64> = Vec::with_capacity(n);
        let mut cum_dist = 0.0f64;
        let mut speed_derived = false;

        for (i, pt) in points.iter().enumerate() {
            activity.course.push((pt.lat, pt.lon));
            activity.elevation.push(pt.elevation.unwrap_or(0.0));
            activity.heartrate.push(pt.heartrate.unwrap_or(0.0));
            activity.lean.push(pt.lean.unwrap_or(0.0));
            activity.cadence.push(pt.cadence.unwrap_or(0.0));
            activity.power.push(pt.power.unwrap_or(0.0));
            activity.temperature.push(pt.temperature.unwrap_or(0.0));
            activity.front_gear.push(pt.front_gear.unwrap_or(0.0));
            activity.rear_gear.push(pt.rear_gear.unwrap_or(0.0));
            activity.gear.push(
                pt.front_gear
                    .zip(pt.rear_gear)
                    .map(|(front, rear)| encode_gear(front, rear))
                    .unwrap_or(0.0),
            );

            // Speed in m/s: prefer the device-reported value when the source file
            // provides one (more accurate, already smoothed). Fall back to the
            // GPS position/time derivative for points without a native value.
            // Per-point fallback keeps the series continuous across sensor gaps.
            let spd = match pt.speed {
                Some(v) => v,
                None if i == 0 => 0.0,
                None => {
                    speed_derived = true;
                    let prev = &points[i - 1];
                    let dist = haversine_m(prev.lat, prev.lon, pt.lat, pt.lon);
                    let dt = time_delta_seconds(prev.time_str.as_deref(), pt.time_str.as_deref());
                    if dt > 0.0 { dist / dt } else { 0.0 }
                }
            };
            activity.speed.push(spd);

            // Cumulative distance from activity start
            if i > 0 {
                let prev = &points[i - 1];
                cum_dist += haversine_m(prev.lat, prev.lon, pt.lat, pt.lon);
            }
            activity.distance.push(cum_dist);

            // Gradient: elevation angle in degrees
            let grad = if i == 0 {
                None
            } else {
                let prev = &points[i - 1];
                if let (Some(e1), Some(e2)) = (prev.elevation, pt.elevation) {
                    let d = haversine_m(prev.lat, prev.lon, pt.lat, pt.lon);
                    if d > 0.0 {
                        Some(((e2 - e1) / d).atan().to_degrees())
                    } else {
                        Some(0.0)
                    }
                } else {
                    Some(0.0)
                }
            };
            raw_gradient.push(grad.unwrap_or(0.0));
        }

        // GPS-derived speed inherits position jitter — on a 1 Hz track the
        // point-to-point derivative swings several mph between consecutive
        // seconds — so smooth it like elevation. Device-reported speed is left
        // alone: the head unit already filtered it. The polynomial fit can
        // overshoot below zero around stops, hence the clamp.
        if speed_derived {
            activity.speed = savgol_smooth_11_3(&activity.speed)
                .into_iter()
                .map(|v| v.max(0.0))
                .collect();
        }

        // Smooth elevation with Savitzky-Golay (window=11, poly=3)
        if activity
            .valid_attributes
            .contains(&ATTR_ELEVATION.to_string())
        {
            activity.elevation = savgol_smooth_11_3(&activity.elevation);
        }

        // Smooth gradient: outlier removal + LOWESS-like + scale factor
        if activity
            .valid_attributes
            .contains(&ATTR_GRADIENT.to_string())
        {
            let mut grad = raw_gradient;
            // Fix first point: extrapolate from next two
            if grad.len() >= 3 {
                grad[0] = 2.0 * grad[1] - grad[2];
            }
            grad = handle_outliers(&grad, 2.0, 7);
            grad = lowess_smooth(&grad, 0.0005);
            activity.gradient = grad.iter().map(|&v| v * GRADIENT_SCALE).collect();
        }

        // Source-density route geometry, before any frame-grid resampling.
        activity.route = activity.course.clone();
        activity.route_distance = activity.distance.clone();

        activity.total_activity_distance = cum_dist;
        activity.total_activity_elapsed = activity.elapsed_seconds.last().copied().unwrap_or(0.0);
        activity.activity_summary = activity.compute_summary();
        activity.overlay_summary = activity.activity_summary.clone();
        Ok(activity)
    }

    /// Expand data density by linear interpolation for smooth per-frame values.
    pub fn interpolate(&mut self, fps: u32) {
        let fps = fps as usize;
        let skip = ATTR_TIME;

        for attr in self.valid_attributes.clone() {
            if attr == skip {
                continue;
            }
            match attr.as_str() {
                ATTR_COURSE => {
                    let lats: Vec<f64> = self.course.iter().map(|c| c.0).collect();
                    let lons: Vec<f64> = self.course.iter().map(|c| c.1).collect();
                    let new_lats = linear_interp(&lats, fps);
                    let new_lons = linear_interp(&lons, fps);
                    self.course = new_lats.into_iter().zip(new_lons).collect();
                }
                ATTR_DISTANCE => {
                    self.distance = linear_interp(&self.distance, fps);
                }
                ATTR_ELEVATION => {
                    self.elevation = linear_interp(&self.elevation, fps);
                }
                ATTR_GRADIENT => {
                    self.gradient = linear_interp(&self.gradient, fps);
                }
                ATTR_HEARTRATE => {
                    self.heartrate = linear_interp(&self.heartrate, fps);
                }
                ATTR_LEAN => {
                    self.lean = linear_interp(&self.lean, fps);
                }
                ATTR_SPEED => {
                    self.speed = linear_interp(&self.speed, fps);
                }
                ATTR_CADENCE => {
                    self.cadence = linear_interp(&self.cadence, fps);
                }
                ATTR_POWER => {
                    self.power = linear_interp(&self.power, fps);
                }
                ATTR_TEMPERATURE => {
                    self.temperature = linear_interp(&self.temperature, fps);
                }
                ATTR_FRONT_GEAR => {
                    self.front_gear = step_interp(&self.front_gear, fps);
                }
                ATTR_REAR_GEAR => {
                    self.rear_gear = step_interp(&self.rear_gear, fps);
                }
                ATTR_GEAR => {
                    self.gear = step_interp(&self.gear, fps);
                }
                _ => {}
            }
        }
        // Not a source attribute, so outside the valid_attributes loop: the
        // lap counter steps at crossings, exactly like the gears.
        if !self.laps_completed.is_empty() {
            self.laps_completed = step_interp(&self.laps_completed, fps);
        }
    }

    pub fn data_len(&self) -> usize {
        self.speed.len()
    }

    pub fn elapsed_duration(&self) -> Option<f64> {
        self.elapsed_seconds.last().copied()
    }

    pub fn has_wall_clock_time_axis(&self) -> bool {
        self.elapsed_seconds.len() == self.data_len()
            && self.elapsed_seconds.len() >= 2
            && self
                .elapsed_seconds
                .windows(2)
                .all(|w| w[0].is_finite() && w[1].is_finite() && w[1] >= w[0])
    }

    pub fn sample_for_scene(
        &self,
        scene: &crate::template::SceneConfig,
        synthetic: bool,
    ) -> Result<Self, String> {
        let fps = scene.fps.max(1);
        let start = scene.start.unwrap_or(0.0).max(0.0);
        // Lap counting must see the full activity — crossings before a trimmed
        // overlay window still advance the counter. It mutates, so it runs on
        // a clone; the source parse (shared via Arc in the app shell) must
        // stay pristine.
        if let Some(gate) = &scene.lap_gate {
            let mut with_laps = self.clone();
            with_laps.compute_laps(gate);
            return with_laps.resample_wall_clock(
                start,
                scene.end,
                fps,
                scene.target_duration,
                synthetic,
            );
        }
        self.resample_wall_clock(start, scene.end, fps, scene.target_duration, synthetic)
    }

    /// Resample the activity onto an evenly spaced frame grid covering the
    /// `start`..`end` ride window.
    ///
    /// `target_duration`: when `Some(secs)`, the whole window is compressed (or
    /// stretched) into that many seconds of output — a time-lapse. The frame
    /// count becomes `secs * fps` and each frame maps linearly across the ride
    /// window, so the render sweeps the entire ride in `secs`. When `None`,
    /// output plays at real time (frame count = window · fps), the historical
    /// behaviour. Either way `elapsed_seconds` stores *ride*-elapsed time, so
    /// running clocks read true ride time regardless of playback speed.
    pub fn resample_wall_clock(
        &self,
        start: f64,
        end: Option<f64>,
        fps: u32,
        target_duration: Option<f64>,
        synthetic: bool,
    ) -> Result<Self, String> {
        if !self.has_wall_clock_time_axis() {
            if synthetic {
                let mut cloned = self.clone();
                cloned.interpolate(fps);
                cloned.route = cloned.course.clone();
                cloned.route_distance = cloned.distance.clone();
                return Ok(cloned);
            }
            return Err("Wall-clock timeline requires activity timestamps".to_string());
        }

        let fps = fps.max(1);
        let duration = self.elapsed_duration().unwrap_or(0.0);
        let start = start.clamp(0.0, duration);
        let end = end.unwrap_or(duration).clamp(start, duration);
        let window = end - start;
        // Output length: the time-lapse target when set (and positive), else the
        // window itself (real-time playback).
        let compress = matches!(target_duration, Some(d) if d > 0.0);
        let out_duration = if compress {
            target_duration.unwrap()
        } else {
            window
        };
        let frames = (out_duration * fps as f64).ceil().max(1.0) as usize;
        let gap_threshold = self.wall_clock_gap_threshold();

        let mut out = Activity {
            total_activity_distance: self.total_activity_distance,
            total_activity_elapsed: self.total_activity_elapsed,
            start_time_ms: self.start_time_ms.map(|ms| ms + (start * 1000.0) as i64),
            valid_attributes: self.valid_attributes.clone(),
            // Whole-ride totals survive the trim; overlay totals are recomputed
            // from the trimmed series below.
            activity_summary: self.activity_summary.clone(),
            total_laps: self.total_laps,
            ..Activity::default()
        };

        for frame in 0..frames {
            // Ride time sampled for this output frame. Real-time playback keeps
            // the historical fixed 1/fps grid. Time-lapse instead spreads the
            // frames evenly across the whole window so the last frame lands
            // exactly on `end` — the ride is swept start→end in `out_duration`.
            let t = if compress {
                if frames <= 1 || window <= 0.0 {
                    start
                } else {
                    start + (frame as f64 / (frames - 1) as f64) * window
                }
            } else {
                start + frame as f64 / fps as f64
            };
            let t = t.min(duration);
            out.elapsed_seconds.push(t - start);
            let sample = self.wall_clock_sample(t, gap_threshold);
            out.course.push(sample.course);
            out.distance.push(sample.distance);
            out.elevation.push(sample.elevation);
            out.gradient.push(sample.gradient);
            out.heartrate.push(sample.heartrate);
            out.lean.push(sample.lean);
            out.speed.push(sample.speed);
            out.cadence.push(sample.cadence);
            out.power.push(sample.power);
            out.temperature.push(sample.temperature);
            out.front_gear.push(sample.front_gear);
            out.rear_gear.push(sample.rear_gear);
            out.gear.push(sample.gear);
            out.laps_completed.push(sample.laps_completed);
        }
        // No lap gate → keep the series empty so lap metrics read 0, not lap 1.
        if self.laps_completed.is_empty() {
            out.laps_completed.clear();
        }

        // Route geometry is kept at source density, independent of the frame
        // grid: a time-lapse has far fewer frames than recorded points, and
        // drawing the course from the per-frame series would decimate it into a
        // corner-cutting scribble. Take every recorded point inside the window.
        let (route, route_distance): (Vec<(f64, f64)>, Vec<f64>) = self
            .elapsed_seconds
            .iter()
            .enumerate()
            .filter(|(_, t)| **t >= start && **t <= end)
            .filter_map(|(i, _)| Some((*self.course.get(i)?, *self.distance.get(i)?)))
            .unzip();
        // A window shorter than the recording interval can span fewer than two
        // points; the resampled series still has a vertex per frame, so fall
        // back to it rather than leaving the plot with nothing to draw.
        if route.len() >= 2 {
            out.route = route;
            out.route_distance = route_distance;
        } else {
            out.route = out.course.clone();
            out.route_distance = out.distance.clone();
        }

        // Running ascent/descent over the resampled elevation series, so the
        // `running_elevation_gain`/`_loss` counters can read a per-frame total.
        let (gains, losses) =
            cumulative_elevation_gain_loss(&out.elevation, ELEVATION_NOISE_THRESHOLD_M);
        out.cumulative_elevation_gain = gains;
        out.cumulative_elevation_loss = losses;

        out.overlay_summary = out.compute_summary();
        Ok(out)
    }

    fn wall_clock_gap_threshold(&self) -> f64 {
        let mut intervals: Vec<f64> = self
            .elapsed_seconds
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|dt| dt.is_finite() && *dt > 0.0)
            .collect();
        if intervals.is_empty() {
            return 2.0;
        }
        intervals.sort_by(|a, b| a.total_cmp(b));
        let median = intervals[(intervals.len() - 1) / 2];
        (median * 2.0).max(2.0)
    }

    fn wall_clock_sample(&self, t: f64, gap_threshold: f64) -> ActivitySample {
        let len = self.data_len();
        if len == 0 {
            return ActivitySample::default();
        }
        let idx = self.elapsed_seconds.partition_point(|&x| x < t);
        if idx < len && (self.elapsed_seconds[idx] - t).abs() < 1e-9 {
            return self.sample_at_index(idx);
        }
        if idx == 0 {
            return self.sample_at_index(0);
        }
        if idx >= len {
            return self.sample_at_index(len - 1);
        }

        let prev = idx - 1;
        let next = idx;
        let t0 = self.elapsed_seconds[prev];
        let t1 = self.elapsed_seconds[next];
        let dt = t1 - t0;
        if dt <= 0.0 || dt > gap_threshold {
            return self.sample_at_index(prev);
        }
        let frac = ((t - t0) / dt).clamp(0.0, 1.0);
        self.sample_between(prev, next, frac)
    }

    fn sample_at_index(&self, index: usize) -> ActivitySample {
        ActivitySample {
            course: self.course.get(index).copied().unwrap_or_default(),
            distance: self.distance.get(index).copied().unwrap_or_default(),
            elevation: self.elevation.get(index).copied().unwrap_or_default(),
            gradient: self.gradient.get(index).copied().unwrap_or_default(),
            heartrate: self.heartrate.get(index).copied().unwrap_or_default(),
            lean: self.lean.get(index).copied().unwrap_or_default(),
            speed: self.speed.get(index).copied().unwrap_or_default(),
            cadence: self.cadence.get(index).copied().unwrap_or_default(),
            power: self.power.get(index).copied().unwrap_or_default(),
            temperature: self.temperature.get(index).copied().unwrap_or_default(),
            front_gear: self.front_gear.get(index).copied().unwrap_or_default(),
            rear_gear: self.rear_gear.get(index).copied().unwrap_or_default(),
            gear: self.gear.get(index).copied().unwrap_or_default(),
            laps_completed: self.laps_completed.get(index).copied().unwrap_or_default(),
        }
    }

    fn sample_between(&self, prev: usize, next: usize, frac: f64) -> ActivitySample {
        let lerp = |data: &[f64]| {
            let a = data.get(prev).copied().unwrap_or_default();
            let b = data.get(next).copied().unwrap_or(a);
            a + frac * (b - a)
        };
        let course_a = self.course.get(prev).copied().unwrap_or_default();
        let course_b = self.course.get(next).copied().unwrap_or(course_a);
        ActivitySample {
            course: (
                course_a.0 + frac * (course_b.0 - course_a.0),
                course_a.1 + frac * (course_b.1 - course_a.1),
            ),
            distance: lerp(&self.distance),
            elevation: lerp(&self.elevation),
            gradient: lerp(&self.gradient),
            heartrate: lerp(&self.heartrate),
            lean: lerp(&self.lean),
            speed: lerp(&self.speed),
            cadence: lerp(&self.cadence),
            power: lerp(&self.power),
            temperature: lerp(&self.temperature),
            front_gear: self.front_gear.get(prev).copied().unwrap_or_default(),
            rear_gear: self.rear_gear.get(prev).copied().unwrap_or_default(),
            gear: self.gear.get(prev).copied().unwrap_or_default(),
            // Steps like the gears: a lap only advances at a crossing sample.
            laps_completed: self.laps_completed.get(prev).copied().unwrap_or_default(),
        }
    }

    pub fn get_scalar(&self, attribute: &str, index: usize) -> f64 {
        let safe = |v: &[f64]| v.get(index).copied().unwrap_or(0.0);
        match attribute {
            ATTR_DISTANCE => safe(&self.distance),
            ATTR_ELEVATION => safe(&self.elevation),
            ATTR_GRADIENT => safe(&self.gradient),
            ATTR_HEARTRATE => safe(&self.heartrate),
            ATTR_LEAN => safe(&self.lean),
            ATTR_SPEED => safe(&self.speed),
            ATTR_CADENCE => safe(&self.cadence),
            ATTR_POWER => safe(&self.power),
            ATTR_TEMPERATURE => safe(&self.temperature),
            ATTR_FRONT_GEAR => safe(&self.front_gear),
            ATTR_REAR_GEAR => safe(&self.rear_gear),
            ATTR_GEAR => safe(&self.gear),
            _ => 0.0,
        }
    }

    /// Raw (GPX-native) value for a metric at `index`, resolving the derived
    /// [`ATTR_POWER_TO_WEIGHT`] (W/kg) from power and a render-time rider weight.
    /// Weight is deliberately not stored on the activity or template — it is
    /// supplied per render call. With no (or non-positive) weight, W/kg is 0.0.
    /// Every other metric defers to [`Activity::get_scalar`].
    pub fn get_metric(&self, attribute: &str, index: usize, rider_weight_kg: Option<f32>) -> f64 {
        if attribute == ATTR_POWER_TO_WEIGHT {
            let power = self.get_scalar(ATTR_POWER, index);
            return match rider_weight_kg {
                Some(w) if w > 0.0 => power / w as f64,
                _ => 0.0,
            };
        }
        self.get_scalar(attribute, index)
    }

    /// Compute all aggregate metrics over this activity's current series.
    /// Called on the full activity at parse time and on the trimmed overlay
    /// window after resampling.
    pub fn compute_summary(&self) -> ActivitySummary {
        let span = |v: &[f64]| match (v.first(), v.last()) {
            (Some(&f), Some(&l)) => (l - f).max(0.0),
            _ => 0.0,
        };
        let max_of = |v: &[f64]| {
            let m = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if m.is_finite() { m } else { 0.0 }
        };
        let min_of = |v: &[f64]| {
            let m = v.iter().copied().fold(f64::INFINITY, f64::min);
            if m.is_finite() { m } else { 0.0 }
        };
        let mean_of = |v: &[f64]| {
            if v.is_empty() {
                0.0
            } else {
                v.iter().sum::<f64>() / v.len() as f64
            }
        };

        let total_distance = span(&self.distance);
        let total_time = span(&self.elapsed_seconds);
        let (elevation_gain, elevation_loss) =
            elevation_gain_loss(&self.elevation, ELEVATION_NOISE_THRESHOLD_M);

        ActivitySummary {
            total_distance,
            total_time,
            elevation_gain,
            elevation_loss,
            max_elevation: max_of(&self.elevation),
            min_elevation: min_of(&self.elevation),
            // Overall average = total distance / total time, which is
            // time-weighted and robust to uneven sample intervals; fall back to
            // a plain sample mean only when there is no usable time axis.
            avg_speed: if total_time > 0.0 {
                total_distance / total_time
            } else {
                mean_of(&self.speed)
            },
            max_speed: max_of(&self.speed),
            avg_power: mean_of(&self.power),
            max_power: max_of(&self.power),
            avg_heartrate: mean_of(&self.heartrate),
            max_heartrate: max_of(&self.heartrate),
            avg_cadence: mean_of(&self.cadence),
        }
    }

    /// Resolve a summary metric token to its constant value for the requested
    /// scope: "activity" (default, whole ride) or "overlay" (trimmed window).
    pub fn summary_value(&self, name: &str, scope: Option<&str>) -> f64 {
        let s = match scope.unwrap_or("activity") {
            "overlay" => &self.overlay_summary,
            _ => &self.activity_summary,
        };
        match name {
            SUM_TOTAL_DISTANCE => s.total_distance,
            SUM_TOTAL_TIME => s.total_time,
            SUM_ELEVATION_GAIN => s.elevation_gain,
            SUM_ELEVATION_LOSS => s.elevation_loss,
            SUM_MAX_ELEVATION => s.max_elevation,
            SUM_MIN_ELEVATION => s.min_elevation,
            SUM_AVG_SPEED => s.avg_speed,
            SUM_MAX_SPEED => s.max_speed,
            SUM_AVG_POWER => s.avg_power,
            SUM_MAX_POWER => s.max_power,
            SUM_AVG_HEARTRATE => s.avg_heartrate,
            SUM_MAX_HEARTRATE => s.max_heartrate,
            SUM_AVG_CADENCE => s.avg_cadence,
            _ => 0.0,
        }
    }

    /// Whether a summary metric can be shown — true when its base telemetry
    /// attribute is present in the source file.
    pub fn has_summary(&self, name: &str) -> bool {
        match summary_base_metric(name) {
            Some(base) => self.valid_attributes.iter().any(|a| a == base),
            None => false,
        }
    }

    /// Distance in metres adjusted for the requested reference point.
    /// `reference` values: "overlay_start" (default), "activity_start",
    /// "overlay_end", "activity_end", "until_custom" (until custom point),
    /// "since_custom" (since custom point).
    /// `target_m`: for "until_custom" / "since_custom" — the reference distance in metres.
    pub fn get_distance(
        &self,
        reference: Option<&str>,
        target_m: Option<f64>,
        index: usize,
    ) -> f64 {
        let current = self.distance.get(index).copied().unwrap_or(0.0);
        let overlay_start = self.distance.first().copied().unwrap_or(0.0);
        let overlay_end = self.distance.last().copied().unwrap_or(0.0);
        match reference.unwrap_or("overlay_start") {
            "activity_start" => current,
            "overlay_end" => (overlay_end - current).max(0.0),
            "activity_end" => (self.total_activity_distance - current).max(0.0),
            "until_custom" | "custom" => target_m.map(|t| (t - current).max(0.0)).unwrap_or(0.0),
            "since_custom" => target_m.map(|t| (current - t).max(0.0)).unwrap_or(0.0),
            _ => (current - overlay_start).max(0.0), // "overlay_start"
        }
    }

    /// Elapsed seconds adjusted for the requested reference point.
    /// `reference` values: "overlay_start" (default), "activity_start",
    /// "overlay_end", "activity_end", "until_custom", "since_custom",
    /// "time_of_day" (wall-clock seconds since midnight, requires `start_time_ms`).
    /// `target_s`: for "until_custom" / "since_custom" — the reference time in seconds.
    /// `hours_offset`: UTC offset in hours applied only for "time_of_day".
    pub fn get_time(
        &self,
        reference: Option<&str>,
        target_s: Option<f64>,
        hours_offset: f32,
        index: usize,
    ) -> f64 {
        let elapsed = self.elapsed_seconds.get(index).copied().unwrap_or(0.0);
        let overlay_end = self.elapsed_seconds.last().copied().unwrap_or(0.0);
        match reference.unwrap_or("overlay_start") {
            "time_of_day" => {
                if let Some(start_ms) = self.start_time_ms {
                    let start_s = start_ms as f64 / 1000.0;
                    let wall_s = start_s + elapsed + hours_offset as f64 * 3600.0;
                    wall_s.rem_euclid(86400.0)
                } else {
                    elapsed
                }
            }
            "activity_start" => elapsed,
            "overlay_end" => (overlay_end - elapsed).max(0.0),
            "activity_end" => (self.total_activity_elapsed - elapsed).max(0.0),
            "until_custom" => target_s.map(|t| (t - elapsed).max(0.0)).unwrap_or(0.0),
            "since_custom" => target_s.map(|t| (elapsed - t).max(0.0)).unwrap_or(0.0),
            _ => elapsed, // "overlay_start"
        }
    }

    /// Resolve a running (cumulative-to-current-point) metric at `index`, in
    /// GPX-native units (seconds / metres). Time and distance reuse the
    /// `activity_start` reference (elapsed and cumulative distance); elevation
    /// gain/loss read the precomputed running series. Unknown tokens → 0.
    pub fn get_running(&self, name: &str, index: usize) -> f64 {
        match name {
            RUN_TIME => self.get_time(Some("activity_start"), None, 0.0, index),
            RUN_DISTANCE => self.get_distance(Some("activity_start"), None, index),
            RUN_ELEVATION_GAIN => self
                .cumulative_elevation_gain
                .get(index)
                .copied()
                .unwrap_or(0.0),
            RUN_ELEVATION_LOSS => self
                .cumulative_elevation_loss
                .get(index)
                .copied()
                .unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// How far past the race-finish handle a crossing's closest-approach
    /// sample may sit and still count. The handle is placed by eye on the
    /// finish frame; GPS cadence means the detected crossing sample can trail
    /// it slightly.
    const LAP_GATE_END_GRACE_S: f64 = 2.0;

    /// Count start/finish gate crossings into the `laps_completed` series and
    /// resolve `total_laps`. Must run on the full-resolution activity before
    /// any trim/resample so crossings outside the overlay window still count.
    ///
    /// The gate point is the track position at ride time `gate.start` — the
    /// rider is on the line at the race-start moment. Each contiguous stretch
    /// of samples within the detection radius is one pass, counted at its
    /// closest-approach sample; passes are deduped by requiring real
    /// along-track travel between them so GPS jitter at the radius boundary
    /// can't double-count. The pass containing the gate anchor starts lap 1 —
    /// earlier passes (warm-up laps) don't count, and passes after `gate.end`
    /// (cooldown) don't either.
    pub fn compute_laps(&mut self, gate: &crate::template::LapGateConfig) {
        let n = self.data_len();
        self.laps_completed = vec![0.0; n];
        self.total_laps = 0.0;
        if n < 2
            || self.course.len() != n
            || self.distance.len() != n
            || self.elapsed_seconds.len() != n
        {
            return;
        }

        let gate_idx = self
            .elapsed_seconds
            .partition_point(|&t| t < gate.start)
            .min(n - 1);
        let (gate_lat, gate_lon) = self.course[gate_idx];
        // Equirectangular metres — exact enough at gate-radius scale.
        let m_per_deg_lat = 111_320.0;
        let m_per_deg_lon = 111_320.0 * gate_lat.to_radians().cos();
        let gate_dist_m = |(lat, lon): (f64, f64)| {
            let dy = (lat - gate_lat) * m_per_deg_lat;
            let dx = (lon - gate_lon) * m_per_deg_lon;
            (dx * dx + dy * dy).sqrt()
        };
        let radius = gate
            .radius
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(LAP_GATE_DEFAULT_RADIUS_M);

        // Closest-approach sample of every in-radius pass, in ride order.
        let mut passes: Vec<usize> = Vec::new();
        let mut best: Option<(f64, usize)> = None;
        for i in 0..n {
            let d = gate_dist_m(self.course[i]);
            if d <= radius {
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, i));
                }
            } else if let Some((_, bi)) = best.take() {
                passes.push(bi);
            }
        }
        if let Some((_, bi)) = best {
            passes.push(bi);
        }
        let min_gap_m = (radius * 3.0).max(50.0);
        passes.dedup_by(|next, prev| self.distance[*next] - self.distance[*prev] < min_gap_m);

        // The pass containing the gate anchor (the anchor sample is always
        // in-radius — it is 0 m from itself) starts lap 1.
        let Some(start_pos) = passes
            .iter()
            .position(|&p| (self.distance[p] - self.distance[gate_idx]).abs() < min_gap_m)
        else {
            return;
        };
        // Crossings between race start and race finish; later passes are the
        // cooldown and never advance the counter (nor inflate the auto total).
        let cutoff = gate.end.map(|e| e + Self::LAP_GATE_END_GRACE_S);
        let lap_marks: Vec<usize> = passes[start_pos + 1..]
            .iter()
            .copied()
            .filter(|&p| cutoff.is_none_or(|c| self.elapsed_seconds[p] <= c))
            .collect();

        let mut mark = 0;
        for (i, out) in self.laps_completed.iter_mut().enumerate() {
            if mark < lap_marks.len() && lap_marks[mark] <= i {
                mark += 1;
            }
            *out = mark as f64;
        }
        self.total_laps = gate
            .total_laps
            .map(|t| t as f64)
            .unwrap_or(lap_marks.len() as f64);
    }

    /// Resolve a lap metric at `index`. `lap` is the 1-based lap currently
    /// being ridden, capped at the race total; `laps_to_go` includes the
    /// current lap (bell lap reads 1, finished reads 0); `lap_fraction` packs
    /// current lap and total into one sample (see [`decode_lap_fraction`]).
    /// All read 0 when no gate is configured.
    pub fn get_lap(&self, name: &str, index: usize) -> f64 {
        if self.laps_completed.is_empty() {
            return 0.0;
        }
        let completed = self.laps_completed.get(index).copied().unwrap_or(0.0);
        let total = self.total_laps;
        let current = if total > 0.0 {
            (completed + 1.0).min(total)
        } else {
            completed + 1.0
        };
        match name {
            ATTR_LAP => current,
            LAP_LAPS_TO_GO => (total - completed).max(0.0),
            LAP_FRACTION => current * 10_000.0 + total,
            _ => 0.0,
        }
    }

    /// Build (x, y) data arrays for a plot of the given attribute.
    /// `x_axis` only affects non-course scalar plots: "distance" uses the
    /// travelled distance as the horizontal axis, anything else uses evenly
    /// spaced sample indices (i.e. time at constant frame rate).
    pub fn plot_data(&self, attribute: &str, x_axis: Option<&str>) -> (Vec<f64>, Vec<f64>) {
        let distance_based = x_axis == Some("distance");
        let scalar = |data: &[f64]| -> (Vec<f64>, Vec<f64>) {
            if distance_based && attribute != ATTR_DISTANCE && data.len() == self.distance.len() {
                (self.distance.clone(), data.to_vec())
            } else {
                let x: Vec<f64> = (0..data.len()).map(|i| i as f64).collect();
                (x, data.to_vec())
            }
        };
        match attribute {
            ATTR_DISTANCE => scalar(&self.distance),
            ATTR_ELEVATION => scalar(&self.elevation),
            ATTR_HEARTRATE => scalar(&self.heartrate),
            ATTR_LEAN => scalar(&self.lean),
            ATTR_SPEED => scalar(&self.speed),
            ATTR_CADENCE => scalar(&self.cadence),
            ATTR_POWER => scalar(&self.power),
            ATTR_TEMPERATURE => scalar(&self.temperature),
            ATTR_GRADIENT => scalar(&self.gradient),
            ATTR_FRONT_GEAR => scalar(&self.front_gear),
            ATTR_REAR_GEAR => scalar(&self.rear_gear),
            ATTR_GEAR => scalar(&self.gear),
            ATTR_COURSE => {
                // Source-density geometry, not the per-frame series — see `route`.
                let x: Vec<f64> = self.route.iter().map(|c| c.1).collect(); // lon
                let y: Vec<f64> = self.route.iter().map(|c| c.0).collect(); // lat
                (x, y)
            }
            _ => (vec![], vec![]),
        }
    }
}

#[derive(Default)]
struct ActivitySample {
    course: (f64, f64),
    distance: f64,
    elevation: f64,
    gradient: f64,
    heartrate: f64,
    lean: f64,
    speed: f64,
    cadence: f64,
    power: f64,
    temperature: f64,
    front_gear: f64,
    rear_gear: f64,
    gear: f64,
    laps_completed: f64,
}

// ─── Raw track point from XML ──────────────────────────────────────────────

#[derive(Default)]
struct TrackPoint {
    lat: f64,
    lon: f64,
    elevation: Option<f64>,
    time_str: Option<String>,
    heartrate: Option<f64>,
    lean: Option<f64>,
    cadence: Option<f64>,
    power: Option<f64>,
    temperature: Option<f64>,
    front_gear: Option<f64>,
    rear_gear: Option<f64>,
    /// Native device-reported speed in m/s, if the source file provides one.
    /// `None` for GPS-only files, where speed is derived from position deltas.
    speed: Option<f64>,
}

// ─── Smoothing algorithms ──────────────────────────────────────────────────

/// Savitzky-Golay filter, window=11, poly=3. Coefficients from standard tables.
fn savgol_smooth_11_3(data: &[f64]) -> Vec<f64> {
    let n = data.len();
    if n < 11 {
        return data.to_vec();
    }
    // Precomputed SG coefficients for window=11, poly=3, derivative=0
    const COEFFS: [f64; 11] = [
        -0.08391608,
        0.02097902,
        0.10256410,
        0.16083916,
        0.19580420,
        0.20745921,
        0.19580420,
        0.16083916,
        0.10256410,
        0.02097902,
        -0.08391608,
    ];
    // Derived as [-36, 9, 44, 69, 84, 89, 84, 69, 44, 9, -36] / 429.0
    let half = 5usize;
    let mut result = Vec::with_capacity(n);

    for i in 0..n {
        let mut val = 0.0f64;
        for (k, &c) in COEFFS.iter().enumerate() {
            let src = if i + k < half {
                // Reflect at left boundary
                half - (i + k) - 1
            } else if i + k - half >= n {
                // Reflect at right boundary
                2 * (n - 1) - (i + k - half)
            } else {
                i + k - half
            };
            val += c * data[src.min(n - 1)];
        }
        result.push(val);
    }
    result
}

/// Z-score outlier detection with sliding window; replaces outliers with window mean.
fn handle_outliers(data: &[f64], z_threshold: f64, window: usize) -> Vec<f64> {
    let mut out = data.to_vec();
    let n = data.len();
    for i in 0..n.saturating_sub(window - 1) {
        let w = &data[i..i + window];
        let mean = w.iter().sum::<f64>() / w.len() as f64;
        let var = w.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / w.len() as f64;
        let std = var.sqrt();
        if std < 1e-10 {
            continue;
        }
        for j in 0..window {
            let z = (data[i + j] - mean).abs() / std;
            if z > z_threshold {
                out[i + j] = mean;
            }
        }
    }
    out
}

/// Lightweight LOWESS approximation using tricubic-weighted local linear regression.
/// With smooth_fraction=0.0005, bandwidth is tiny (~3 points), matching the Python implementation.
fn lowess_smooth(data: &[f64], smooth_fraction: f64) -> Vec<f64> {
    let n = data.len();
    let bandwidth = ((smooth_fraction * n as f64).round() as usize).max(3);
    let half_bw = bandwidth / 2;
    let mut result = Vec::with_capacity(n);

    for i in 0..n {
        let start = i.saturating_sub(half_bw);
        let end = (start + bandwidth).min(n);
        let start = end.saturating_sub(bandwidth);

        let len = end - start;
        let center = i - start;
        let max_dist = (len as f64) / 2.0;

        let mut sw = 0.0f64;
        let mut sx = 0.0f64;
        let mut sy = 0.0f64;
        let mut sxx = 0.0f64;
        let mut sxy = 0.0f64;

        for j in 0..len {
            let d = ((j as f64 - center as f64) / max_dist).abs().min(1.0);
            // Tricubic weight
            let w = (1.0 - d.powi(3)).powi(3);
            let x = j as f64;
            let y = data[start + j];
            sw += w;
            sx += w * x;
            sy += w * y;
            sxx += w * x * x;
            sxy += w * x * y;
        }

        let denom = sw * sxx - sx * sx;
        let fitted = if denom.abs() < 1e-12 {
            sy / sw
        } else {
            let b = (sw * sxy - sx * sy) / denom;
            let a = (sy - b * sx) / sw;
            a + b * center as f64
        };
        result.push(fitted);
    }
    result
}

// ─── Linear interpolation ──────────────────────────────────────────────────

/// Expand data by linear interpolation to add fps-1 intermediate points per second.
pub fn linear_interp(data: &[f64], fps: usize) -> Vec<f64> {
    let n = data.len();
    if n == 0 {
        return vec![];
    }
    // Append extrapolated boundary point
    let mut extended = data.to_vec();
    if n >= 2 {
        extended.push(2.0 * data[n - 1] - data[n - 2]);
    } else {
        extended.push(data[n - 1]);
    }

    let total = (n - 1) * fps + 1;
    let mut result = Vec::with_capacity(total);
    let step = 1.0 / fps as f64;
    let mut x = 0.0f64;

    while x <= (n - 1) as f64 + 1e-9 {
        let i = x.floor() as usize;
        let frac = x - i as f64;
        let i_next = (i + 1).min(extended.len() - 1);
        result.push(extended[i] + frac * (extended[i_next] - extended[i]));
        x += step;
    }
    result
}

/// Expand discrete state data by holding each source sample until the next one.
fn step_interp(data: &[f64], fps: usize) -> Vec<f64> {
    let n = data.len();
    if n == 0 {
        return vec![];
    }
    let total = (n - 1) * fps + 1;
    let mut result = Vec::with_capacity(total);
    for value in data.iter().take(n - 1) {
        for _ in 0..fps {
            result.push(*value);
        }
    }
    result.push(data[n - 1]);
    result
}

// ─── Geometry helpers ──────────────────────────────────────────────────────

pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let phi1 = lat1.to_radians();
    let phi2 = lat2.to_radians();
    let d_phi = (lat2 - lat1).to_radians();
    let d_lam = (lon2 - lon1).to_radians();
    let a = (d_phi / 2.0).sin().powi(2) + phi1.cos() * phi2.cos() * (d_lam / 2.0).sin().powi(2);
    R * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

fn time_delta_seconds(t1: Option<&str>, t2: Option<&str>) -> f64 {
    match (t1, t2) {
        (Some(a), Some(b)) => {
            let t1 = parse_timestamp_millis(Some(a));
            let t2 = parse_timestamp_millis(Some(b));
            match (t1, t2) {
                (Some(a), Some(b)) => (b - a) as f64 / 1000.0,
                _ => 0.0,
            }
        }
        _ => 0.0,
    }
}

fn parse_timestamp_millis(t: Option<&str>) -> Option<i64> {
    use chrono::DateTime;
    DateTime::parse_from_rfc3339(t?)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// Unpack a `lap_fraction` sample (encoded in [`Activity::get_lap`] as
/// `lap * 10_000 + total`, mirroring the gear encoding) into (lap, total).
pub fn decode_lap_fraction(raw: f64) -> (i64, i64) {
    if !raw.is_finite() || raw < 0.0 {
        return (0, 0);
    }
    let encoded = raw.round() as i64;
    (encoded / 10_000, encoded % 10_000)
}

pub fn encode_gear(front: f64, rear: f64) -> f64 {
    front.round() * 100.0 + rear.round()
}

pub fn decode_gear(gear: f64) -> Option<(i64, i64)> {
    if !gear.is_finite() || gear <= 0.0 {
        return None;
    }
    let encoded = gear.round() as i64;
    let front = encoded / 100;
    let rear = encoded % 100;
    if front > 0 && rear > 0 {
        Some((front, rear))
    } else {
        None
    }
}

// ─── FIT helpers ──────────────────────────────────────────────────────────

/// FIT stores lat/lon as signed 32-bit semicircles; multiply by this to get degrees.
const SEMICIRCLES_TO_DEG: f64 = 180.0 / 2_147_483_648.0;

fn fit_f64(value: &fitparser::Value) -> Option<f64> {
    use fitparser::Value::*;
    match value {
        SInt8(v) => Some(*v as f64),
        UInt8(v) => Some(*v as f64),
        SInt16(v) => Some(*v as f64),
        UInt16(v) => Some(*v as f64),
        SInt32(v) => Some(*v as f64),
        UInt32(v) => Some(*v as f64),
        Float32(v) if v.is_finite() => Some(*v as f64),
        Float64(v) if v.is_finite() => Some(*v),
        SInt64(v) => Some(*v as f64),
        UInt64(v) => Some(*v as f64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::SceneConfig;

    fn wall_clock_scene(start: f64, end: f64, fps: u32) -> SceneConfig {
        SceneConfig {
            width: 1920,
            height: 1080,
            fps,
            font_size: None,
            font: None,
            overlay_filename: None,
            start: Some(start),
            end: Some(end),
            target_duration: None,
            decimal_rounding: None,
            color: None,
            opacity: None,
            rider_weight_kg: None,
            lap_gate: None,
            layers: None,
            groups: Vec::new(),
            vars: std::collections::HashMap::new(),
            units: None,
        }
    }

    /// A winding 1 Hz track: a full circle, so a decimated polyline is easy to
    /// tell from a faithful one (it cuts across the arc).
    fn circling_gpx(points: usize) -> String {
        let mut s = String::from("<gpx><trk><trkseg>");
        for i in 0..points {
            let angle = (i as f64 / points as f64) * std::f64::consts::TAU;
            let lat = 37.8 + 0.01 * angle.sin();
            let lon = -122.4 + 0.01 * angle.cos();
            s.push_str(&format!(
                "<trkpt lat=\"{lat:.6}\" lon=\"{lon:.6}\"><ele>10</ele>\
                 <time>2026-01-01T00:{:02}:{:02}Z</time></trkpt>",
                i / 60,
                i % 60,
            ));
        }
        s.push_str("</trkseg></trk></gpx>");
        s
    }

    /// A time-lapse has far fewer frames than recorded points. The route must
    /// keep its source density regardless, or the course plot degenerates into a
    /// coarse scribble that cuts every corner.
    #[test]
    fn timelapse_keeps_route_at_source_density() {
        let activity = Activity::parse_gpx(&circling_gpx(600)).expect("gpx parses");
        assert_eq!(activity.route.len(), 600);

        let mut scene = wall_clock_scene(0.0, 599.0, 30);
        scene.target_duration = Some(3.0); // whole ride compressed into 3 s
        let out = activity.sample_for_scene(&scene, false).expect("resamples");

        // The frame grid collapses to 3 s × 30 fps …
        assert_eq!(out.data_len(), 90);
        // … but the drawn geometry keeps every recorded point in the window.
        assert_eq!(out.route.len(), 600);
        assert_eq!(out.route_distance.len(), out.route.len());
        assert!(out.route_distance.windows(2).all(|w| w[1] >= w[0]));

        // Course plots draw from `route`, so the plotted line stays full-density.
        let (x, y) = out.plot_data(ATTR_COURSE, None);
        assert_eq!(x.len(), 600);
        assert_eq!(y.len(), 600);
    }

    /// Waypoints (cue sheet / POI entries, which many route exports carry) are
    /// not part of the recorded track: they'd warp the route and, being
    /// timestamp-less, take the whole timeline down with them.
    #[test]
    fn waypoints_are_not_treated_as_track_points() {
        let gpx = "<gpx>\
             <wpt lat=\"0.0\" lon=\"0.0\"><name>Coffee stop</name></wpt>\
             <trk><trkseg>\
               <trkpt lat=\"37.80\" lon=\"-122.40\"><time>2026-01-01T00:00:00Z</time></trkpt>\
               <trkpt lat=\"37.81\" lon=\"-122.41\"><time>2026-01-01T00:00:01Z</time></trkpt>\
             </trkseg></trk></gpx>";
        let activity = Activity::parse_gpx(gpx).expect("gpx parses");

        assert_eq!(activity.route.len(), 2);
        assert!(!activity.route.contains(&(0.0, 0.0)));
        // The waypoint carried no <time>; the track's own axis must survive it.
        assert_eq!(activity.elapsed_seconds, vec![0.0, 1.0]);
    }

    /// `start`/`end` still trim the route — a clip shows only the ridden portion.
    #[test]
    fn route_is_trimmed_to_the_scene_window() {
        let activity = Activity::parse_gpx(&circling_gpx(600)).expect("gpx parses");
        let out = activity
            .sample_for_scene(&wall_clock_scene(100.0, 199.0, 30), false)
            .expect("resamples");

        assert_eq!(out.route.len(), 100);
        assert_eq!(out.route_distance.len(), 100);
    }

    #[test]
    fn parses_gpx_timestamps_as_elapsed_seconds() {
        let gpx = r#"
        <gpx>
          <trk><trkseg>
            <trkpt lat="1" lon="2"><time>2026-01-01T00:00:00Z</time></trkpt>
            <trkpt lat="1" lon="2.001"><time>2026-01-01T00:00:02Z</time></trkpt>
            <trkpt lat="1" lon="2.002"><time>2026-01-01T00:00:05Z</time></trkpt>
          </trkseg></trk>
        </gpx>
        "#;
        let activity = Activity::parse_gpx(gpx).unwrap();
        assert_eq!(activity.elapsed_seconds, vec![0.0, 2.0, 5.0]);
        assert!(activity.has_wall_clock_time_axis());
    }

    #[test]
    fn gps_derived_speed_is_smoothed() {
        // 1 Hz track alternating ~12 m and ~8 m steps: the raw position
        // derivative saws 12→8→12 m/s every second, like real GPS jitter.
        // 1e-4 deg of latitude ≈ 11.1 m.
        let mut lat = 0.0f64;
        let mut body = String::new();
        for i in 0..40 {
            if i > 0 {
                let step_m = if i % 2 == 0 { 12.0 } else { 8.0 };
                lat += step_m / 111_194.9;
            }
            body.push_str(&format!(
                r#"<trkpt lat="{lat:.7}" lon="0"><time>2026-01-01T00:00:{i:02}Z</time></trkpt>"#
            ));
        }
        let gpx = format!("<gpx><trk><trkseg>{body}</trkseg></trk></gpx>");
        let a = Activity::parse_gpx(&gpx).unwrap();

        // Away from the boundaries the ±4 m/s sawtooth must be flattened…
        let max_jump = a.speed[5..35]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f64::max);
        assert!(max_jump < 1.0, "smoothed speed still jumps {max_jump} m/s");
        // …without shifting the average speed.
        let mean = a.speed[5..35].iter().sum::<f64>() / 30.0;
        assert!(
            (mean - 10.0).abs() < 0.5,
            "mean speed drifted to {mean} m/s"
        );
    }

    #[test]
    fn device_reported_speed_is_not_smoothed() {
        // Same sawtooth, but as explicit <speed> values: the device already
        // filtered these, so they must come through untouched.
        let mut body = String::new();
        for i in 0..40 {
            let spd = if i % 2 == 0 { 12.0 } else { 8.0 };
            body.push_str(&format!(
                r#"<trkpt lat="1" lon="2"><time>2026-01-01T00:00:{i:02}Z</time><extensions><speed>{spd}</speed></extensions></trkpt>"#
            ));
        }
        let gpx = format!("<gpx><trk><trkseg>{body}</trkseg></trk></gpx>");
        let a = Activity::parse_gpx(&gpx).unwrap();
        for (i, &v) in a.speed.iter().enumerate() {
            let expected = if i % 2 == 0 { 12.0 } else { 8.0 };
            assert_eq!(v, expected, "speed[{i}] was altered");
        }
    }

    #[test]
    fn elevation_gain_loss_ignores_sub_threshold_jitter() {
        // Net +10m with 0.5m jitter that must not accumulate at a 1m floor.
        let elev = vec![100.0, 100.5, 100.0, 105.0, 104.7, 110.0];
        let (gain, loss) = elevation_gain_loss(&elev, ELEVATION_NOISE_THRESHOLD_M);
        assert!((gain - 10.0).abs() < 1e-9, "gain was {gain}");
        assert_eq!(loss, 0.0, "loss was {loss}");
    }

    #[test]
    fn summary_value_reflects_scope() {
        let mut a = Activity::default();
        a.valid_attributes = vec![
            ATTR_DISTANCE.to_string(),
            ATTR_ELEVATION.to_string(),
            ATTR_SPEED.to_string(),
        ];
        // Full ride: 0..1000m, elevation climbs 100→300 (gain 200).
        a.distance = vec![0.0, 500.0, 1000.0];
        a.elevation = vec![100.0, 200.0, 300.0];
        a.speed = vec![5.0, 10.0, 15.0];
        a.elapsed_seconds = vec![0.0, 50.0, 100.0];
        a.activity_summary = a.compute_summary();
        // Overlay window = second half only: 500..1000m, elevation 200→300.
        let mut overlay = a.clone();
        overlay.distance = vec![500.0, 1000.0];
        overlay.elevation = vec![200.0, 300.0];
        overlay.elapsed_seconds = vec![50.0, 100.0];
        a.overlay_summary = overlay.compute_summary();

        assert_eq!(
            a.summary_value(SUM_TOTAL_DISTANCE, Some("activity")),
            1000.0
        );
        assert_eq!(a.summary_value(SUM_TOTAL_DISTANCE, Some("overlay")), 500.0);
        assert_eq!(a.summary_value(SUM_ELEVATION_GAIN, None), 200.0); // default = activity
        assert_eq!(a.summary_value(SUM_ELEVATION_GAIN, Some("overlay")), 100.0);
        assert_eq!(a.summary_value(SUM_MAX_ELEVATION, Some("activity")), 300.0);
        // avg_speed = distance / time = 1000 / 100 = 10 m/s.
        assert_eq!(a.summary_value(SUM_AVG_SPEED, Some("activity")), 10.0);
        assert!(a.has_summary(SUM_TOTAL_DISTANCE));
        assert!(!a.has_summary(SUM_AVG_POWER)); // power not in valid_attributes
    }

    /// Synthetic crit track: each entry is (lat, cumulative distance m); the
    /// gate sits at lon 5.0 and lat 40.0, ~0.0001° lat ≈ 11 m.
    fn lap_activity(points: &[(f64, f64)]) -> Activity {
        let mut a = Activity::default();
        a.course = points.iter().map(|&(lat, _)| (lat, 5.0)).collect();
        a.distance = points.iter().map(|&(_, d)| d).collect();
        a.speed = vec![10.0; points.len()];
        // One sample per second, so sample index == ride time.
        a.elapsed_seconds = (0..points.len()).map(|i| i as f64).collect();
        a
    }

    fn lap_gate(
        start: f64,
        end: Option<f64>,
        total_laps: Option<u32>,
    ) -> crate::template::LapGateConfig {
        crate::template::LapGateConfig {
            start,
            end,
            radius: None,
            total_laps,
        }
    }

    #[test]
    fn compute_laps_counts_gate_crossings() {
        const FAR: f64 = 40.004; // ≈445 m from the gate
        let mut a = lap_activity(&[
            (40.0, 0.0), // start at the line
            (FAR, 300.0),
            (FAR, 600.0),
            (40.0001, 900.0), // lap 1
            (FAR, 1200.0),
            (FAR, 1500.0),
            (40.0, 1800.0), // lap 2
            (FAR, 2100.0),
            (FAR, 2400.0),
            (40.0001, 2700.0), // lap 3 — finish
            (FAR, 3000.0),     // cooldown
        ]);
        a.compute_laps(&lap_gate(0.0, None, None));

        assert_eq!(
            a.laps_completed,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0]
        );
        assert_eq!(a.total_laps, 3.0);
        assert_eq!(a.get_lap(ATTR_LAP, 0), 1.0);
        assert_eq!(a.get_lap(ATTR_LAP, 4), 2.0);
        // Current lap caps at the race total after the finish crossing.
        assert_eq!(a.get_lap(ATTR_LAP, 10), 3.0);
        assert_eq!(a.get_lap(LAP_LAPS_TO_GO, 0), 3.0);
        assert_eq!(a.get_lap(LAP_LAPS_TO_GO, 3), 2.0);
        assert_eq!(a.get_lap(LAP_LAPS_TO_GO, 10), 0.0);
        assert_eq!(decode_lap_fraction(a.get_lap(LAP_FRACTION, 4)), (2, 3));
    }

    #[test]
    fn compute_laps_ignores_warmup_and_honours_total_override() {
        const FAR: f64 = 40.004;
        let mut a = lap_activity(&[
            (40.0, 0.0), // warm-up pass — before the gate anchor
            (FAR, 300.0),
            (FAR, 600.0),
            (40.0, 900.0), // gate anchor: race start
            (FAR, 1200.0),
            (40.0, 1500.0), // lap 1
            (FAR, 1800.0),
            (40.0, 2100.0), // lap 2
        ]);
        a.compute_laps(&lap_gate(3.0, None, Some(5)));

        assert_eq!(
            a.laps_completed,
            vec![0.0; 5]
                .into_iter()
                .chain([1.0, 1.0, 2.0])
                .collect::<Vec<_>>()
        );
        assert_eq!(a.total_laps, 5.0);
        assert_eq!(a.get_lap(ATTR_LAP, 6), 2.0);
        assert_eq!(a.get_lap(LAP_LAPS_TO_GO, 6), 4.0);
    }

    #[test]
    fn compute_laps_dedupes_jitter_at_radius_boundary() {
        const FAR: f64 = 40.004;
        let mut a = lap_activity(&[
            (40.0, 0.0),
            (FAR, 300.0),
            (FAR, 600.0),
            (40.0001, 900.0),  // lap 1: in radius
            (40.00027, 920.0), // ~30 m: just outside
            (40.00007, 940.0), // back inside — same pass, must not double-count
            (FAR, 1200.0),
            (40.0, 1500.0), // lap 2
        ]);
        a.compute_laps(&lap_gate(0.0, None, None));

        assert_eq!(a.laps_completed.last(), Some(&2.0));
        assert_eq!(a.total_laps, 2.0);
    }

    #[test]
    fn compute_laps_race_end_excludes_cooldown_crossings() {
        const FAR: f64 = 40.004;
        let mut a = lap_activity(&[
            (40.0, 0.0), // race start at t=0
            (FAR, 300.0),
            (FAR, 600.0),
            (40.0, 900.0), // lap 1 at t=3
            (FAR, 1200.0),
            (FAR, 1500.0),
            (40.0, 1800.0), // lap 2 — finish, t=6
            (FAR, 2100.0),
            (FAR, 2400.0),
            (40.0, 2700.0), // cooldown pass at t=9 — must not count
        ]);
        a.compute_laps(&lap_gate(0.0, Some(6.0), None));

        // Auto total stops at the finish handle; the cooldown crossing
        // advances nothing.
        assert_eq!(a.total_laps, 2.0);
        assert_eq!(a.laps_completed.last(), Some(&2.0));
        assert_eq!(a.get_lap(ATTR_LAP, 9), 2.0);
        assert_eq!(a.get_lap(LAP_LAPS_TO_GO, 9), 0.0);
    }

    #[test]
    fn lap_metrics_read_zero_without_gate() {
        let a = lap_activity(&[(40.0, 0.0), (40.004, 300.0)]);
        assert_eq!(a.get_lap(ATTR_LAP, 1), 0.0);
        assert_eq!(a.get_lap(LAP_LAPS_TO_GO, 1), 0.0);
        assert_eq!(a.get_lap(LAP_FRACTION, 1), 0.0);
    }

    #[test]
    fn lap_counter_survives_scene_resampling_as_step_series() {
        const FAR: f64 = 40.004;
        // One-second cadence: gate at t=0, lap crossing at t=3, ride ends t=5.
        let mut a = lap_activity(&[
            (40.0, 0.0),
            (FAR, 300.0),
            (FAR, 600.0),
            (40.0, 900.0), // lap 1 at t=3
            (FAR, 1200.0),
            (FAR, 1500.0),
        ]);
        a.elapsed_seconds = vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0];

        let mut scene = wall_clock_scene(0.0, 5.0, 2);
        scene.lap_gate = Some(lap_gate(0.0, None, None));
        let sampled = a.sample_for_scene(&scene, false).unwrap();

        assert_eq!(sampled.total_laps, 1.0);
        // Steps (never interpolates) from 0 to 1 exactly at the crossing.
        let mid = sampled
            .laps_completed
            .iter()
            .position(|&c| c == 1.0)
            .expect("crossing must survive resampling");
        assert!(sampled.laps_completed[..mid].iter().all(|&c| c == 0.0));
        assert!(sampled.laps_completed[mid..].iter().all(|&c| c == 1.0));
        assert_eq!(sampled.get_lap(ATTR_LAP, 0), 1.0);

        // Without a gate the resampled series stays empty → metrics read 0.
        let mut b = lap_activity(&[(40.0, 0.0), (FAR, 300.0), (40.0, 600.0)]);
        b.elapsed_seconds = vec![0.0, 1.0, 2.0];
        let sampled = b
            .sample_for_scene(&wall_clock_scene(0.0, 2.0, 2), false)
            .unwrap();
        assert!(sampled.laps_completed.is_empty());
        assert_eq!(sampled.get_lap(ATTR_LAP, 0), 0.0);
    }

    #[test]
    fn parses_gpx_lean_extension_as_degrees() {
        let gpx = r#"
        <gpx xmlns:ext="https://example.com/gpx/extensions">
          <trk><trkseg>
            <trkpt lat="14.087824807012469" lon="120.97403787326296">
              <time>2026-05-17T00:49:21.754Z</time>
              <extensions><ext:lean>0.21519530465830444</ext:lean></extensions>
            </trkpt>
          </trkseg></trk>
        </gpx>
        "#;
        let activity = Activity::parse_gpx(gpx).unwrap();
        let expected = 0.21519530465830444_f64.to_degrees();

        assert!(activity.valid_attributes.contains(&ATTR_LEAN.to_string()));
        assert!((activity.lean[0] - expected).abs() < 1e-12);
        assert!((activity.get_scalar(ATTR_LEAN, 0) - expected).abs() < 1e-12);
    }

    #[test]
    fn parses_gpx_di2_gear_extensions() {
        let gpx = r#"
        <gpx>
          <trk><trkseg>
            <trkpt lat="1" lon="2">
              <time>2026-01-01T00:00:00Z</time>
              <extensions><front_gear>2</front_gear><rear_gear>11</rear_gear></extensions>
            </trkpt>
            <trkpt lat="1" lon="2.001">
              <time>2026-01-01T00:00:01Z</time>
              <extensions><front_gear>2</front_gear><rear_gear>12</rear_gear></extensions>
            </trkpt>
          </trkseg></trk>
        </gpx>
        "#;
        let activity = Activity::parse_gpx(gpx).unwrap();
        assert!(activity.valid_attributes.contains(&ATTR_GEAR.to_string()));
        assert!(
            activity
                .valid_attributes
                .contains(&ATTR_FRONT_GEAR.to_string())
        );
        assert!(
            activity
                .valid_attributes
                .contains(&ATTR_REAR_GEAR.to_string())
        );
        assert_eq!(activity.front_gear, vec![2.0, 2.0]);
        assert_eq!(activity.rear_gear, vec![11.0, 12.0]);
        assert_eq!(activity.gear, vec![211.0, 212.0]);
        assert_eq!(decode_gear(activity.gear[1]), Some((2, 12)));
    }

    // The three tests below prove the asserted speeds can only have come from
    // the file's native speed field (not a computed haversine/dt fallback),
    // across each format's real-world field-name variants. The GPX and TCX
    // tests pin two points to an *identical* position so a computed speed would
    // be ~0; the FIT test uses a single point, whose fallback speed is always
    // 0.0 by definition (index 0 has no previous point to derive from).

    #[test]
    fn gpx_native_speed_field_variants() {
        // Parse two trackpoints whose speed is expressed by `inner`; the
        // local-name match collapses every namespace prefix to "speed".
        let speeds = |inner_a: &str, inner_b: &str| {
            let gpx = format!(
                r#"<gpx xmlns:gpxtpx="http://www.garmin.com/xmlschemas/TrackPointExtension/v2"
                        xmlns:gpxdata="http://www.cluetrust.com/XML/GPXDATA/1/0">
                  <trk><trkseg>
                    <trkpt lat="1" lon="2"><time>2026-01-01T00:00:00Z</time>{inner_a}</trkpt>
                    <trkpt lat="1" lon="2"><time>2026-01-01T00:00:01Z</time>{inner_b}</trkpt>
                  </trkseg></trk>
                </gpx>"#
            );
            Activity::parse_gpx(&gpx).unwrap().speed
        };

        // bare <speed> (GPX 1.0, direct child of trkpt)
        assert_eq!(
            speeds("<speed>5.0</speed>", "<speed>7.5</speed>"),
            vec![5.0, 7.5]
        );

        // Garmin TrackPointExtension v2: <gpxtpx:speed> inside <extensions>
        assert_eq!(
            speeds(
                "<extensions><gpxtpx:TrackPointExtension><gpxtpx:speed>5.0</gpxtpx:speed></gpxtpx:TrackPointExtension></extensions>",
                "<extensions><gpxtpx:TrackPointExtension><gpxtpx:speed>7.5</gpxtpx:speed></gpxtpx:TrackPointExtension></extensions>",
            ),
            vec![5.0, 7.5]
        );

        // Cluetrust GPXDATA extension: <gpxdata:speed>
        assert_eq!(
            speeds(
                "<extensions><gpxdata:speed>5.0</gpxdata:speed></extensions>",
                "<extensions><gpxdata:speed>7.5</gpxdata:speed></extensions>",
            ),
            vec![5.0, 7.5]
        );
    }

    #[test]
    fn tcx_native_speed_field_variants() {
        // Speed lives in the TPX extension; local-name is "Speed" regardless of
        // the namespace prefix (ns3: for Garmin's ActivityExtension, or none).
        let speeds = |speed_a: &str, speed_b: &str| {
            let tcx = format!(
                r#"<TrainingCenterDatabase xmlns:ns3="http://www.garmin.com/xmlschemas/ActivityExtension/v2">
                  <Activities><Activity><Lap><Track>
                    <Trackpoint>
                      <Time>2026-01-01T00:00:00Z</Time>
                      <Position><LatitudeDegrees>1</LatitudeDegrees><LongitudeDegrees>2</LongitudeDegrees></Position>
                      <Extensions><ns3:TPX>{speed_a}</ns3:TPX></Extensions>
                    </Trackpoint>
                    <Trackpoint>
                      <Time>2026-01-01T00:00:01Z</Time>
                      <Position><LatitudeDegrees>1</LatitudeDegrees><LongitudeDegrees>2</LongitudeDegrees></Position>
                      <Extensions><ns3:TPX>{speed_b}</ns3:TPX></Extensions>
                    </Trackpoint>
                  </Track></Lap></Activity></Activities>
                </TrainingCenterDatabase>"#
            );
            Activity::parse_tcx(&tcx).unwrap().speed
        };

        // Garmin ActivityExtension v2: <ns3:Speed>
        assert_eq!(
            speeds("<ns3:Speed>5.0</ns3:Speed>", "<ns3:Speed>7.5</ns3:Speed>"),
            vec![5.0, 7.5]
        );

        // Prefix-less <Speed> (same local name)
        assert_eq!(
            speeds("<Speed>5.0</Speed>", "<Speed>7.5</Speed>"),
            vec![5.0, 7.5]
        );
    }

    #[test]
    fn fit_native_speed_field_variants() {
        use fitparser::profile::MesgNum;
        use fitparser::{FitDataField, FitDataRecord, Value};

        // The FIT field-definition number is required by FitDataField::new, but
        // from_fit_records matches purely on field.name() and never reads the
        // number, so every field here uses the same placeholder to make clear
        // the specific value carries no meaning in this test.
        const FIELD_NUM_UNUSED: u8 = 0;

        // One GPS Record with a position plus the given speed fields, pushed in
        // order (first-seen wins in the parser). Position is required for the
        // point to be kept; its value is irrelevant to the speed assertion.
        let speed_of = |speed_fields: &[(&str, f64)]| {
            let mut rec = FitDataRecord::new(MesgNum::Record);
            rec.push(FitDataField::new(
                "position_lat".into(),
                FIELD_NUM_UNUSED,
                Value::SInt32(0),
                "semicircles".into(),
            ));
            rec.push(FitDataField::new(
                "position_long".into(),
                FIELD_NUM_UNUSED,
                Value::SInt32(0),
                "semicircles".into(),
            ));
            for &(name, v) in speed_fields {
                rec.push(FitDataField::new(
                    name.into(),
                    FIELD_NUM_UNUSED,
                    Value::Float64(v),
                    "m/s".into(),
                ));
            }
            Activity::from_fit_records(vec![rec]).unwrap().speed
        };

        // plain `speed`
        assert_eq!(speed_of(&[("speed", 8.5)]), vec![8.5]);
        // `enhanced_speed`
        assert_eq!(speed_of(&[("enhanced_speed", 9.25)]), vec![9.25]);
        // both present, speed seen first → enhanced_speed still wins
        assert_eq!(
            speed_of(&[("speed", 8.5), ("enhanced_speed", 9.25)]),
            vec![9.25]
        );
    }

    #[test]
    fn falls_back_to_computed_speed_without_native() {
        // No <speed> element: speed is derived from position/time deltas.
        let gpx = r#"
        <gpx>
          <trk><trkseg>
            <trkpt lat="1" lon="2">
              <time>2026-01-01T00:00:00Z</time>
            </trkpt>
            <trkpt lat="1" lon="2.001">
              <time>2026-01-01T00:00:01Z</time>
            </trkpt>
          </trkseg></trk>
        </gpx>
        "#;
        let activity = Activity::parse_gpx(gpx).unwrap();
        assert_eq!(activity.speed[0], 0.0);
        // ~0.001° of longitude at the equator over 1s → tens of m/s, clearly > 0.
        assert!(activity.speed[1] > 0.0);
    }

    #[test]
    fn gear_resampling_holds_previous_state() {
        let mut activity = Activity::default();
        activity.elapsed_seconds = vec![0.0, 2.0];
        activity.speed = vec![10.0, 20.0];
        activity.distance = vec![0.0, 20.0];
        activity.course = vec![(0.0, 0.0), (0.0, 2.0)];
        activity.front_gear = vec![2.0, 2.0];
        activity.rear_gear = vec![11.0, 12.0];
        activity.gear = vec![211.0, 212.0];
        activity.valid_attributes = vec![
            ATTR_SPEED.to_string(),
            ATTR_DISTANCE.to_string(),
            ATTR_GEAR.to_string(),
            ATTR_FRONT_GEAR.to_string(),
            ATTR_REAR_GEAR.to_string(),
        ];

        let sampled = activity
            .sample_for_scene(&wall_clock_scene(0.0, 2.0, 2), false)
            .unwrap();

        assert_eq!(sampled.gear, vec![211.0, 211.0, 211.0, 211.0]);
        assert_eq!(sampled.rear_gear, vec![11.0, 11.0, 11.0, 11.0]);
    }

    #[test]
    fn wall_clock_resampling_interpolates_normal_intervals() {
        let mut activity = Activity::default();
        activity.elapsed_seconds = vec![0.0, 2.0];
        activity.speed = vec![10.0, 20.0];
        activity.distance = vec![0.0, 20.0];
        activity.course = vec![(0.0, 0.0), (0.0, 2.0)];
        activity.valid_attributes = vec![ATTR_SPEED.to_string(), ATTR_DISTANCE.to_string()];

        let sampled = activity
            .sample_for_scene(&wall_clock_scene(0.0, 2.0, 2), false)
            .unwrap();

        assert_eq!(sampled.data_len(), 4);
        assert_eq!(sampled.speed, vec![10.0, 12.5, 15.0, 17.5]);
        assert_eq!(sampled.distance, vec![0.0, 5.0, 10.0, 15.0]);
    }

    #[test]
    fn wall_clock_resampling_freezes_inside_pause_gap() {
        let mut activity = Activity::default();
        activity.elapsed_seconds = vec![0.0, 1.0, 30.0];
        activity.speed = vec![10.0, 12.0, 40.0];
        activity.distance = vec![0.0, 12.0, 100.0];
        activity.course = vec![(0.0, 0.0), (0.0, 1.0), (0.0, 30.0)];
        activity.valid_attributes = vec![ATTR_SPEED.to_string(), ATTR_DISTANCE.to_string()];

        let sampled = activity
            .sample_for_scene(&wall_clock_scene(1.0, 4.0, 1), false)
            .unwrap();

        assert_eq!(sampled.speed, vec![12.0, 12.0, 12.0]);
        assert_eq!(sampled.distance, vec![12.0, 12.0, 12.0]);
    }

    /// A wall-clock activity of `n` evenly-spaced samples over `duration_s`,
    /// covering 0→1000 m and climbing 100→200 m linearly.
    fn linear_activity(duration_s: f64, n: usize) -> Activity {
        let mut a = Activity::default();
        a.valid_attributes = vec![
            ATTR_DISTANCE.to_string(),
            ATTR_ELEVATION.to_string(),
            ATTR_SPEED.to_string(),
        ];
        for i in 0..n {
            let f = i as f64 / (n - 1) as f64;
            a.elapsed_seconds.push(f * duration_s);
            a.distance.push(f * 1000.0);
            a.elevation.push(100.0 + f * 100.0);
            a.speed.push(10.0);
            a.course.push((0.0, f * 0.01));
        }
        a.total_activity_distance = 1000.0;
        a.total_activity_elapsed = duration_s;
        a.activity_summary = a.compute_summary();
        a
    }

    #[test]
    fn target_duration_sets_frame_count_and_sweeps_full_window() {
        let a = linear_activity(100.0, 101);
        let mut scene = wall_clock_scene(0.0, 100.0, 10);
        scene.target_duration = Some(5.0);
        let out = a.sample_for_scene(&scene, false).unwrap();

        // 5 s of output at 10 fps, independent of the 100 s ride length.
        assert_eq!(out.data_len(), 50);
        // elapsed_seconds is ride time, not output time: sweeps 0 → full window.
        assert!((out.elapsed_seconds.first().copied().unwrap() - 0.0).abs() < 1e-9);
        assert!((out.elapsed_seconds.last().copied().unwrap() - 100.0).abs() < 1e-6);
        // Distance sweeps the whole ride too.
        assert!((out.distance.last().copied().unwrap() - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn no_target_duration_keeps_realtime_frame_count() {
        let a = linear_activity(4.0, 41);
        let scene = wall_clock_scene(0.0, 4.0, 2); // window 4 s · 2 fps = 8 frames
        let out = a.sample_for_scene(&scene, false).unwrap();
        assert_eq!(out.data_len(), 8);
    }

    #[test]
    fn running_elevation_gain_accumulates_to_series_total() {
        let a = linear_activity(100.0, 101);
        let scene = wall_clock_scene(0.0, 100.0, 4);
        let out = a.sample_for_scene(&scene, false).unwrap();
        let n = out.data_len();

        // Monotonic non-decreasing.
        for w in out.cumulative_elevation_gain.windows(2) {
            assert!(w[1] >= w[0]);
        }
        // Final running gain matches the aggregate over the same series.
        let (gain, _) = elevation_gain_loss(&out.elevation, ELEVATION_NOISE_THRESHOLD_M);
        let last = out.get_running(RUN_ELEVATION_GAIN, n - 1);
        assert!((last - gain).abs() < 1e-9, "last={last} gain={gain}");
        assert!(last > 90.0, "expected ~100 m of climb, got {last}");
        // A pure climb accrues no loss.
        assert_eq!(out.get_running(RUN_ELEVATION_LOSS, n - 1), 0.0);
    }

    #[test]
    fn running_time_and_distance_read_current_point() {
        let a = linear_activity(100.0, 101);
        let scene = wall_clock_scene(0.0, 100.0, 4);
        let out = a.sample_for_scene(&scene, false).unwrap();
        let mid = out.data_len() / 2;

        assert_eq!(out.get_running(RUN_TIME, mid), out.elapsed_seconds[mid]);
        assert_eq!(out.get_running(RUN_DISTANCE, mid), out.distance[mid]);
        // Unknown running token resolves to 0.
        assert_eq!(out.get_running("not_a_metric", mid), 0.0);
    }

    #[test]
    fn plot_data_uses_distance_x_axis_when_requested() {
        let mut activity = Activity::default();
        activity.distance = vec![0.0, 5.0, 15.0];
        activity.elevation = vec![10.0, 20.0, 10.0];
        activity.speed = vec![1.0, 2.0, 3.0];
        activity.course = vec![(0.0, 0.0), (0.0, 1.0), (0.0, 2.0)];
        activity.route = vec![(0.0, 0.0), (0.0, 1.0), (0.0, 2.0)];
        activity.valid_attributes = vec![
            ATTR_DISTANCE.to_string(),
            ATTR_ELEVATION.to_string(),
            ATTR_SPEED.to_string(),
            ATTR_COURSE.to_string(),
        ];

        // Default (time/sample-index) x-axis.
        let (x, y) = activity.plot_data(ATTR_ELEVATION, None);
        assert_eq!(x, vec![0.0, 1.0, 2.0]);
        assert_eq!(y, vec![10.0, 20.0, 10.0]);

        // Distance-based x-axis: x mirrors the distance array.
        let (x, y) = activity.plot_data(ATTR_ELEVATION, Some("distance"));
        assert_eq!(x, vec![0.0, 5.0, 15.0]);
        assert_eq!(y, vec![10.0, 20.0, 10.0]);

        // Course plots ignore x_axis (keep geographic lon/lat).
        let (x, y) = activity.plot_data(ATTR_COURSE, Some("distance"));
        assert_eq!(x, vec![0.0, 1.0, 2.0]);
        assert_eq!(y, vec![0.0, 0.0, 0.0]);

        // Distance plot never becomes a diagonal line: falls back to indices.
        let (x, y) = activity.plot_data(ATTR_DISTANCE, Some("distance"));
        assert_eq!(x, vec![0.0, 1.0, 2.0]);
        assert_eq!(y, vec![0.0, 5.0, 15.0]);
    }
}

// ─── XML helpers ──────────────────────────────────────────────────────────

fn local_name(name: &[u8]) -> &str {
    let s = std::str::from_utf8(name).unwrap_or("");
    s.rfind(':').map(|i| &s[i + 1..]).unwrap_or(s)
}

fn attr_f64(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<f64> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| {
            std::str::from_utf8(a.value.as_ref())
                .ok()
                .and_then(|s| s.parse().ok())
        })
}
