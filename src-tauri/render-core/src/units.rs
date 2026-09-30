//! Single source of truth for metric → display-unit conversion.
//!
//! GPX-native units are fixed: speed m/s, distance m, elevation m,
//! temperature °C. Every render path (value elements in `frame.rs`, chart
//! point labels in `chart.rs`) resolves through [`resolve`] so the conversion
//! and suffix never drift between code paths.

use crate::activity::{
    ATTR_CADENCE, ATTR_DISTANCE, ATTR_ELEVATION, ATTR_GRADIENT, ATTR_HEARTRATE, ATTR_POWER,
    ATTR_POWER_TO_WEIGHT, ATTR_SPEED, ATTR_TEMPERATURE, FT_CONVERSION, KMH_CONVERSION,
    MI_CONVERSION, MPH_CONVERSION,
};

/// Affine conversion from a GPX-native value to a display value:
/// `display = raw * scale + offset`. `offset` is non-zero only for
/// temperature (°C → °F).
#[derive(Clone, Copy, Debug)]
pub struct Conversion {
    pub scale: f64,
    pub offset: f64,
}

impl Conversion {
    pub const IDENTITY: Conversion = Conversion {
        scale: 1.0,
        offset: 0.0,
    };

    fn scale(scale: f64) -> Conversion {
        Conversion { scale, offset: 0.0 }
    }

    pub fn apply(&self, raw: f64) -> f64 {
        raw * self.scale + self.offset
    }
}

/// Resolve a metric attribute + unit token into its conversion and an
/// uppercase display suffix (e.g. `"KM/H"`, `"FT"`).
///
/// Accepts the precise per-metric tokens (`"kmh"`, `"mph"`, `"ms"`, `"km"`,
/// `"mi"`, `"m"`, `"ft"`, `"c"`, `"f"`), the legacy `"metric"`/`"imperial"`
/// tokens, and `None` (→ metric default) — so every existing template keeps
/// rendering unchanged. Metrics with no unit choice (gradient, power,
/// cadence, heartrate) and unknown attributes return the identity conversion
/// and echo the provided token uppercased.
pub fn resolve(attr: &str, unit: Option<&str>) -> (Conversion, String) {
    let u = unit.unwrap_or("").to_ascii_lowercase();
    match attr {
        ATTR_SPEED => match u.as_str() {
            "ms" | "m/s" => (Conversion::scale(1.0), "M/S".into()),
            "mph" | "imperial" => (Conversion::scale(MPH_CONVERSION), "MPH".into()),
            _ => (Conversion::scale(KMH_CONVERSION), "KM/H".into()), // kmh / metric / default
        },
        ATTR_DISTANCE => match u.as_str() {
            "m" => (Conversion::scale(1.0), "M".into()),
            "mi" | "imperial" => (Conversion::scale(MI_CONVERSION), "MI".into()),
            _ => (Conversion::scale(0.001), "KM".into()), // km / metric / default
        },
        ATTR_ELEVATION => match u.as_str() {
            "ft" | "imperial" => (Conversion::scale(FT_CONVERSION), "FT".into()),
            _ => (Conversion::scale(1.0), "M".into()), // m / metric / default
        },
        ATTR_TEMPERATURE => match u.as_str() {
            "f" | "imperial" => (
                Conversion {
                    scale: 1.8,
                    offset: 32.0,
                },
                "F".into(),
            ),
            _ => (Conversion::IDENTITY, "C".into()), // c / metric / default
        },
        _ => (Conversion::IDENTITY, u.to_uppercase()),
    }
}

/// Human-facing unit label for the "auto" value suffix, including the
/// conventional leading space (`" km"`, `" mph"`, `" W"`) or degree spacing
/// (`"°C"`, `"%"` — no space). Unlike [`resolve`]'s uppercase chart suffix,
/// these are cased the way a rider expects to read them in an overlay.
///
/// `attr` is the metric attribute (summary metrics should be mapped to their
/// base metric by the caller). Returns `None` for metrics that have no natural
/// unit label (time, gear), so "auto" simply appends nothing there.
pub fn display_suffix(attr: &str, unit: Option<&str>) -> Option<String> {
    let u = unit.unwrap_or("").to_ascii_lowercase();
    let s = match attr {
        ATTR_SPEED => match u.as_str() {
            "ms" | "m/s" => " m/s",
            "mph" | "imperial" => " mph",
            _ => " km/h", // kmh / metric / default
        },
        ATTR_DISTANCE => match u.as_str() {
            "m" => " m",
            "mi" | "imperial" => " mi",
            _ => " km", // km / metric / default
        },
        ATTR_ELEVATION => match u.as_str() {
            "ft" | "imperial" => " ft",
            _ => " m", // m / metric / default
        },
        ATTR_TEMPERATURE => match u.as_str() {
            "f" | "imperial" => "°F",
            _ => "°C", // c / metric / default
        },
        ATTR_POWER => " W",
        ATTR_POWER_TO_WEIGHT => " W/kg",
        ATTR_HEARTRATE => " bpm",
        ATTR_CADENCE => " rpm",
        ATTR_GRADIENT => "%",
        _ => return None,
    };
    Some(s.to_string())
}

/// Whether a metric has a metric/imperial distinction (speed, distance,
/// elevation, temperature). Metrics without one (power, cadence, heartrate,
/// gradient, gear, time) render identically in either system, so the
/// scene-level unit toggle skips them.
pub fn has_unit_system(attr: &str) -> bool {
    matches!(
        attr,
        ATTR_SPEED | ATTR_DISTANCE | ATTR_ELEVATION | ATTR_TEMPERATURE
    )
}

/// Render a unit string in the app's compound-unit style. With `compact` set,
/// drop the `/` separator that only compound units carry (`"km/h"`→`"kmh"`,
/// `"m/s"`→`"ms"`); every other unit (`"mph"`, `"mi"`, `"ft"`, `"%"`, `"W"`,
/// `"°C"`, …) has no separator and passes through unchanged. Driven by the
/// app-wide `scene.compact_units` preference.
pub fn compact_unit(s: &str, compact: bool) -> String {
    if compact {
        s.replace('/', "")
    } else {
        s.to_string()
    }
}

/// Standalone unit label for a metric under the active unit system, cased the
/// way a rider reads it in an overlay and *without* the leading space
/// [`display_suffix`] carries (`"mph"`, `"km/h"`, `"mi"`, `"ft"`, `"°C"`,
/// `"%"`, `"W"`). `compact` applies the [`compact_unit`] style. Used by
/// unit-representing labels (`label.unit_of`) so a standalone "mph"/"mi"
/// caption tracks the settings toggle instead of being frozen static text.
/// Returns `None` for metrics with no natural unit label (time, gear) — the
/// caller leaves such a label's text untouched.
pub fn unit_label(attr: &str, unit: Option<&str>, compact: bool) -> Option<String> {
    display_suffix(attr, unit).map(|s| compact_unit(s.trim_start(), compact))
}

/// Convert a display-unit distance target back to metres (used by the
/// "until_custom" distance-reference mode). Inverse of the distance branch of
/// [`resolve`]; distance has no offset so this is a plain divide.
pub fn distance_target_to_m(target: f64, unit: Option<&str>) -> f64 {
    let (conv, _) = resolve(ATTR_DISTANCE, unit);
    target / conv.scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_tokens_match_precise_tokens() {
        // metric/None map to the metric default; imperial to the imperial unit.
        for (attr, metric, imperial) in [
            (ATTR_SPEED, "kmh", "mph"),
            (ATTR_DISTANCE, "km", "mi"),
            (ATTR_ELEVATION, "m", "ft"),
            (ATTR_TEMPERATURE, "c", "f"),
        ] {
            let none = resolve(attr, None);
            let met = resolve(attr, Some("metric"));
            let met_precise = resolve(attr, Some(metric));
            let imp = resolve(attr, Some("imperial"));
            let imp_precise = resolve(attr, Some(imperial));
            assert_eq!(none.0.scale, met_precise.0.scale, "{attr} None vs metric");
            assert_eq!(met.0.scale, met_precise.0.scale, "{attr} metric");
            assert_eq!(imp.0.scale, imp_precise.0.scale, "{attr} imperial");
            assert_eq!(imp.0.offset, imp_precise.0.offset, "{attr} imperial offset");
        }
    }

    #[test]
    fn distance_target_round_trips() {
        assert!((distance_target_to_m(5.0, Some("km")) - 5000.0).abs() < 1e-6);
        assert!((distance_target_to_m(800.0, Some("m")) - 800.0).abs() < 1e-6);
        assert!((distance_target_to_m(1.0, Some("mi")) - 1609.34).abs() < 1.0);
        assert!((distance_target_to_m(5.0, Some("metric")) - 5000.0).abs() < 1e-6);
        assert!((distance_target_to_m(1.0, Some("imperial")) - 1609.34).abs() < 1.0);
    }

    #[test]
    fn auto_suffix_tracks_unit_and_metric() {
        // Unit-convertible metrics follow the unit token.
        assert_eq!(
            display_suffix(ATTR_DISTANCE, Some("km")).as_deref(),
            Some(" km")
        );
        assert_eq!(
            display_suffix(ATTR_DISTANCE, Some("mi")).as_deref(),
            Some(" mi")
        );
        assert_eq!(
            display_suffix(ATTR_SPEED, Some("mph")).as_deref(),
            Some(" mph")
        );
        assert_eq!(
            display_suffix(ATTR_TEMPERATURE, Some("f")).as_deref(),
            Some("°F")
        );
        // Legacy metric/imperial tokens and None resolve to the same labels.
        assert_eq!(display_suffix(ATTR_DISTANCE, None).as_deref(), Some(" km"));
        assert_eq!(
            display_suffix(ATTR_DISTANCE, Some("imperial")).as_deref(),
            Some(" mi")
        );
        // Non-convertible metrics have a fixed label regardless of unit.
        assert_eq!(display_suffix(ATTR_POWER, None).as_deref(), Some(" W"));
        assert_eq!(
            display_suffix(ATTR_HEARTRATE, None).as_deref(),
            Some(" bpm")
        );
        assert_eq!(display_suffix(ATTR_GRADIENT, None).as_deref(), Some("%"));
        // Metrics with no natural unit label yield nothing.
        assert_eq!(display_suffix("time", None), None);
        assert_eq!(display_suffix("gear", None), None);
    }

    #[test]
    fn temperature_is_affine() {
        let (c, _) = resolve(ATTR_TEMPERATURE, Some("f"));
        assert!((c.apply(0.0) - 32.0).abs() < 1e-9);
        assert!((c.apply(100.0) - 212.0).abs() < 1e-9);
    }
}
