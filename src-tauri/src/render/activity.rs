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
pub const ATTR_REAR_GEAR: &str = "rear_gear";
pub const ATTR_SPEED: &str = "speed";
pub const ATTR_TIME: &str = "time";
pub const ATTR_TEMPERATURE: &str = "temperature";

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
                                pt.speed = current_text.parse::<f64>().ok().filter(|v| v.is_finite());
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
            ATTR_TEMPERATURE,
            ATTR_FRONT_GEAR,
            ATTR_REAR_GEAR,
            ATTR_GEAR,
            ATTR_TIME,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        a
    }

    pub fn parse_gpx(content: &str) -> Result<Self, String> {
        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);

        let mut points: Vec<TrackPoint> = Vec::new();
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
                                pt.speed = current_text.parse::<f64>().ok().filter(|v| v.is_finite());
                            }
                            "TrackPointExtension" => {
                                in_tpx = false;
                            }
                            "extensions" => {
                                in_extensions = false;
                            }
                            tag if !current_point_tag.is_empty() && tag == current_point_tag => {
                                if let Some(pt) = current.take() {
                                    points.push(pt);
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

        activity.total_activity_distance = cum_dist;
        activity.total_activity_elapsed = activity.elapsed_seconds.last().copied().unwrap_or(0.0);
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
        self,
        scene: &crate::render::template::SceneConfig,
        synthetic: bool,
    ) -> Result<Self, String> {
        let fps = scene.fps.max(1);
        let start = scene.start.unwrap_or(0.0).max(0.0);
        self.resample_wall_clock(start, scene.end, fps, synthetic)
    }

    pub fn resample_wall_clock(
        &self,
        start: f64,
        end: Option<f64>,
        fps: u32,
        synthetic: bool,
    ) -> Result<Self, String> {
        if !self.has_wall_clock_time_axis() {
            if synthetic {
                let mut cloned = self.clone();
                cloned.interpolate(fps);
                return Ok(cloned);
            }
            return Err("Wall-clock timeline requires activity timestamps".to_string());
        }

        let fps = fps.max(1);
        let duration = self.elapsed_duration().unwrap_or(0.0);
        let start = start.clamp(0.0, duration);
        let end = end.unwrap_or(duration).clamp(start, duration);
        let frames = ((end - start) * fps as f64).ceil().max(1.0) as usize;
        let gap_threshold = self.wall_clock_gap_threshold();

        let mut out = Activity {
            total_activity_distance: self.total_activity_distance,
            total_activity_elapsed: self.total_activity_elapsed,
            start_time_ms: self.start_time_ms.map(|ms| ms + (start * 1000.0) as i64),
            valid_attributes: self.valid_attributes.clone(),
            ..Activity::default()
        };

        for frame in 0..frames {
            let t = (start + frame as f64 / fps as f64).min(duration);
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
        }

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

    /// Build (x, y) data arrays for a plot of the given attribute.
    pub fn plot_data(&self, attribute: &str) -> (Vec<f64>, Vec<f64>) {
        let scalar = |data: &[f64]| -> (Vec<f64>, Vec<f64>) {
            let x: Vec<f64> = (0..data.len()).map(|i| i as f64).collect();
            (x, data.to_vec())
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
                let x: Vec<f64> = self.course.iter().map(|c| c.1).collect(); // lon
                let y: Vec<f64> = self.course.iter().map(|c| c.0).collect(); // lat
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
    use crate::render::template::SceneConfig;

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
            decimal_rounding: None,
            color: None,
            opacity: None,
            layers: None,
            groups: Vec::new(),
            vars: std::collections::HashMap::new(),
        }
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
        assert_eq!(speeds("<speed>5.0</speed>", "<speed>7.5</speed>"), vec![5.0, 7.5]);

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
