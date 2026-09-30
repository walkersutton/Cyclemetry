mod activity;
mod recent;
mod render;
mod strava;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Manager;

// ─── Path helpers ─────────────────────────────────────────────────────────────

/// Per-user writable data root: `~/Library/Application Support/com.cyclemetry/`.
/// Resolved once from Tauri's `app_data_dir()` in `setup()`. Persists across
/// app updates (unlike the bundle) and reboots (unlike `/tmp`).
static APP_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

fn app_data_base() -> PathBuf {
    APP_DATA_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir().join("cyclemetry"))
}

/// Where opened/uploaded GPX activities are stored.
fn uploads_dir() -> PathBuf {
    let dir = app_data_base().join("uploads");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// User-editable templates directory (created + installed community templates).
fn templates_user_dir() -> PathBuf {
    let dir = app_data_base().join("templates");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// User-installed custom fonts directory. Checked by the renderer (via
/// `load_typeface`) and listed alongside bundled fonts.
fn fonts_user_dir() -> PathBuf {
    let dir = app_data_base().join("fonts");
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn system_font_families() -> Vec<String> {
    let mgr = skia_safe::FontMgr::default();
    let mut set = std::collections::BTreeSet::new();
    for name in mgr.family_names() {
        let name = name.trim();
        if !name.is_empty() && !name.starts_with('.') {
            set.insert(name.to_string());
        }
    }

    for family in [
        "Arial",
        "Helvetica",
        "Times New Roman",
        "Courier New",
        "DejaVu Sans",
        "Liberation Sans",
        "Noto Sans",
        "sans-serif",
    ] {
        if mgr
            .match_family_style(family, skia_safe::FontStyle::normal())
            .is_some()
        {
            set.insert(family.to_string());
        }
    }

    set.into_iter().collect()
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum FontSource {
    Bundled,
    Custom,
    System,
}

#[derive(serde::Serialize)]
struct FontItem {
    value: String,
    label: String,
    source: FontSource,
}

fn font_file_label(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
        .to_string()
}

fn font_file_items(dir: PathBuf, source: FontSource) -> Vec<FontItem> {
    let mut items = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if let Some(name) = e.file_name().to_str() {
                let lower = name.to_lowercase();
                if lower.ends_with(".ttf") || lower.ends_with(".otf") {
                    items.push(FontItem {
                        value: name.to_string(),
                        label: font_file_label(name),
                        source,
                    });
                }
            }
        }
    }
    items
}

fn window_size_file() -> PathBuf {
    app_data_base().join("window-size.json")
}

fn save_window_size(width: u32, height: u32) {
    if let Ok(json) = serde_json::to_string(&serde_json::json!({"width": width, "height": height}))
    {
        std::fs::write(window_size_file(), json).ok();
    }
}

fn load_window_size() -> Option<(u32, u32)> {
    let text = std::fs::read_to_string(window_size_file()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let w = v["width"].as_u64()? as u32;
    let h = v["height"].as_u64()? as u32;
    Some((w, h))
}

/// Default render output directory, platform-appropriate.
fn default_output_dir() -> PathBuf {
    let dir = {
        #[cfg(windows)]
        {
            std::env::var("USERPROFILE")
                .map(|h| PathBuf::from(h).join("Videos").join("Cyclemetry"))
                .unwrap_or_else(|_| std::env::temp_dir().join("Cyclemetry"))
        }
        #[cfg(not(windows))]
        {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join("Movies").join("Cyclemetry"))
                .unwrap_or_else(|_| std::env::temp_dir().join("Cyclemetry"))
        }
    };
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn template_display_name(s: &str) -> String {
    s.replace('_', " ")
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn template_display_names_from_metadata(
    metadata: &serde_json::Value,
) -> std::collections::HashMap<String, String> {
    metadata
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|template| {
            let name = template.get("name")?.as_str()?.to_string();
            let display = template.get("displayName")?.as_str()?.to_string();
            Some((name, display))
        })
        .collect()
}

fn template_metadata_display_name(
    display_names: &std::collections::HashMap<String, String>,
    name: &str,
) -> String {
    display_names
        .get(name)
        .cloned()
        .unwrap_or_else(|| template_display_name(name))
}

/// Read a JPG into a base64 `data:` URL value, or `None` if it doesn't exist.
fn jpg_data_url(path: &Path) -> Option<serde_json::Value> {
    if !path.exists() {
        return None;
    }
    use base64::Engine;
    std::fs::read(path).ok().map(|bytes| {
        let enc = base64::engine::general_purpose::STANDARD.encode(&bytes);
        serde_json::Value::String(format!("data:image/jpeg;base64,{enc}"))
    })
}

/// Resolve a template's preview image.
/// 1. User-saved preview next to the json (`{user_dir}/{base}.jpg`).
/// 2. Dev: the repo's `templates/{base}/preview.jpg` (inlined as a data URL).
/// 3. Release: the GitHub raw URL for the bundled template preview.
fn resolve_preview_value(user_dir: &Path, base: &str) -> serde_json::Value {
    if let Some(v) = jpg_data_url(&user_dir.join(format!("{base}.jpg"))) {
        return v;
    }
    #[cfg(debug_assertions)]
    {
        jpg_data_url(&repo_templates_dir().join(base).join("preview.jpg"))
            .unwrap_or(serde_json::Value::Null)
    }
    #[cfg(not(debug_assertions))]
    {
        serde_json::Value::String(format!("{GITHUB_RAW_TEMPLATES}/{base}/preview.jpg"))
    }
}

// ─── GPX path resolution ──────────────────────────────────────────────────────

/// Resolve a bare GPX filename to an absolute path.
/// Order: absolute path → prod uploads → dev uploads → dev backend → exe dir.
fn resolve_gpx_path(gpx_filename: &str) -> Result<(String, Option<String>), String> {
    let p = Path::new(gpx_filename);
    if p.is_absolute() && p.exists() {
        return Ok((gpx_filename.to_string(), None));
    }

    let basename = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(gpx_filename);

    let prod = uploads_dir().join(basename);
    if prod.exists() {
        return Ok((prod.to_string_lossy().to_string(), None));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.ancestors().find(|p| p.join("backend").exists()) {
            let dev_upload = root.join("backend").join("uploads").join(basename);
            if dev_upload.exists() {
                return Ok((dev_upload.to_string_lossy().to_string(), None));
            }
            let dev_root = root.join("backend").join(basename);
            if dev_root.exists() {
                return Ok((dev_root.to_string_lossy().to_string(), None));
            }
        }
        if let Some(parent) = exe.parent() {
            let bundled = parent.join(basename);
            if bundled.exists() {
                return Ok((bundled.to_string_lossy().to_string(), None));
            }
        }
    }

    Err(format!("GPX file not found: {gpx_filename}"))
}

fn resolve_fonts_dir() -> String {
    if let Ok(exe) = std::env::current_exe() {
        // Dev: walk ancestor dirs until we find one containing resources/fonts/.
        if let Some(root) = exe
            .ancestors()
            .find(|p| p.join("resources").join("fonts").exists())
        {
            return root
                .join("resources")
                .join("fonts")
                .to_string_lossy()
                .to_string();
        }
        // Production macOS .app bundle: Contents/Resources/fonts/
        if let Some(contents) = exe.parent().and_then(|p| p.parent()) {
            let prod = contents.join("Resources").join("fonts");
            if prod.exists() {
                return prod.to_string_lossy().to_string();
            }
        }
        // Production Linux AppImage (APPDIR set by the AppImage runtime) or
        // DEB/RPM install (exe at /usr/bin/; resources at /usr/lib/<name>/).
        #[cfg(target_os = "linux")]
        {
            if let Ok(appdir) = std::env::var("APPDIR") {
                for name in &["Cyclemetry", "cyclemetry"] {
                    let p = PathBuf::from(&appdir)
                        .join("usr/lib")
                        .join(name)
                        .join("fonts");
                    if p.exists() {
                        return p.to_string_lossy().to_string();
                    }
                }
            }
            if let Some(exe_dir) = exe.parent() {
                for rel in &[
                    "../lib/Cyclemetry/fonts",
                    "../lib/cyclemetry/fonts",
                    "fonts",
                ] {
                    let p = exe_dir.join(rel);
                    if p.exists() {
                        return p.canonicalize().unwrap_or(p).to_string_lossy().to_string();
                    }
                }
            }
        }
        // Production Windows: fonts/ next to the exe or inside resources/
        #[cfg(windows)]
        if let Some(exe_dir) = exe.parent() {
            for candidate in &[
                exe_dir.join("fonts"),
                exe_dir.join("resources").join("fonts"),
            ] {
                if candidate.exists() {
                    return candidate.to_string_lossy().to_string();
                }
            }
        }
    }
    "./fonts".to_string()
}

/// User-uploaded image assets directory.
fn assets_user_dir() -> PathBuf {
    let dir = app_data_base().join("assets");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// Ordered list of asset search directories: user dir first, then bundled.
fn assets_search_dirs_vec() -> Vec<String> {
    let mut dirs = vec![assets_user_dir().to_string_lossy().to_string()];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe
            .ancestors()
            .find(|p| p.join("templates").join("_assets").is_dir())
        {
            dirs.push(
                root.join("templates")
                    .join("_assets")
                    .to_string_lossy()
                    .to_string(),
            );
        }
        if let Some(contents) = exe.parent().and_then(|p| p.parent()) {
            let prod = contents.join("Resources").join("assets");
            if prod.exists() {
                dirs.push(prod.to_string_lossy().to_string());
            }
        }
        #[cfg(windows)]
        if let Some(exe_dir) = exe.parent() {
            for candidate in &[
                exe_dir.join("assets"),
                exe_dir.join("resources").join("assets"),
            ] {
                if candidate.exists() {
                    dirs.push(candidate.to_string_lossy().to_string());
                }
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(exe_dir) = exe.parent() {
            let app_name = exe
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "cyclemetry".to_string());
            for candidate in &[
                exe_dir.join("assets"),
                exe_dir.join("resources").join("assets"),
                exe_dir
                    .parent()
                    .unwrap_or(exe_dir)
                    .join("lib")
                    .join(&app_name)
                    .join("assets"),
                exe_dir
                    .parent()
                    .unwrap_or(exe_dir)
                    .join("lib")
                    .join(&app_name)
                    .join("resources")
                    .join("assets"),
            ] {
                if candidate.exists() {
                    dirs.push(
                        candidate
                            .canonicalize()
                            .unwrap_or_else(|_| candidate.to_path_buf())
                            .to_string_lossy()
                            .to_string(),
                    );
                }
            }
        }
    }
    dirs
}

fn resolve_output_path(
    template_json: &serde_json::Value,
    output_dir: Option<&str>,
    extension: &str,
) -> String {
    let stem = template_json
        .pointer("/scene/overlay_filename")
        .and_then(|v| v.as_str())
        .unwrap_or("overlay")
        .trim_end_matches(".mov");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi) = unix_to_ymdhm(now);
    let filename = format!("{stem}_{y}{mo:02}{d:02}_{h:02}{mi:02}.{extension}");

    let dir = match output_dir {
        Some(d) if !d.is_empty() => {
            let p = PathBuf::from(d);
            std::fs::create_dir_all(&p).ok();
            p
        }
        _ => default_output_dir(),
    };
    format!("{}/{}", dir.to_string_lossy(), filename)
}

fn unix_to_ymdhm(secs: u64) -> (u64, u64, u64, u64, u64) {
    let mi = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d, h, mi)
}

// ─── Build info ───────────────────────────────────────────────────────────────

#[tauri::command]
fn app_build_info() -> String {
    use chrono::{Local, TimeZone};

    let ts: u64 = env!("CYCLEMETRY_BUILD_TIME").parse().unwrap_or(0);
    let local = Local.timestamp_opt(ts as i64, 0).single();
    match local {
        Some(dt) => format!("build {}", dt.format("%Y-%m-%d %-I:%M %p %Z")),
        None => "build unknown".to_string(),
    }
}

// ─── Template commands ────────────────────────────────────────────────────────

#[tauri::command]
fn backend_list_templates() -> Result<String, String> {
    let mut templates: Vec<serde_json::Value> = Vec::new();
    let dir = templates_user_dir();
    #[cfg(debug_assertions)]
    let display_names = repo_template_display_names();
    #[cfg(not(debug_assertions))]
    let display_names = std::collections::HashMap::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok("[]".to_string());
    };
    for entry in entries.flatten() {
        let Ok(ftype) = entry.file_type() else {
            continue;
        };
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy();
        if ftype.is_file() && name.ends_with(".json") {
            let fname = name.to_string();
            let path = dir.join(&fname);
            if !is_template_json_file(&path) {
                continue;
            }
            let display = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| {
                    v.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| {
                    template_metadata_display_name(&display_names, fname.trim_end_matches(".json"))
                });
            let sidecar = dir.join(format!("{fname}.remote"));
            let type_label = if sidecar.exists() {
                let current = std::fs::read_to_string(&path).unwrap_or_default();
                let reference = std::fs::read_to_string(&sidecar).unwrap_or_default();
                if current.trim() == reference.trim() {
                    "community"
                } else {
                    "community-modified"
                }
            } else {
                "user"
            };
            let base = fname.trim_end_matches(".json");
            let preview_url = resolve_preview_value(&dir, base);
            templates.push(serde_json::json!({
                "id": fname,
                "name": display,
                "type": type_label,
                "preview_url": preview_url
            }));
        }
    }
    Ok(serde_json::to_string(&templates).unwrap_or_else(|_| "[]".to_string()))
}

fn is_template_json_file(path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return false;
    };
    let Some(root) = value.as_object() else {
        return false;
    };
    root.get("scene").is_some_and(serde_json::Value::is_object)
        && root
            .get("elements")
            .is_none_or(|elements| elements.is_array() || elements.is_null())
}

#[tauri::command]
fn backend_get_template(filename: String) -> Result<String, String> {
    let rel = validate_template_path(&filename)?;
    let path = templates_user_dir().join(&rel);
    if !path.exists() {
        return Err(format!("Template not found: {filename}"));
    }
    let contents =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read template: {e}"))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&contents).map_err(|e| format!("Invalid template JSON: {e}"))?;
    serde_json::to_string(&parsed).map_err(|e| e.to_string())
}

#[tauri::command]
fn backend_save_template(
    mut config: serde_json::Value,
    filename: String,
) -> Result<String, String> {
    let rel = validate_template_path(&filename)?;
    let path = templates_user_dir().join(&rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    if let Some(obj) = config.as_object_mut() {
        obj.remove("name");
    }
    let pretty =
        serde_json::to_string_pretty(&config).map_err(|e| format!("Serialize error: {e}"))?;
    std::fs::write(&path, &pretty).map_err(|e| format!("Failed to write template: {e}"))?;
    Ok(
        serde_json::json!({ "message": format!("Template saved to {rel}"), "filename": rel })
            .to_string(),
    )
}

#[tauri::command]
fn backend_import_template(path: String) -> Result<String, String> {
    let src = Path::new(&path);
    if !src.exists() {
        return Err(format!("Template file not found: {path}"));
    }
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());
    if !matches!(ext.as_deref(), Some("json")) {
        return Err("Template must be a .json file".to_string());
    }
    if !is_template_json_file(src) {
        return Err("Selected file is not a Cyclemetry template JSON.".to_string());
    }

    let file_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid template filename".to_string())?;
    let base = file_name.trim_end_matches(".json");
    let sanitized = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    let stem = if sanitized.is_empty() {
        "imported_template".to_string()
    } else {
        sanitized
    };

    let dir = templates_user_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create templates dir: {e}"))?;
    let mut rel = format!("{stem}.json");
    let mut dest = dir.join(&rel);
    let mut n = 2;
    while dest.exists() {
        rel = format!("{stem}_{n}.json");
        dest = dir.join(&rel);
        n += 1;
    }
    std::fs::copy(src, &dest).map_err(|e| format!("Failed to import template: {e}"))?;
    Ok(serde_json::json!({ "filename": rel }).to_string())
}

/// Validate a template path: at most `folder/file.json`, no `..`, must end with `.json`.
fn validate_template_path(filename: &str) -> Result<String, String> {
    let parts: Vec<&str> = filename.splitn(3, '/').collect();
    match parts.as_slice() {
        [file] if file.ends_with(".json") && !file.contains("..") => Ok(file.to_string()),
        [dir, file] if file.ends_with(".json") && !dir.contains("..") && !file.contains("..") => {
            Ok(format!("{dir}/{file}"))
        }
        _ => Err("Invalid template path".to_string()),
    }
}

// ─── Delete / preview commands ───────────────────────────────────────────────

#[tauri::command]
fn backend_delete_template(filename: String) -> Result<String, String> {
    let rel = validate_template_path(&filename)?;
    let user_dir = templates_user_dir();
    let path = user_dir.join(&rel);
    if !path.exists() {
        return Err(format!("Template not found in user templates: {filename}"));
    }
    std::fs::remove_file(&path).map_err(|e| format!("Failed to delete template: {e}"))?;
    let base = rel.trim_end_matches(".json");
    let sidecar = user_dir.join(format!("{base}.json.remote"));
    if sidecar.exists() {
        std::fs::remove_file(&sidecar).ok();
    }
    let preview = user_dir.join(format!("{base}.jpg"));
    if preview.exists() {
        std::fs::remove_file(&preview).ok();
    }
    Ok(serde_json::json!({ "message": format!("Deleted {filename}") }).to_string())
}

#[tauri::command]
fn backend_save_template_preview(filename: String, image_data_url: String) -> Result<(), String> {
    let rel = validate_template_path(&filename)?;
    let base = rel.trim_end_matches(".json").to_string();
    let prefix = "data:image/png;base64,";
    let encoded = image_data_url
        .strip_prefix(prefix)
        .ok_or("Expected data:image/png;base64, prefix")?;
    use base64::Engine;
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("Base64 decode error: {e}"))?;
    let img_data = skia_safe::Data::new_copy(&png_bytes);
    let image =
        skia_safe::Image::from_encoded(img_data).ok_or("Failed to decode PNG image data")?;
    let jpeg_data = image
        .encode(None, skia_safe::EncodedImageFormat::JPEG, 85)
        .ok_or("Failed to encode image as JPEG")?;
    let dest = templates_user_dir().join(format!("{base}.jpg"));
    std::fs::write(&dest, jpeg_data.as_bytes())
        .map_err(|e| format!("Failed to write preview: {e}"))?;
    Ok(())
}

// ─── File-system open commands ────────────────────────────────────────────────

#[tauri::command]
fn backend_open_templates() -> Result<String, String> {
    let dir = templates_user_dir();
    open_path(&dir.to_string_lossy())?;
    Ok(r#"{"message":"Templates folder opened"}"#.to_string())
}

#[tauri::command]
fn backend_open_activities() -> Result<String, String> {
    open_path(&uploads_dir().to_string_lossy())?;
    Ok(r#"{"message":"Activities folder opened"}"#.to_string())
}

#[derive(Serialize)]
struct SavedActivity {
    filename: String,
    /// Unix epoch milliseconds of the *first recorded sample* — the activity's
    /// start time, not when the user added the file. Falls back to the file's
    /// mtime when the source lacks usable timestamps so the entry still sorts
    /// reasonably and shows *some* date.
    start_ms: i64,
    /// Total recorded duration in seconds; None when the file has no time axis.
    duration_s: Option<f64>,
    /// Downsampled route preview: up to ~64 points normalized into a unit
    /// square (y grows downward, aspect preserved and centered) so the
    /// frontend can draw it as an SVG polyline without knowing about
    /// coordinates or projections. Empty when the file has no GPS track.
    track: Vec<[f32; 2]>,
}

/// Equirectangular-project a course into a unit square for the picker's
/// route thumbnail: longitude is scaled by cos(mid-latitude) so the shape
/// isn't stretched, the longer axis spans 0..1, and the shorter axis is
/// centered.
fn preview_track(course: &[(f64, f64)]) -> Vec<[f32; 2]> {
    const MAX_POINTS: usize = 64;
    let pts: Vec<(f64, f64)> = course
        .iter()
        .filter(|(lat, lon)| lat.is_finite() && lon.is_finite() && (*lat != 0.0 || *lon != 0.0))
        .copied()
        .collect();
    if pts.len() < 2 {
        return Vec::new();
    }
    let (min_lat, max_lat) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (lat, _)| {
        (lo.min(*lat), hi.max(*lat))
    });
    let (min_lon, max_lon) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (_, lon)| {
        (lo.min(*lon), hi.max(*lon))
    });
    let cos_mid = ((min_lat + max_lat) / 2.0).to_radians().cos().max(0.01);
    let width = (max_lon - min_lon) * cos_mid;
    let height = max_lat - min_lat;
    let span = width.max(height).max(1e-9);
    let x_off = (1.0 - width / span) / 2.0;
    let y_off = (1.0 - height / span) / 2.0;
    let step = (pts.len() as f64 / MAX_POINTS as f64).max(1.0);
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(MAX_POINTS + 1);
    let mut i = 0.0;
    while (i as usize) < pts.len() {
        let (lat, lon) = pts[i as usize];
        out.push([
            (x_off + (lon - min_lon) * cos_mid / span) as f32,
            (y_off + (max_lat - lat) / span) as f32,
        ]);
        i += step;
    }
    let last = pts[pts.len() - 1];
    out.push([
        (x_off + (last.1 - min_lon) * cos_mid / span) as f32,
        (y_off + (max_lat - last.0) / span) as f32,
    ]);
    out
}

/// List activities in the uploads dir, newest first. Used by the
/// activity picker modal so the user can re-open a recently added GPX
/// without going through the native file dialog again.
#[tauri::command]
fn backend_list_activities() -> Vec<SavedActivity> {
    use std::time::UNIX_EPOCH;
    let mut items: Vec<SavedActivity> = Vec::new();
    let Ok(entries) = std::fs::read_dir(uploads_dir()) else {
        return items;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".gpx") || lower.ends_with(".fit") || lower.ends_with(".tcx")) {
            continue;
        }
        let path = entry.path();
        // symlink_metadata so a broken symlink still shows up (user can delete it).
        let mtime_ms = path
            .symlink_metadata()
            .ok()
            .and_then(|m| m.modified().ok().or_else(|| m.created().ok()))
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        // Prefer the activity's first-sample timestamp; fall back to file mtime
        // so timestamp-less or unreadable files still show *a* date.
        let parsed = render::activity::Activity::from_file(&path.to_string_lossy()).ok();
        let start_ms = parsed
            .as_ref()
            .and_then(|a| a.start_time_ms)
            .unwrap_or(mtime_ms);
        let duration_s = parsed
            .as_ref()
            .and_then(|a| a.elapsed_duration())
            .filter(|d| d.is_finite() && *d > 0.0);
        let track = parsed
            .as_ref()
            .map(|a| preview_track(&a.course))
            .unwrap_or_default();
        items.push(SavedActivity {
            filename: name,
            start_ms,
            duration_s,
            track,
        });
    }
    items.sort_by_key(|a| std::cmp::Reverse(a.start_ms));
    items
}

/// Load a previously saved activity by its filename in the uploads dir.
/// Thin wrapper over [`backend_load_gpx`] so the frontend never needs to
/// know the absolute path of the uploads directory.
#[tauri::command]
fn backend_load_saved_activity(filename: String) -> Result<String, String> {
    let path = uploads_dir().join(&filename);
    if !path.exists() {
        return Err(format!(
            "Saved activity is missing or its source moved: {filename}"
        ));
    }
    backend_load_gpx(path.to_string_lossy().into_owned())
}

/// Remove a saved activity from the uploads dir. For symlinked entries this
/// only unlinks our pointer; the file at the original path is untouched.
#[tauri::command]
fn backend_delete_activity(filename: String) -> Result<(), String> {
    let path = uploads_dir().join(&filename);
    // symlink_metadata so a broken symlink can still be deleted.
    if path.symlink_metadata().is_err() {
        return Err(format!("Activity not found: {filename}"));
    }
    std::fs::remove_file(&path).map_err(|e| format!("Failed to delete activity: {e}"))
}

#[tauri::command]
fn backend_dev_clear_cache() -> Result<(), String> {
    for dir in [uploads_dir(), templates_user_dir()] {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    std::fs::remove_dir_all(&p).ok();
                } else {
                    std::fs::remove_file(&p).ok();
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn backend_default_output_dir() -> String {
    default_output_dir().to_string_lossy().to_string()
}

/// Free space on the volume holding the render output directory. The export
/// dialog compares it against the estimated file size to warn before starting
/// a render that won't fit. `free_bytes` is null when the query fails.
#[tauri::command]
fn backend_disk_free(output_dir: Option<String>) -> String {
    let dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_output_dir);
    serde_json::json!({
        "dir": dir.to_string_lossy(),
        "free_bytes": render::scene::available_disk_bytes(&dir),
    })
    .to_string()
}

/// Single source of truth for available fonts:
/// bundled font files, user-installed font files, and system font families.
#[tauri::command]
fn backend_list_fonts() -> Vec<FontItem> {
    let mut items = font_file_items(PathBuf::from(resolve_fonts_dir()), FontSource::Bundled);
    items.extend(font_file_items(fonts_user_dir(), FontSource::Custom));
    items.extend(system_font_families().into_iter().map(|family| FontItem {
        value: family.clone(),
        label: family,
        source: FontSource::System,
    }));

    let mut seen = std::collections::BTreeSet::new();
    items.retain(|item| seen.insert(item.value.clone()));
    items.sort_by(|a, b| {
        let source_rank = |source: FontSource| match source {
            FontSource::Bundled => 0,
            FontSource::Custom => 1,
            FontSource::System => 2,
        };
        source_rank(a.source)
            .cmp(&source_rank(b.source))
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| a.value.cmp(&b.value))
    });
    items
}

/// Import a user-picked font into the user fonts dir; returns updated list.
///
/// `.ttf`/`.otf` are copied as-is. `.woff`/`.woff2` are web fonts whose glyph
/// tables are compressed inside a WOFF container — Skia's `new_from_data` only
/// parses raw SFNT, so we decompress to SFNT bytes here and store those. The
/// original name is preserved (e.g. `MyFont.woff2` -> `MyFont.woff2.ttf`) so a
/// converted web font never collides with a real `MyFont.ttf`.
#[tauri::command]
fn backend_import_font(path: String) -> Result<Vec<FontItem>, String> {
    let src = Path::new(&path);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());
    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid font filename".to_string())?;
    let user_dir = fonts_user_dir();

    match ext.as_deref() {
        Some("ttf") | Some("otf") => {
            let dest = user_dir.join(name);
            std::fs::copy(src, &dest).map_err(|e| format!("Could not import font: {e}"))?;
        }
        Some("woff") | Some("woff2") => {
            let bytes = std::fs::read(src).map_err(|e| format!("Could not read font: {e}"))?;
            let sfnt = if ext.as_deref() == Some("woff2") {
                wuff::decompress_woff2(&bytes)
            } else {
                wuff::decompress_woff1(&bytes)
            }
            .map_err(|e| format!("Could not decode web font: {e:?}"))?;
            // SFNT version tag `OTTO` means CFF/OpenType outlines -> .otf.
            let sfnt_ext = if sfnt.starts_with(b"OTTO") {
                "otf"
            } else {
                "ttf"
            };
            let dest = user_dir.join(format!("{name}.{sfnt_ext}"));
            std::fs::write(&dest, &sfnt).map_err(|e| format!("Could not import font: {e}"))?;
        }
        _ => return Err("Font must be a .ttf, .otf, .woff, or .woff2 file".into()),
    }
    Ok(backend_list_fonts())
}

#[derive(serde::Serialize)]
struct AssetItem {
    name: String,
    data_url: String,
}

fn asset_data_url(path: &Path) -> String {
    use base64::{Engine as _, engine::general_purpose};
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mime = match ext.as_str() {
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        _ => "image/png",
    };
    match std::fs::read(path) {
        Ok(bytes) => format!(
            "data:{mime};base64,{}",
            general_purpose::STANDARD.encode(&bytes)
        ),
        Err(_) => String::new(),
    }
}

#[tauri::command]
fn backend_list_assets() -> Vec<AssetItem> {
    let mut items: Vec<AssetItem> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let dirs = assets_search_dirs_vec();
    for dir in &dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str().map(str::to_string) {
                    let ext = Path::new(&name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if matches!(ext.as_str(), "png" | "webp" | "svg") && seen.insert(name.clone()) {
                        let data_url = asset_data_url(&entry.path());
                        items.push(AssetItem { name, data_url });
                    }
                }
            }
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

#[tauri::command]
fn backend_import_asset(path: String) -> Result<String, String> {
    let src = Path::new(&path);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());
    if !matches!(ext.as_deref(), Some("png") | Some("webp") | Some("svg")) {
        return Err("Asset must be a .png, .webp, or .svg file".into());
    }
    let name = src
        .file_name()
        .ok_or("Invalid path")?
        .to_string_lossy()
        .to_string();
    let dest = assets_user_dir().join(&name);
    std::fs::copy(src, &dest).map_err(|e| format!("Could not import asset: {e}"))?;
    Ok(name)
}

#[tauri::command]
fn backend_open_downloads(path: Option<String>) -> Result<String, String> {
    let dir = match path {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => default_output_dir(),
    };
    open_path(&dir.to_string_lossy())?;
    Ok(r#"{"message":"Folder opened"}"#.to_string())
}

#[tauri::command]
fn backend_open_video(filename: String) -> Result<String, String> {
    let path = PathBuf::from(&filename);
    if !path.exists() {
        return Err(format!("Video file not found: {filename}"));
    }
    open_render_result(&path)?;
    Ok(r#"{"message":"Video opened"}"#.to_string())
}

#[tauri::command]
fn backend_file_size(path: String) -> Result<String, String> {
    let bytes = std::fs::metadata(&path)
        .map_err(|e| format!("Could not read file metadata: {e}"))?
        .len();
    Ok(serde_json::json!({ "bytes": bytes }).to_string())
}

fn open_render_result(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        open_path(&path.to_string_lossy())?;
    }
    #[cfg(target_os = "windows")]
    {
        let target = path.parent().unwrap_or(path);
        std::process::Command::new("explorer")
            .arg(target)
            .spawn()
            .map_err(|e| format!("Failed to open output folder: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        let target = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|e| format!("Failed to open output folder: {e}"))?;
    }
    Ok(())
}

fn open_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open path: {e}"))?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open path: {e}"))?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open path: {e}"))?;
    Ok(())
}

#[derive(Serialize)]
struct ImageSize {
    width: i32,
    height: i32,
}

#[tauri::command]
fn backend_image_size(filename: String) -> Result<ImageSize, String> {
    let dirs_owned = assets_search_dirs_vec();
    let dirs: Vec<&str> = dirs_owned.iter().map(String::as_str).collect();
    let path = render::frame::resolve_asset_path(&filename, &dirs)
        .ok_or_else(|| format!("Asset not found: {filename}"))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "svg" {
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("Could not read SVG: {e}"))?;
        let tree = resvg::usvg::Tree::from_str(&content, &resvg::usvg::Options::default())
            .map_err(|e| format!("Could not parse SVG: {e}"))?;
        let size = tree.size().to_int_size();
        Ok(ImageSize {
            width: size.width() as i32,
            height: size.height() as i32,
        })
    } else {
        let bytes = std::fs::read(&path).map_err(|e| format!("Could not read image: {e}"))?;
        let data = skia_safe::Data::new_copy(&bytes);
        let img = skia_safe::Image::from_encoded(data)
            .ok_or_else(|| "Could not decode image".to_string())?;
        Ok(ImageSize {
            width: img.width(),
            height: img.height(),
        })
    }
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = ("open", url);
    #[cfg(target_os = "windows")]
    let cmd = ("explorer", url);
    #[cfg(target_os = "linux")]
    let cmd = ("xdg-open", url);

    let _ = std::process::Command::new(cmd.0).arg(cmd.1).spawn();
}

fn url_encode_component(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[tauri::command]
fn backend_report_issue(title: String, body: String) -> Result<(), String> {
    let url = format!(
        "https://github.com/walkersutton/cyclemetry/issues/new?title={}&body={}",
        url_encode_component(&title),
        url_encode_component(&body),
    );
    open_url(&url);
    Ok(())
}

// ─── GPX upload / load ────────────────────────────────────────────────────────

/// Load a GPX from an absolute path chosen via the native file dialog.
/// Symlinks it into the uploads dir (falling back to copy when the OS or
/// filesystem can't support symlinks) and returns metadata. The frontend
/// stores only the basename, so re-opens never re-prompt for file access.
#[tauri::command]
fn backend_load_gpx(path: String) -> Result<String, String> {
    let src = Path::new(&path);
    if !src.exists() {
        return Err(format!("File not found: {path}"));
    }
    let filename = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid filename")?;
    let dest = uploads_dir().join(filename);
    let already_registered =
        std::fs::canonicalize(src).ok() == std::fs::canonicalize(&dest).ok() && src.exists();
    if !already_registered {
        link_or_copy(src, &dest).map_err(|e| format!("Failed to register GPX: {e}"))?;
    }
    match gpx_metadata_response(filename, &dest.to_string_lossy()) {
        Ok(response) => Ok(response),
        Err(e) => {
            let _ = std::fs::remove_file(&dest);
            Err(e)
        }
    }
}

/// Replace `dest` with a symlink to `src` when possible; otherwise copy bytes.
/// We always wipe `dest` first so re-opening the same filename never leaves
/// a stale link pointing at a now-moved original.
fn link_or_copy(src: &Path, dest: &Path) -> std::io::Result<()> {
    if dest.exists() || dest.symlink_metadata().is_ok() {
        std::fs::remove_file(dest)?;
    }
    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(src, dest).is_ok() {
            return Ok(());
        }
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(src, dest).is_ok() {
            return Ok(());
        }
    }
    std::fs::copy(src, dest)?;
    Ok(())
}

/// Receive raw GPX bytes from the frontend (web drag-drop / file picker).
#[tauri::command]
fn backend_upload(file_data: Vec<u8>, filename: String) -> Result<String, String> {
    let dest = uploads_dir().join(&filename);
    std::fs::write(&dest, &file_data).map_err(|e| format!("Failed to write GPX: {e}"))?;
    match gpx_metadata_response(&filename, &dest.to_string_lossy()) {
        Ok(response) => Ok(response),
        Err(e) => {
            let _ = std::fs::remove_file(&dest);
            Err(e)
        }
    }
}

/// Parse GPX at `path` and return `{ filename, duration_seconds, has_data, start_time }`.
/// `start_time` is the wall-clock UTC of the first recorded sample (ISO 8601)
/// when the source contained timestamps, else `null` — the alignment timeline
/// needs it to map activity time onto the video's `creation_time` axis.
fn gpx_metadata_response(filename: &str, path: &str) -> Result<String, String> {
    use chrono::{TimeZone, Utc};
    let activity =
        load_activity_cached(path).map_err(|e| format!("Could not read activity file: {e}"))?;
    if !activity.has_wall_clock_time_axis() {
        return Err(
            "This activity file does not include usable timestamps. Cyclemetry needs activity timestamps to align the overlay timeline.".to_string(),
        );
    }
    let duration = activity.elapsed_duration().unwrap_or(0.0);
    if !duration.is_finite() || duration <= 0.0 {
        return Err("This activity file has no usable duration.".to_string());
    };
    let start_time = activity.start_time_ms.and_then(|ms| {
        Utc.timestamp_millis_opt(ms)
            .single()
            .map(|dt| dt.to_rfc3339())
    });
    Ok(serde_json::json!({
        "data": "file loaded",
        "filename": filename,
        "duration_seconds": duration,
        "has_data": duration > 0.0,
        "start_time": start_time,
        "valid_attributes": activity.valid_attributes,
    })
    .to_string())
}

// ─── Video probe ──────────────────────────────────────────────────────────────

/// Read container metadata for a video file via the bundled ffmpeg binary.
/// Used by the timeline alignment UI — duration + creation_time let us map
/// the video onto the GPX's real-time axis; codec drives the future proxy-
/// transcode decision; width/height feed the preview canvas sizing.
#[tauri::command]
async fn probe_video(path: String) -> Result<render::video::VideoProbe, String> {
    tokio::task::spawn_blocking(move || render::video::probe(&path))
        .await
        .map_err(|e| format!("Probe task join error: {e}"))?
}

// ─── Video streaming server ────────────────────────────────────────────────────
//
// WebKitGTK (Linux) does not reliably support the `asset://` custom protocol
// for HTML5 video — range requests are mishandled and the <video> element
// fails silently. A plain HTTP/1.1 server on localhost gives WebKitGTK a
// well-understood transport with proper range support for seeking.
//
// The server starts lazily on the first `video_src_url` call. File paths are
// base64url-encoded in the URL to avoid path-separator and percent-encoding
// ambiguity on all platforms.

const VIDEO_SERVER_HOST: &str = "127.0.0.1";
const VIDEO_CHUNK_SIZE: usize = 65536;

static VIDEO_SERVER_PORT: OnceLock<u16> = OnceLock::new();
/// Paths registered via `video_src_url`; only these are served by the HTTP server.
static VIDEO_ALLOWED_PATHS: OnceLock<Mutex<std::collections::HashSet<std::path::PathBuf>>> =
    OnceLock::new();

fn video_allowed_paths() -> &'static Mutex<std::collections::HashSet<std::path::PathBuf>> {
    VIDEO_ALLOWED_PATHS.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

async fn ensure_video_server() -> u16 {
    if let Some(&p) = VIDEO_SERVER_PORT.get() {
        return p;
    }
    let listener = tokio::net::TcpListener::bind(format!("{VIDEO_SERVER_HOST}:0"))
        .await
        .expect("bind video server");
    let port = listener.local_addr().unwrap().port();
    if VIDEO_SERVER_PORT.set(port).is_ok() {
        tokio::spawn(video_server_loop(listener));
    }
    VIDEO_SERVER_PORT.get().copied().unwrap_or(port)
}

/// Accept connections in a loop and spawn a handler task per connection.
async fn video_server_loop(listener: tokio::net::TcpListener) {
    loop {
        if let Ok((conn, _)) = listener.accept().await {
            tokio::spawn(handle_video_conn(conn));
        }
    }
}

async fn handle_video_conn(stream: tokio::net::TcpStream) {
    use base64::Engine;
    use std::io::SeekFrom;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut buf = BufReader::new(reader);

    // Parse request line: "GET /<base64path> HTTP/1.1"
    let mut line = String::new();
    if buf.read_line(&mut line).await.is_err() {
        return;
    }
    let parts: Vec<&str> = line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return;
    }
    let url_path = parts[1].trim_start_matches('/');
    let file_path = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(url_path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return,
        },
        Err(_) => return,
    };

    // Only serve paths that were explicitly registered via video_src_url.
    {
        let allowed = video_allowed_paths().lock().unwrap();
        if !allowed.contains(std::path::Path::new(&file_path)) {
            return;
        }
    }

    // Read headers — stop at the blank separator line
    let mut range_start: u64 = 0;
    let mut range_end: Option<u64> = None;
    loop {
        let mut header = String::new();
        if buf.read_line(&mut header).await.is_err() {
            break;
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_lowercase();
        if let Some(val) = lower.strip_prefix("range: bytes=") {
            if let Some((s, e)) = val.split_once('-') {
                range_start = s.parse().unwrap_or(0);
                range_end = e.parse().ok();
            }
        }
    }

    let path = std::path::Path::new(&file_path);
    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(_) => {
            let _ = writer
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            return;
        }
    };
    let file_size = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => {
            let _ = writer
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            return;
        }
    };

    let content_type = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        _ => "video/mp4",
    };

    if file_size == 0 {
        let _ = writer
            .write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: 0\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n").as_bytes())
            .await;
        return;
    }

    let start = range_start;
    let end = range_end.unwrap_or(file_size - 1).min(file_size - 1);

    if start > end || start >= file_size {
        let _ = writer
            .write_all(
                format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{file_size}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await;
        return;
    }

    let length = end - start + 1;
    let is_partial = range_start > 0 || range_end.is_some();

    if start > 0 {
        if file.seek(SeekFrom::Start(start)).await.is_err() {
            return;
        }
    }

    let status = if is_partial {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let range_header = if is_partial {
        format!("Content-Range: bytes {start}-{end}/{file_size}\r\n")
    } else {
        String::new()
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n{range_header}\r\n"
    );
    if writer.write_all(head.as_bytes()).await.is_err() {
        return;
    }

    let mut remaining = length;
    let mut chunk = vec![0u8; VIDEO_CHUNK_SIZE];
    while remaining > 0 {
        let to_read = (chunk.len() as u64).min(remaining) as usize;
        match file.read(&mut chunk[..to_read]).await {
            Ok(0) => break,
            Ok(n) => {
                if writer.write_all(&chunk[..n]).await.is_err() {
                    break;
                }
                remaining -= n as u64;
            }
            Err(_) => break,
        }
    }
}

/// Return an `http://127.0.0.1:{port}/<base64path>` URL that the webview can
/// stream via plain HTTP/1.1 with full range-request support.
#[tauri::command]
async fn video_src_url(path: String) -> Result<String, String> {
    use base64::Engine;
    video_allowed_paths()
        .lock()
        .unwrap()
        .insert(std::path::PathBuf::from(&path));
    let port = ensure_video_server().await;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(path.as_bytes());
    Ok(format!("http://{VIDEO_SERVER_HOST}:{port}/{encoded}"))
}

/// Return total activity distance and the overlay window bounds in metres.
/// Used by the frontend to render the distance-reference slider.
#[tauri::command]
async fn backend_activity_distance_info(
    gpx_filename: String,
    scene_start: f64,
    scene_end: f64,
) -> Result<String, String> {
    // If a previously selected activity can no longer be resolved, keep the
    // distance slider usable with synthetic sample data.
    let gpx_path = match resolve_gpx_path(&gpx_filename) {
        Ok((p, _)) => p,
        Err(_) => "<synthetic>".to_string(),
    };
    let synthetic = gpx_path == "<synthetic>";
    let activity = if synthetic {
        let duration = (scene_end.ceil() as usize).max(60);
        Arc::new(render::activity::Activity::synthetic(duration))
    } else {
        load_activity_cached(&gpx_path)?
    };
    let total_m = activity.total_activity_distance;
    let scene = render::template::SceneConfig {
        width: 1,
        height: 1,
        fps: 1,
        font_size: None,
        font: None,
        overlay_filename: None,
        start: Some(scene_start),
        end: Some(scene_end),
        target_duration: None,
        decimal_rounding: None,
        color: None,
        opacity: None,
        layers: None,
        groups: Vec::new(),
        vars: std::collections::HashMap::new(),
        rider_weight_kg: None,
        units: None,
        compact_units: None,
        lap_gate: None,
    };
    let activity = activity.sample_for_scene(&scene, synthetic)?;
    Ok(serde_json::json!({
        "total_m": total_m,
        "overlay_start_m": activity.distance.first().copied().unwrap_or(0.0),
        "overlay_end_m": activity.distance.last().copied().unwrap_or(0.0),
    })
    .to_string())
}

/// Returns the Unix timestamp (milliseconds) of the first GPS sample in an
/// activity file, or `null` if the file has no timestamps (e.g. a manually
/// authored GPX). Used by the frontend to compute the correct DST-aware UTC
/// offset when the user picks a named timezone for the time-of-day overlay.
#[tauri::command]
async fn backend_activity_start_time_ms(gpx_filename: String) -> Option<i64> {
    let path = resolve_gpx_path(&gpx_filename).ok()?.0;
    load_activity_cached(&path)
        .ok()
        .and_then(|a| a.start_time_ms)
}

#[tauri::command]
async fn backend_activity_metric_range(
    gpx_filename: String,
    metric: String,
    unit: Option<String>,
    scene_start: f64,
    scene_end: f64,
    // For the W/kg metric only; a local editor setting, never persisted.
    rider_weight_kg: Option<f32>,
) -> Result<String, String> {
    let gpx_path = match resolve_gpx_path(&gpx_filename) {
        Ok((p, _)) => p,
        Err(_) => "<synthetic>".to_string(),
    };
    let synthetic = gpx_path == "<synthetic>";
    let activity = if synthetic {
        let duration = (scene_end.ceil() as usize).max(60);
        Arc::new(render::activity::Activity::synthetic(duration))
    } else {
        load_activity_cached(&gpx_path)?
    };
    let scene = render::template::SceneConfig {
        width: 1,
        height: 1,
        fps: 1,
        font_size: None,
        font: None,
        overlay_filename: None,
        start: Some(scene_start),
        end: Some(scene_end),
        target_duration: None,
        decimal_rounding: None,
        color: None,
        opacity: None,
        layers: None,
        groups: Vec::new(),
        vars: std::collections::HashMap::new(),
        rider_weight_kg: None,
        units: None,
        compact_units: None,
        lap_gate: None,
    };
    let activity = activity.sample_for_scene(&scene, synthetic)?;

    let (conversion, _) = render::units::resolve(&metric, unit.as_deref());
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for idx in 0..activity.data_len() {
        let value = conversion.apply(activity.get_metric(&metric, idx, rider_weight_kg));
        if value.is_finite() {
            min = min.min(value);
            max = max.max(value);
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return Err(format!("No finite values for metric: {metric}"));
    }

    Ok(serde_json::json!({ "metric": metric, "min": min, "max": max }).to_string())
}

// ─── Community templates ──────────────────────────────────────────────────────

#[cfg(not(debug_assertions))]
const GITHUB_API_TEMPLATES: &str =
    "https://api.github.com/repos/walkersutton/cyclemetry/contents/templates";
#[cfg(not(debug_assertions))]
const GITHUB_RAW_TEMPLATES: &str =
    "https://raw.githubusercontent.com/walkersutton/cyclemetry/main/templates";
#[cfg(not(debug_assertions))]
const GITHUB_RAW_TEMPLATE_METADATA: &str =
    "https://raw.githubusercontent.com/walkersutton/cyclemetry/main/website/content/templates.json";
#[cfg(not(debug_assertions))]
const COMMUNITY_TEMPLATES_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
#[cfg(not(debug_assertions))]
static COMMUNITY_TEMPLATES_CACHE: OnceLock<Mutex<Option<(std::time::Instant, String)>>> =
    OnceLock::new();

/// In dev: scan local templates/ folder. In production: walk GitHub Contents API.
#[tauri::command]
async fn backend_community_templates() -> Result<String, String> {
    #[cfg(debug_assertions)]
    {
        community_templates_from_disk()
    }
    #[cfg(not(debug_assertions))]
    community_templates_from_github().await
}

/// Returns the repo's templates/ folder for dev-mode community browsing.
/// The exe lives under `src-tauri/target/debug/`, which also has a build-copied
/// `templates/`. Require the ancestor to contain `src-tauri/` so we match the
/// real repo root and not that stale build-output copy.
#[cfg(debug_assertions)]
fn repo_templates_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.ancestors()
                .find(|p| p.join("src-tauri").is_dir() && p.join("templates").is_dir())
                .map(|root| root.join("templates"))
        })
        .unwrap_or_else(|| PathBuf::from("templates"))
}

#[cfg(debug_assertions)]
fn repo_template_display_names() -> std::collections::HashMap<String, String> {
    let metadata_path = repo_templates_dir()
        .parent()
        .map(|root| root.join("website").join("content").join("templates.json"))
        .unwrap_or_else(|| PathBuf::from("website/content/templates.json"));
    std::fs::read_to_string(metadata_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .map(|metadata| template_display_names_from_metadata(&metadata))
        .unwrap_or_default()
}

/// Template folders use lowercase snake_case names (e.g. `crit`, `power_and_hr`).
/// Dirs starting with uppercase (e.g. `TO_BE_REFACTORED`) or `.` are skipped.
fn is_template_dir_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
}

#[cfg(debug_assertions)]
fn community_templates_from_disk() -> Result<String, String> {
    let dir = repo_templates_dir();
    let display_names = repo_template_display_names();
    let mut templates: Vec<serde_json::Value> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok("[]".to_string());
    };
    for entry in entries.flatten() {
        let Ok(ftype) = entry.file_type() else {
            continue;
        };
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy().to_string();
        if ftype.is_dir() && is_template_dir_name(&name) {
            // Each template folder contains {name}/{name}.json + preview.jpg
            let json_path = dir.join(&name).join(format!("{name}.json"));
            if json_path.exists() {
                let id = format!("{name}.json");
                let display = template_metadata_display_name(&display_names, &name);
                let preview_url = jpg_data_url(&dir.join(&name).join("preview.jpg"))
                    .unwrap_or(serde_json::Value::Null);
                templates.push(serde_json::json!({
                    "id": id,
                    "name": display,
                    "preview_url": preview_url,
                }));
            }
        }
    }
    serde_json::to_string(&templates).map_err(|e| e.to_string())
}

#[cfg(not(debug_assertions))]
async fn community_templates_from_github() -> Result<String, String> {
    use base64::Engine;

    let cache = COMMUNITY_TEMPLATES_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap();
        if let Some((fetched_at, cached)) = guard.as_ref() {
            if fetched_at.elapsed() < COMMUNITY_TEMPLATES_TTL {
                return Ok(cached.clone());
            }
        }
    }

    let client = reqwest::Client::builder()
        .user_agent("cyclemetry-app")
        .build()
        .map_err(|e| format!("Client error: {e}"))?;

    let root: serde_json::Value = client
        .get(GITHUB_API_TEMPLATES)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Parse error: {e}"))?;

    let entries = root.as_array().ok_or("Expected array from GitHub API")?;
    let display_names = match client.get(GITHUB_RAW_TEMPLATE_METADATA).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(metadata) => template_display_names_from_metadata(&metadata),
            Err(_) => std::collections::HashMap::new(),
        },
        Err(_) => std::collections::HashMap::new(),
    };

    // Collect template metadata first, then fetch all preview images concurrently.
    let mut metas: Vec<(String, String)> = Vec::new(); // (name, display)
    for entry in entries {
        let name = entry["name"].as_str().unwrap_or("");
        if entry["type"] == "dir" && is_template_dir_name(name) {
            metas.push((
                name.to_string(),
                template_metadata_display_name(&display_names, name),
            ));
        }
    }

    let mut set = tokio::task::JoinSet::new();
    for (name, display) in metas {
        let client = client.clone();
        set.spawn(async move {
            let jpg_url = format!("{GITHUB_RAW_TEMPLATES}/{name}/preview.jpg");
            let preview_url = match client.get(&jpg_url).send().await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => format!(
                        "data:image/jpeg;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(&bytes)
                    ),
                    Err(_) => jpg_url,
                },
                Err(_) => jpg_url,
            };
            serde_json::json!({
                "id": format!("{name}.json"),
                "name": display,
                "preview_url": preview_url,
            })
        });
    }

    let mut templates: Vec<serde_json::Value> = Vec::new();
    while let Some(Ok(entry)) = set.join_next().await {
        templates.push(entry);
    }
    templates.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });

    let result = serde_json::to_string(&templates).map_err(|e| e.to_string())?;
    *cache.lock().unwrap() = Some((std::time::Instant::now(), result.clone()));
    Ok(result)
}

#[tauri::command]
async fn backend_install_community_template(id: String) -> Result<String, String> {
    let rel = validate_template_path(&id)?;
    // rel is e.g. "crit.json"; the source lives in the {name}/ subfolder
    let dest = templates_user_dir().join(&rel);
    install_community_template_impl(&rel, &dest).await
}

#[cfg(debug_assertions)]
async fn install_community_template_impl(
    rel: &str,
    dest: &std::path::Path,
) -> Result<String, String> {
    let name = rel.trim_end_matches(".json");
    // Source: templates/{name}/{name}.json
    let src = repo_templates_dir().join(name).join(rel);
    let content =
        std::fs::read_to_string(src).map_err(|e| format!("Failed to read template: {e}"))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Invalid template JSON: {e}"))?;
    let pretty =
        serde_json::to_string_pretty(&parsed).map_err(|e| format!("Serialize error: {e}"))?;
    std::fs::write(dest, &pretty).map_err(|e| format!("Failed to write template: {e}"))?;
    std::fs::write(dest.with_extension("json.remote"), &pretty)
        .map_err(|e| format!("Failed to write sidecar: {e}"))?;
    Ok(serde_json::json!({ "message": format!("Installed {rel}"), "filename": rel }).to_string())
}

#[cfg(not(debug_assertions))]
async fn install_community_template_impl(
    rel: &str,
    dest: &std::path::Path,
) -> Result<String, String> {
    let name = rel.trim_end_matches(".json");
    // Source: templates/{name}/{name}.json on GitHub raw
    let url = format!("{GITHUB_RAW_TEMPLATES}/{name}/{rel}");
    let body = reqwest::Client::builder()
        .user_agent("cyclemetry-app")
        .build()
        .map_err(|e| format!("Client error: {e}"))?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Read error: {e}"))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Invalid template JSON: {e}"))?;
    let pretty =
        serde_json::to_string_pretty(&parsed).map_err(|e| format!("Serialize error: {e}"))?;
    std::fs::write(dest, &pretty).map_err(|e| format!("Failed to write template: {e}"))?;
    std::fs::write(dest.with_extension("json.remote"), &pretty)
        .map_err(|e| format!("Failed to write sidecar: {e}"))?;
    Ok(serde_json::json!({ "message": format!("Installed {rel}"), "filename": rel }).to_string())
}

/// Dev-only: overwrite the repo's community template JSON with the currently
/// saved user copy. Also refreshes the `.remote` sidecar so the template shows
/// as unmodified ("community") immediately after the write.
#[tauri::command]
fn backend_overwrite_community_template(filename: String) -> Result<(), String> {
    #[cfg(debug_assertions)]
    {
        let rel = validate_template_path(&filename)?;
        let name = rel.trim_end_matches(".json");
        let user_path = templates_user_dir().join(&rel);
        let content = std::fs::read_to_string(&user_path)
            .map_err(|e| format!("Failed to read user template: {e}"))?;
        let repo_path = repo_templates_dir().join(name).join(&rel);
        if !repo_path.exists() {
            return Err(format!("Repo template not found: {}", repo_path.display()));
        }
        std::fs::write(&repo_path, &content)
            .map_err(|e| format!("Failed to write repo template: {e}"))?;
        // Keep sidecar in sync so the template reads as "community" (not "community-modified").
        std::fs::write(user_path.with_extension("json.remote"), &content)
            .map_err(|e| format!("Failed to update sidecar: {e}"))?;
        Ok(())
    }
    #[cfg(not(debug_assertions))]
    Err("Only available in dev mode".to_string())
}

// ─── AI template generation ───────────────────────────────────────────────────

/// Hosted proxy that holds the OpenRouter API key server-side. The desktop app
/// only ever sees a prompt and the generated template — never the key.
/// Override with `CYCLEMETRY_GENERATE_URL` for local website development.
#[cfg(debug_assertions)]
const GENERATE_TEMPLATE_URL: &str = "http://localhost:3000/api/generate-template";
#[cfg(not(debug_assertions))]
const GENERATE_TEMPLATE_URL: &str = "https://cyclemetry.walkersutton.com/api/generate-template";

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("cyclemetry-app")
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .expect("http client")
    })
}

/// Create or edit a template from a natural-language prompt. Pass
/// `current_template` to edit it in place; omit it to generate a new one.
/// Returns the generated template JSON (authored at 4K) after validating that
/// it deserializes into a renderable template.
#[tauri::command]
async fn backend_generate_template(
    prompt: String,
    current_template: Option<serde_json::Value>,
) -> Result<String, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Prompt cannot be empty".to_string());
    }

    let url = std::env::var("CYCLEMETRY_GENERATE_URL")
        .unwrap_or_else(|_| GENERATE_TEMPLATE_URL.to_string());

    let mut body = serde_json::json!({ "prompt": prompt });
    if let Some(current) = current_template {
        body["currentTemplate"] = current;
    }

    let resp = http_client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let status = resp.status();
    let payload: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;

    if !status.is_success() {
        let msg = payload
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Template generation failed");
        return Err(msg.to_string());
    }

    let template = payload
        .get("template")
        .cloned()
        .ok_or("Response did not include a template")?;

    // Validate that the generated JSON deserializes into a renderable template
    // before handing it to the editor.
    render::template::Template::from_value_scaled(template.clone(), None)
        .map_err(|e| format!("Generated template was invalid: {e}"))?;

    serde_json::to_string(&template).map_err(|e| e.to_string())
}

// ─── Native Rust renderer ─────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub message: String,
}

// ─── Demo frame cache ─────────────────────────────────────────────────────────

/// Cached activity + scene for preview frames.
/// Keyed on (gpx_path, config_hash) — rebuilt only when GPX or template changes.
struct DemoCache {
    gpx_key: String,
    config_hash: u64,
    template: render::template::Template,
    activity: render::activity::Activity,
    scene_cache: render::frame::SceneCache,
}
type SharedDemoCache = Arc<Mutex<Option<DemoCache>>>;

/// Cheap identity stamp for a ride file: (byte length, mtime seconds).
/// `None` for the synthetic pseudo-path or when the file can't be statted.
fn gpx_file_stamp(path: &str) -> Option<(u64, i64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((meta.len(), mtime))
}

/// Process-wide cache of the most recently parsed ride file.
///
/// A single config change fans out into many commands that each need the
/// parsed activity — the preview rebuild, one metric-range call per meter
/// metric, the distance-slider info. Parsing a large FIT ride costs seconds of
/// CPU, so without this cache a timeline-trim change ran ~a dozen full parses
/// in parallel. Callers still window/resample per their own scene params.
struct ParsedActivity {
    path: String,
    stamp: Option<(u64, i64)>,
    activity: Arc<render::activity::Activity>,
}
static PARSED_ACTIVITY: Mutex<Option<ParsedActivity>> = Mutex::new(None);

/// Parse `path` or return the cached parse if the file is unchanged.
/// The parse deliberately happens while holding the lock: concurrent misses
/// for the same file wait for one parse instead of racing N identical ones.
fn load_activity_cached(path: &str) -> Result<Arc<render::activity::Activity>, String> {
    let stamp = gpx_file_stamp(path);
    let mut guard = PARSED_ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = guard.as_ref()
        && entry.path == path
        && entry.stamp == stamp
        // An unreadable stamp can't prove the file is unchanged — re-parse.
        && entry.stamp.is_some()
    {
        return Ok(entry.activity.clone());
    }
    let activity = Arc::new(render::activity::Activity::from_file(path)?);
    *guard = Some(ParsedActivity {
        path: path.to_string(),
        stamp,
        activity: activity.clone(),
    });
    Ok(activity)
}

fn quick_hash(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

// ─────────────────────────────────────────────────────────────────────────────

struct NativeRenderState {
    progress: render::scene::RenderProgress,
    is_running: bool,
    error: Option<String>,
}

impl NativeRenderState {
    fn new() -> Self {
        NativeRenderState {
            progress: render::scene::RenderProgress::new(),
            is_running: false,
            error: None,
        }
    }
}

type SharedRenderState = Arc<Mutex<NativeRenderState>>;

#[tauri::command]
async fn native_render(
    config: serde_json::Value,
    gpx_filename: String,
    output_dir: Option<String>,
    target_width: Option<u32>,
    target_height: Option<u32>,
    export_format: Option<String>,
    stitch_video_path: Option<String>,
    stitch_video_in: Option<f64>,
    // Transparent overlay exports: when true, emit the full canvas (dead space
    // around the overlay) instead of cropping to the visible elements. Defaults
    // to false (crop + placement-offset sidecar). Ignored for stitched.
    full_frame: Option<bool>,
    // Rider weight (kg) for the W/kg metric. A local editor setting, never part
    // of the saved template — see `SceneConfig::rider_weight_kg`.
    rider_weight_kg: Option<f32>,
    state: tauri::State<'_, SharedRenderState>,
) -> Result<String, String> {
    log::info!(
        "native_render: requested gpx={gpx_filename}, output_dir={:?}, target={:?}x{:?}, format={:?}, stitch={:?}@{:?}s",
        output_dir,
        target_width,
        target_height,
        export_format,
        stitch_video_path,
        stitch_video_in
    );
    let export_format = export_format
        .as_deref()
        .map(render::scene::ExportFormat::from_str_lossy)
        .unwrap_or_default();
    let stitch = stitch_video_path.map(|video_path| render::scene::StitchOptions {
        video_path,
        video_in: stitch_video_in.unwrap_or(0.0),
    });
    if export_format == render::scene::ExportFormat::Stitched {
        match &stitch {
            None => return Err("Stitched export requires a source video".to_string()),
            Some(s) if !std::path::Path::new(&s.video_path).exists() => {
                return Err(format!("Source video not found: {}", s.video_path));
            }
            _ => {}
        }
    }
    let target = match (target_width, target_height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };
    let mut template = render::template::Template::from_value_scaled(config.clone(), target)
        .map_err(|e| format!("Template parse error: {e}"))?;
    // Weight is injected here, after parsing — it never rides in the template JSON.
    template.scene.rider_weight_kg = rider_weight_kg;
    log::info!(
        "native_render: template parsed ({} elements, {}x{} @ {}fps)",
        template.elements.len(),
        template.scene.width,
        template.scene.height,
        template.scene.fps
    );
    let output_ext = if export_format == render::scene::ExportFormat::Stitched {
        "mp4"
    } else {
        "mov"
    };
    let output_path = resolve_output_path(&config, output_dir.as_deref(), output_ext);
    let fonts_dir = resolve_fonts_dir();
    let (gpx_path, _) = resolve_gpx_path(&gpx_filename)?;
    log::info!("native_render: resolved gpx={gpx_path}, output={output_path}");

    let progress_clone = {
        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
        *s = NativeRenderState::new();
        s.is_running = true;
        render::scene::RenderProgress {
            frames_rendered: s.progress.frames_rendered.clone(),
            total_frames: s.progress.total_frames.clone(),
            cancelled: s.progress.cancelled.clone(),
            paused_low_disk: s.progress.paused_low_disk.clone(),
        }
    };
    let state_clone = state.inner().clone();
    let output_path_for_response = output_path.clone();
    let assets_dirs_owned = assets_search_dirs_vec();

    tokio::task::spawn_blocking(move || {
        log::info!("native_render: worker started");
        // Held for the whole render: without it, minimizing the window lets macOS
        // App Nap demote these workers to the efficiency cores mid-export. Dropped
        // when the worker returns, panic or not.
        let _activity = activity::RenderActivity::begin("Rendering video");
        // catch_unwind ensures is_running is always cleared even if render_video panics
        // (e.g. Skia surface failure, rayon worker panic). Without this, a panic would
        // silently swallow the JoinHandle error and leave is_running=true forever.
        let assets_dirs: Vec<&str> = assets_dirs_owned.iter().map(String::as_str).collect();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render::scene::render_video(
                &gpx_path,
                &template,
                &output_path,
                &fonts_dir,
                &assets_dirs,
                export_format,
                stitch.as_ref(),
                full_frame.unwrap_or(false),
                &progress_clone,
            )
        }))
        .unwrap_or_else(|e| {
            let msg = e
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| e.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            log::error!("Render panicked: {msg}");
            Err(format!("Render crashed: {msg}"))
        });

        let mut s = state_clone.lock().unwrap_or_else(|e| e.into_inner());
        s.is_running = false;
        match result {
            Ok(()) => {
                s.error = None;
                log::info!("Native render complete: {output_path}");
            }
            Err(e) => {
                log::error!("Native render error: {e}");
                s.error = Some(e);
            }
        }
    });

    Ok(
        serde_json::json!({ "status": "started", "output_path": output_path_for_response })
            .to_string(),
    )
}

#[tauri::command]
async fn native_progress(state: tauri::State<'_, SharedRenderState>) -> Result<String, String> {
    let s = state.lock().unwrap_or_else(|e| e.into_inner());
    let (rendered, total) = s.progress.snapshot();
    let fraction = if total > 0 {
        rendered as f64 / total as f64
    } else {
        0.0
    };
    Ok(serde_json::json!({
        "frames_rendered": rendered,
        "total_frames": total,
        "fraction": fraction,
        "is_running": s.is_running,
        "paused_low_disk": s.progress.paused_low_disk.load(std::sync::atomic::Ordering::Relaxed),
        "error": s.error,
    })
    .to_string())
}

#[tauri::command]
async fn native_cancel(state: tauri::State<'_, SharedRenderState>) -> Result<String, String> {
    let s = state.lock().unwrap_or_else(|e| e.into_inner());
    s.progress.cancel();
    log::info!("Render cancel requested");
    Ok(serde_json::json!({ "status": "cancel_requested" }).to_string())
}

/// Preview a single frame.
///
/// Hot-path optimisation: the prepared Activity + SceneCache are cached in
/// `SharedDemoCache` so subsequent frames cost only a single `render_frame`
/// call (~5–20 ms) instead of a full rebuild (~500 ms–2 s).
#[tauri::command]
async fn native_demo(
    config: serde_json::Value,
    gpx_filename: String,
    frame_index: u32,
    preview_fps: u32,
    target_width: Option<u32>,
    target_height: Option<u32>,
    // Zoomed-preview supersampling. When the output dims are present, only the
    // scene window (view_x,view_y,view_w,view_h) is rendered, magnified into a
    // view_out_w × view_out_h surface so text stays crisp at any zoom. The cache
    // is still keyed by target_width/height, so the zoomed crop reuses the same
    // base SceneCache the un-zoomed frame built (no rebuild on zoom/pan).
    view_x: Option<f32>,
    view_y: Option<f32>,
    view_w: Option<f32>,
    view_h: Option<f32>,
    view_out_w: Option<u32>,
    view_out_h: Option<u32>,
    // Rider weight (kg) for the W/kg metric. A local editor setting, never part
    // of the saved template — see `SceneConfig::rider_weight_kg`.
    rider_weight_kg: Option<f32>,
    demo_cache: tauri::State<'_, SharedDemoCache>,
) -> Result<String, String> {
    let target = match (target_width, target_height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };
    // Preview canvas = chosen output resolution (so aspect ratio is honored).
    let wh = target.unwrap_or_else(|| template_value_wh(&config));
    let view = match (view_out_w, view_out_h) {
        (Some(ow), Some(oh)) => Some(render::frame::ViewTransform {
            vx: view_x.unwrap_or(0.0),
            vy: view_y.unwrap_or(0.0),
            vw: view_w.unwrap_or(wh.0 as f32),
            vh: view_h.unwrap_or(wh.1 as f32),
            out_w: ow,
            out_h: oh,
        }),
        _ => None,
    };
    let preview_fps = preview_fps.max(1);
    if gpx_filename.is_empty() || gpx_filename == "null" || gpx_filename == "undefined" {
        return Err("No activity selected".to_string());
    }
    // Include preview_fps + target + rider weight in cache hash so changing any
    // of them rebuilds (weight isn't in `config`, so it must be hashed here).
    let config_hash = quick_hash(&format!(
        "{}:{}:{:?}:{:?}",
        config, preview_fps, target, rider_weight_kg
    ));
    // If a previously selected activity can no longer be resolved, preview
    // synthetic sample data rather than failing the whole preview.
    let (gpx_path, gpx_warning) = match resolve_gpx_path(&gpx_filename) {
        Ok(v) => v,
        Err(_) => {
            let real =
                !gpx_filename.is_empty() && gpx_filename != "null" && gpx_filename != "undefined";
            let warning = real
                .then(|| format!("GPX '{gpx_filename}' not found — showing sample data instead"));
            ("<synthetic>".to_string(), warning)
        }
    };
    let fonts_dir = resolve_fonts_dir();
    let cache_arc = demo_cache.inner().clone();

    let (rgba, elements, out_wh) = tokio::task::spawn_blocking(move || {
        let mut guard = cache_arc.lock().unwrap_or_else(|e| e.into_inner());

        let needs_rebuild = match &*guard {
            None => true,
            Some(c) => c.gpx_key != gpx_path || c.config_hash != config_hash,
        };

        if needs_rebuild {
            let mut template = render::template::Template::from_value_scaled(config, target)
                .map_err(|e| format!("Template parse error: {e}"))?;
            // Weight is injected here, after parsing — it never rides in the template JSON.
            template.scene.rider_weight_kg = rider_weight_kg;

            let synthetic = gpx_path == "<synthetic>";
            // Config-only rebuilds (timeline trim, element edits) reuse the
            // parsed ride via the shared cache; only a changed file re-parses.
            let source = if synthetic {
                Arc::new(render::activity::Activity::synthetic(60))
            } else {
                load_activity_cached(&gpx_path).map_err(|e| format!("GPX parse error: {e}"))?
            };
            let mut preview_scene = template.scene.clone();
            preview_scene.fps = preview_fps;
            // The preview scrubs ride-time across the whole window; the editor
            // indexes frames by ride-second × preview_fps. A time-lapse
            // (target_duration) would instead compress the window into a handful
            // of output frames, breaking that mapping (every later scrub position
            // would clamp to the last frame). So the preview always samples the
            // full window at real time; the frontend plays it back fast to
            // reflect the time-lapse output length.
            preview_scene.target_duration = None;
            let mut preview_template = template.clone();
            preview_template.scene = preview_scene;
            let activity = source
                .sample_for_scene(&preview_template.scene, synthetic)
                .map_err(|e| format!("Activity timeline error: {e}"))?;

            let assets_dirs_owned = assets_search_dirs_vec();
            let assets_dirs: Vec<&str> = assets_dirs_owned.iter().map(String::as_str).collect();
            let scene_cache = render::frame::SceneCache::build(
                &activity,
                &preview_template,
                &fonts_dir,
                &assets_dirs,
            )
            .map_err(|e| format!("SceneCache build failed: {e}"))?;

            *guard = Some(DemoCache {
                gpx_key: gpx_path,
                config_hash,
                template: preview_template,
                activity,
                scene_cache,
            });
        }

        let cached = guard.as_ref().unwrap();
        // frame_index is relative to trimmed+interpolated activity start (0-based).
        let frame_idx = (frame_index as usize).min(cached.activity.data_len().saturating_sub(1));

        if let Some(v) = &view {
            // Zoomed crop: supersample just the visible window. No element
            // measurement — the un-zoomed base frame already owns the overlay's
            // coordinate space; this layer is purely a crispness enhancement.
            let rgba = render::frame::render_frame(
                frame_idx,
                &cached.scene_cache,
                &cached.activity,
                &cached.template,
                None,
                Some(v),
            );
            return Ok::<(Vec<u8>, Vec<render::frame::ElementBounds>, (u32, u32)), String>((
                rgba,
                Vec::new(),
                (v.out_w, v.out_h),
            ));
        }

        // Preview renders the full frame (placement context); no crop.
        let rgba = render::frame::render_frame(
            frame_idx,
            &cached.scene_cache,
            &cached.activity,
            &cached.template,
            None,
            None,
        );
        let elements = render::frame::measure_elements(
            frame_idx,
            &cached.activity,
            &cached.template,
            &fonts_dir,
        );
        Ok::<(Vec<u8>, Vec<render::frame::ElementBounds>, (u32, u32)), String>((rgba, elements, wh))
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))??;

    let png = rgba_to_png(&rgba, out_wh);
    use base64::{Engine as _, engine::general_purpose};
    let b64 = general_purpose::STANDARD.encode(&png);
    Ok(serde_json::json!({
        "image": format!("data:image/png;base64,{b64}"),
        "elements": elements,
        // The dims the frame was actually rendered + measured at. The frontend
        // keys the canvas buffer and the WYSIWYG overlay's coordinate space off
        // these so they track the displayed frame, not an in-flight render target.
        "width": out_wh.0,
        "height": out_wh.1,
        "warning": gpx_warning,
    })
    .to_string())
}

// Pixel buffers from render_frame are BGRA8888 (Skia's native raster format,
// fed to FFmpeg as -pix_fmt bgra with zero conversion). The ColorType here must
// match so the preview PNG doesn't swap red/blue.
fn rgba_to_png(rgba: &[u8], (w, h): (u32, u32)) -> Vec<u8> {
    let info = skia_safe::ImageInfo::new(
        skia_safe::ISize::new(w as i32, h as i32),
        skia_safe::ColorType::BGRA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let data = skia_safe::Data::new_copy(rgba);
    if let Some(img) = skia_safe::images::raster_from_data(&info, data, (w * 4) as usize)
        && let Some(encoded) = img.encode(None, skia_safe::EncodedImageFormat::PNG, None)
    {
        return encoded.as_bytes().to_vec();
    }
    vec![]
}

fn template_value_wh(config: &serde_json::Value) -> (u32, u32) {
    let w = config
        .pointer("/scene/width")
        .and_then(|x| x.as_u64())
        .unwrap_or(1920) as u32;
    let h = config
        .pointer("/scene/height")
        .and_then(|x| x.as_u64())
        .unwrap_or(1080) as u32;
    (w, h)
}

/// Renders `frames` frames (no ffmpeg, no output file) and returns elapsed
/// milliseconds. Used by the frontend to estimate full render duration without
/// needing to run or store a real render first.
#[tauri::command]
async fn native_benchmark(
    config: serde_json::Value,
    gpx_filename: String,
    frames: u32,
    target_width: Option<u32>,
    target_height: Option<u32>,
) -> Result<String, String> {
    let target = match (target_width, target_height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };
    let template = render::template::Template::from_value_scaled(config, target)
        .map_err(|e| format!("Template parse error: {e}"))?;
    let fonts_dir = resolve_fonts_dir();
    let (gpx_path, _) = resolve_gpx_path(&gpx_filename)?;
    let frames = frames.clamp(1, 300) as usize;

    tokio::task::spawn_blocking(move || {
        let synthetic = gpx_path == "<synthetic>";
        let activity = if synthetic {
            Arc::new(render::activity::Activity::synthetic(60))
        } else {
            load_activity_cached(&gpx_path).map_err(|e| format!("GPX parse: {e}"))?
        };
        let activity = activity
            .sample_for_scene(&template.scene, synthetic)
            .map_err(|e| format!("Activity timeline: {e}"))?;
        if activity.data_len() == 0 {
            return Err("No activity data".to_string());
        }
        let assets_dirs_owned = assets_search_dirs_vec();
        let assets_dirs: Vec<&str> = assets_dirs_owned.iter().map(String::as_str).collect();
        let cache =
            render::frame::SceneCache::build(&activity, &template, &fonts_dir, &assets_dirs)
                .map_err(|e| format!("SceneCache: {e}"))?;

        let total = activity.data_len();
        let n = frames.min(total);
        // Spread sample indices evenly across the full timeline so varied
        // segments (elevation changes, map tiles, chart curves) are represented.
        let indices: Vec<usize> = (0..n)
            .map(|i| {
                if n > 1 {
                    (i * (total - 1)) / (n - 1)
                } else {
                    0
                }
            })
            .collect();
        // Three warm-up frames to prime font/surface/path caches before timing.
        for &idx in indices.iter().take(3) {
            let _ = render::frame::render_frame(idx, &cache, &activity, &template, None, None);
        }
        let t0 = std::time::Instant::now();
        for &idx in &indices {
            let _ = render::frame::render_frame(idx, &cache, &activity, &template, None, None);
        }
        let elapsed_ms = t0.elapsed().as_millis() as u64;

        Ok(serde_json::json!({ "frames": n, "elapsed_ms": elapsed_ms }).to_string())
    })
    .await
    .map_err(|e| format!("Join error: {e}"))?
}

/// Runs a short real export — full pipeline including the FFmpeg encode — to a
/// throwaway temp file and reports measured wall time and encoded size. The
/// frontend uses this to calibrate render-time and file-size estimates for a
/// codec on this machine, instead of deriving them from another codec's
/// numbers via hardcoded ratios.
#[tauri::command]
async fn native_calibrate_export(
    config: serde_json::Value,
    gpx_filename: String,
    export_format: String,
    seconds: Option<f64>,
    target_width: Option<u32>,
    target_height: Option<u32>,
) -> Result<String, String> {
    let format = render::scene::ExportFormat::from_str_lossy(&export_format);
    if format == render::scene::ExportFormat::Stitched {
        // Estimating a stitched export would need the source footage too;
        // its numbers come from real exports instead.
        return Err("Calibration is not supported for stitched export".to_string());
    }
    let target = match (target_width, target_height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };
    let mut template = render::template::Template::from_value_scaled(config, target)
        .map_err(|e| format!("Template parse error: {e}"))?;
    // Clip the scene window to a short slice at the overlay start: same
    // template, resolution, and crop as the real export — just brief.
    let seconds = seconds.unwrap_or(3.0).clamp(1.0, 10.0);
    let start = template.scene.start.unwrap_or(0.0);
    let end = template
        .scene
        .end
        .map_or(start + seconds, |e| e.min(start + seconds));
    template.scene.start = Some(start);
    template.scene.end = Some(end.max(start + 1.0));

    let fonts_dir = resolve_fonts_dir();
    let (gpx_path, _) = resolve_gpx_path(&gpx_filename)?;
    let assets_dirs_owned = assets_search_dirs_vec();

    tokio::task::spawn_blocking(move || {
        let output_path = std::env::temp_dir()
            .join(format!("cyclemetry-calibrate-{export_format}.mov"))
            .to_string_lossy()
            .into_owned();
        let progress = render::scene::RenderProgress::new();
        let assets_dirs: Vec<&str> = assets_dirs_owned.iter().map(String::as_str).collect();
        let t0 = std::time::Instant::now();
        let result = render::scene::render_video(
            &gpx_path,
            &template,
            &output_path,
            &fonts_dir,
            &assets_dirs,
            format,
            None,
            // Calibration measures bits-per-pixel-second, which is independent
            // of crop vs full-frame — use the default (cropped) path.
            false,
            &progress,
        );
        let elapsed_ms = t0.elapsed().as_millis() as u64;
        let bytes = std::fs::metadata(&output_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let _ = std::fs::remove_file(&output_path);
        let _ = std::fs::remove_file(format!("{output_path}.placement.txt"));
        result?;
        let (frames, _) = progress.snapshot();
        if frames == 0 || bytes == 0 {
            return Err("Test render produced no output".to_string());
        }
        Ok(serde_json::json!({
            "frames": frames,
            "elapsed_ms": elapsed_ms,
            "bytes": bytes,
        })
        .to_string())
    })
    .await
    .map_err(|e| format!("Join error: {e}"))?
}

// ─── Recent GPX state ─────────────────────────────────────────────────────────

type SharedRecentGpx = Arc<Mutex<Vec<String>>>;

#[tauri::command]
fn record_gpx_opened(app: tauri::AppHandle, path: String) {
    let state = app.state::<SharedRecentGpx>();
    let mut files = state.lock().unwrap();
    *files = recent::push(path, files.clone());
    recent::save(&files);
}

// ─── App entry point ──────────────────────────────────────────────────────────

pub fn run() {
    // The render core can't know per-platform app-data paths; hand it the
    // user fonts dir so load_typeface can find custom fonts.
    render::frame::set_user_fonts_dir(fonts_user_dir());

    // On Windows (release), panics produce no visible output — write a crash file
    // to %TEMP% so the user can report it. Also write a startup marker so we know
    // the process launched at all if there are no logs.
    #[cfg(windows)]
    {
        let tmp = std::env::temp_dir();
        std::fs::write(
            tmp.join("cyclemetry_started.txt"),
            env!("CARGO_PKG_VERSION"),
        )
        .ok();
        std::panic::set_hook(Box::new(|info| {
            let msg = format!("{info}");
            std::fs::write(std::env::temp_dir().join("cyclemetry_panic.txt"), &msg).ok();
        }));
    }

    // Read recent GPX list before building the app so the menu can use it at startup.
    #[cfg(target_os = "macos")]
    let initial_recent_gpx = recent::read();
    #[cfg(not(target_os = "macos"))]
    let initial_recent_gpx: Vec<String> = Vec::new();

    let recent_gpx_state: SharedRecentGpx = Arc::new(Mutex::new(initial_recent_gpx.clone()));

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .manage(Arc::new(Mutex::new(NativeRenderState::new())) as SharedRenderState)
        .manage(Arc::new(Mutex::new(None)) as SharedDemoCache)
        .manage(recent_gpx_state)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init());

    #[cfg(not(debug_assertions))]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .invoke_handler(tauri::generate_handler![
            app_build_info,
            native_render,
            native_progress,
            native_cancel,
            native_demo,
            native_benchmark,
            native_calibrate_export,
            backend_list_templates,
            backend_get_template,
            backend_save_template,
            backend_import_template,
            backend_open_templates,
            backend_default_output_dir,
            backend_disk_free,
            backend_list_fonts,
            backend_import_font,
            backend_list_assets,
            backend_import_asset,
            backend_image_size,
            backend_open_activities,
            backend_open_downloads,
            backend_open_video,
            backend_file_size,
            backend_load_gpx,
            backend_list_activities,
            backend_load_saved_activity,
            backend_delete_activity,
            backend_dev_clear_cache,
            backend_upload,
            probe_video,
            video_src_url,
            backend_community_templates,
            backend_install_community_template,
            backend_overwrite_community_template,
            backend_generate_template,
            backend_delete_template,
            backend_save_template_preview,
            backend_report_issue,
            backend_activity_distance_info,
            backend_activity_start_time_ms,
            backend_activity_metric_range,
            record_gpx_opened,
            strava::strava_status,
            strava::strava_connect,
            strava::strava_disconnect,
            strava::strava_activities,
            strava::strava_import_activity,
        ])
        .setup(move |app| {
            // Init logging FIRST — if anything below panics/fails, we at least have a log.
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::new()
                        .level(log::LevelFilter::Debug)
                        .targets([
                            tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                            tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                        ])
                        .build(),
                )?;
            } else {
                app.handle().plugin(
                    tauri_plugin_log::Builder::new()
                        .level(log::LevelFilter::Info)
                        .level_for("tauri_plugin_updater", log::LevelFilter::Warn)
                        .targets([tauri_plugin_log::Target::new(
                            tauri_plugin_log::TargetKind::LogDir {
                                file_name: Some("cyclemetry".into()),
                            },
                        )])
                        .build(),
                )?;
            }

            if let Ok(dir) = app.path().app_data_dir() {
                std::fs::create_dir_all(&dir).ok();
                APP_DATA_DIR.set(dir).ok();
            }
            log::info!("startup: app_data_dir = {:?}", app_data_base());

            // ── Window size: restore saved, then persist on close ───────────
            if let Some(win) = app.get_webview_window("main") {
                if let Some((w, h)) = load_window_size() {
                    win.set_size(tauri::Size::Logical(tauri::LogicalSize {
                        width: w as f64,
                        height: h as f64,
                    }))
                    .ok();
                }
                let win2 = win.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { .. } = event
                        && let (Ok(s), Ok(sf)) = (win2.inner_size(), win2.scale_factor())
                    {
                        let w = (s.width as f64 / sf).round() as u32;
                        let h = (s.height as f64 / sf).round() as u32;
                        save_window_size(w, h);
                    }
                });
            }

            #[cfg(all(debug_assertions, target_os = "macos"))]
            {
                use objc2::{AnyThread, MainThreadMarker};
                use objc2_app_kit::{NSApplication, NSImage};
                use objc2_foundation::NSData;
                let mtm = MainThreadMarker::new().expect("setup must run on main thread");
                let data = NSData::with_bytes(include_bytes!("../icons/icon.icns"));
                if let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) {
                    let ns_app = NSApplication::sharedApplication(mtm);
                    unsafe { ns_app.setApplicationIconImage(Some(&icon)) };
                }
            }

            // ── macOS menu bar ──────────────────────────────────────────────
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

                // ── Cyclemetry (app) menu ─────────────────────────────────
                let about =
                    MenuItem::with_id(app, "about", "About Cyclemetry", true, None::<&str>)?;
                let settings =
                    MenuItem::with_id(app, "settings", "Settings...", true, Some("CmdOrCtrl+,"))?;
                let check_updates = MenuItem::with_id(
                    app,
                    "check_updates",
                    "Check for Updates...",
                    true,
                    None::<&str>,
                )?;
                let app_submenu = Submenu::with_items(
                    app,
                    "Cyclemetry",
                    true,
                    &[
                        &about,
                        &PredefinedMenuItem::separator(app)?,
                        &settings,
                        &PredefinedMenuItem::separator(app)?,
                        &check_updates,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::services(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::hide(app, None)?,
                        &PredefinedMenuItem::hide_others(app, None)?,
                        &PredefinedMenuItem::show_all(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::quit(app, None)?,
                    ],
                )?;

                // ── File menu ─────────────────────────────────────────────
                // "Open Recent" submenu — items built from list stored on disk
                let no_recent_item =
                    MenuItem::with_id(app, "no_recent", "No Recent Files", false, None::<&str>)?;
                let clear_recent_item =
                    MenuItem::with_id(app, "clear_recent", "Clear Recent", true, None::<&str>)?;
                let recent_sep = PredefinedMenuItem::separator(app)?;
                let recent_gpx_items: Vec<MenuItem<_>> = initial_recent_gpx
                    .iter()
                    .enumerate()
                    .map(|(i, path)| {
                        let name = std::path::Path::new(path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(path.as_str())
                            .to_owned();
                        MenuItem::with_id(app, format!("recent_gpx_{i}"), name, true, None::<&str>)
                    })
                    .collect::<tauri::Result<_>>()?;

                let open_recent = if initial_recent_gpx.is_empty() {
                    Submenu::with_items(app, "Open Recent", false, &[&no_recent_item])?
                } else {
                    let mut refs: Vec<&dyn IsMenuItem<_>> = recent_gpx_items
                        .iter()
                        .map(|i| i as &dyn IsMenuItem<_>)
                        .collect();
                    refs.push(&recent_sep);
                    refs.push(&clear_recent_item);
                    Submenu::with_items(app, "Open Recent", true, &refs)?
                };

                let add_video =
                    MenuItem::with_id(app, "add_video", "Add Video…", true, None::<&str>)?;
                let file_submenu = Submenu::with_items(app, "File", true, &[&add_video])?;

                // ── Overlays menu ─────────────────────────────────────────
                let open_exports = MenuItem::with_id(
                    app,
                    "show_downloads",
                    "Open Exports Folder",
                    true,
                    Some("CmdOrCtrl+Shift+E"),
                )?;
                let overlays_save_path = MenuItem::with_id(
                    app,
                    "overlays_save_path",
                    "Choose Default Save Path…",
                    true,
                    None::<&str>,
                )?;
                let exports_submenu = Submenu::with_items(
                    app,
                    "Exports",
                    true,
                    &[
                        &open_exports,
                        &PredefinedMenuItem::separator(app)?,
                        &overlays_save_path,
                    ],
                )?;

                // ── Activities menu ───────────────────────────────────────
                let act_open = MenuItem::with_id(
                    app,
                    "activities_open_gpx",
                    "Select Activity…",
                    true,
                    None::<&str>,
                )?;
                let act_sep = PredefinedMenuItem::separator(app)?;
                let act_show_folder = MenuItem::with_id(
                    app,
                    "activities_show_folder",
                    "Show Activities Folder",
                    true,
                    None::<&str>,
                )?;
                let activities_submenu = Submenu::with_items(
                    app,
                    "Activities",
                    true,
                    &[
                        &act_open,
                        &act_sep,
                        &open_recent,
                        &PredefinedMenuItem::separator(app)?,
                        &act_show_folder,
                    ],
                )?;

                // ── Help menu ─────────────────────────────────────────────
                let help_docs =
                    MenuItem::with_id(app, "help_docs", "Documentation", true, None::<&str>)?;
                let help_issues =
                    MenuItem::with_id(app, "help_issues", "Report an Issue", true, None::<&str>)?;
                let edit_undo = MenuItem::with_id(app, "edit_undo", "Undo", true, None::<&str>)?;
                let edit_redo = MenuItem::with_id(app, "edit_redo", "Redo", true, None::<&str>)?;
                let edit_copy =
                    MenuItem::with_id(app, "edit_copy", "Copy Element", true, None::<&str>)?;
                let edit_paste =
                    MenuItem::with_id(app, "edit_paste", "Paste Element", true, None::<&str>)?;
                let edit_submenu = Submenu::with_items(
                    app,
                    "Edit",
                    true,
                    &[
                        &edit_undo,
                        &edit_redo,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::cut(app, None)?,
                        &edit_copy,
                        &edit_paste,
                        &PredefinedMenuItem::select_all(app, None)?,
                    ],
                )?;

                let help_submenu =
                    Submenu::with_items(app, "Help", true, &[&help_docs, &help_issues])?;

                // ── Templates menu ────────────────────────────────────────
                let new_tpl =
                    MenuItem::with_id(app, "new_template", "Create Template", true, None::<&str>)?;
                let save_tpl = MenuItem::with_id(
                    app,
                    "save_template",
                    "Save Template",
                    true,
                    Some("CmdOrCtrl+S"),
                )?;
                let select_template = MenuItem::with_id(
                    app,
                    "select_template",
                    "Select Template…",
                    true,
                    None::<&str>,
                )?;
                let tpl_sep1 = PredefinedMenuItem::separator(app)?;
                let show_tpl_dir = MenuItem::with_id(
                    app,
                    "show_templates",
                    "Show Templates Folder",
                    true,
                    None::<&str>,
                )?;
                let tpl_sep2 = PredefinedMenuItem::separator(app)?;
                let add_custom_font = MenuItem::with_id(
                    app,
                    "add_custom_font",
                    "Add Custom Font…",
                    true,
                    None::<&str>,
                )?;
                let templates_submenu = Submenu::with_items(
                    app,
                    "Templates",
                    true,
                    &[
                        &select_template,
                        &tpl_sep1,
                        &new_tpl,
                        &save_tpl,
                        &tpl_sep2,
                        &show_tpl_dir,
                        &PredefinedMenuItem::separator(app)?,
                        &add_custom_font,
                    ],
                )?;

                #[cfg(debug_assertions)]
                let dev_reset =
                    MenuItem::with_id(app, "dev_reset", "Reset App State", true, None::<&str>)?;
                #[cfg(debug_assertions)]
                let dev_build_label = MenuItem::with_id(
                    app,
                    "dev_build_info",
                    app_build_info(),
                    false,
                    None::<&str>,
                )?;
                #[cfg(debug_assertions)]
                let dev_submenu = Submenu::with_items(
                    app,
                    "Developer",
                    true,
                    &[
                        &dev_reset,
                        &PredefinedMenuItem::separator(app)?,
                        &dev_build_label,
                    ],
                )?;

                #[cfg(debug_assertions)]
                let menu = Menu::with_items(
                    app,
                    &[
                        &app_submenu,
                        &file_submenu,
                        &edit_submenu,
                        &templates_submenu,
                        &activities_submenu,
                        &exports_submenu,
                        &help_submenu,
                        &dev_submenu,
                    ],
                )?;
                #[cfg(not(debug_assertions))]
                let menu = Menu::with_items(
                    app,
                    &[
                        &app_submenu,
                        &file_submenu,
                        &edit_submenu,
                        &templates_submenu,
                        &activities_submenu,
                        &exports_submenu,
                        &help_submenu,
                    ],
                )?;
                app.set_menu(menu)?;

                // macOS shows a Spotlight-style search field at the top of the
                // Help menu, but only once AppKit knows which submenu *is* the
                // Help menu. Because Tauri installs the menu programmatically
                // after launch, AppKit's title-based auto-detection has already
                // run and found nothing — so point `helpMenu` at our "Help"
                // submenu explicitly to opt back into the search field.
                {
                    use objc2::MainThreadMarker;
                    use objc2_app_kit::NSApplication;
                    let mtm = MainThreadMarker::new().expect("setup must run on main thread");
                    let ns_app = NSApplication::sharedApplication(mtm);
                    if let Some(main_menu) = ns_app.mainMenu() {
                        for i in 0..main_menu.numberOfItems() {
                            let Some(item) = main_menu.itemAtIndex(i) else {
                                continue;
                            };
                            let Some(submenu) = item.submenu() else {
                                continue;
                            };
                            if submenu.title().to_string() == "Help" {
                                ns_app.setHelpMenu(Some(&submenu));
                                break;
                            }
                        }
                    }
                }

                app.on_menu_event(|app_handle, event| {
                    use tauri::Emitter;
                    let id = event.id().as_ref();
                    match id {
                        "edit_undo" => {
                            app_handle.emit("menu_undo", ()).ok();
                        }
                        "edit_redo" => {
                            app_handle.emit("menu_redo", ()).ok();
                        }
                        "edit_copy" => {
                            app_handle.emit("menu_copy", ()).ok();
                        }
                        "edit_paste" => {
                            app_handle.emit("menu_paste", ()).ok();
                        }
                        "about" => {
                            app_handle.emit("menu_about", ()).ok();
                        }
                        "settings" => {
                            app_handle.emit("menu_settings", ()).ok();
                        }
                        "check_updates" => {
                            app_handle.emit("check_for_updates", ()).ok();
                        }
                        "activities_open_gpx" => {
                            app_handle.emit("menu_open_gpx", ()).ok();
                        }
                        "add_video" => {
                            app_handle.emit("menu_add_video", ()).ok();
                        }
                        "activities_show_folder" => {
                            app_handle.emit("menu_show_activities", ()).ok();
                        }
                        "save_template" => {
                            app_handle.emit("menu_save_template", ()).ok();
                        }
                        "new_template" => {
                            app_handle.emit("menu_new_template", ()).ok();
                        }
                        "select_template" => {
                            app_handle.emit("menu_show_template_dialog", ()).ok();
                        }
                        "add_custom_font" => {
                            app_handle.emit("menu_add_custom_font", ()).ok();
                        }
                        "show_downloads" => {
                            app_handle.emit("menu_show_downloads", ()).ok();
                        }
                        "overlays_save_path" => {
                            app_handle.emit("menu_settings", ()).ok();
                        }
                        "show_templates" => {
                            app_handle.emit("menu_open_templates_folder", ()).ok();
                        }
                        "dev_reset" => {
                            app_handle.emit("menu_dev_reset", ()).ok();
                        }
                        "clear_recent" => {
                            app_handle
                                .state::<SharedRecentGpx>()
                                .lock()
                                .unwrap()
                                .clear();
                            recent::clear();
                        }
                        "help_docs" => {
                            open_url("https://cyclemetry.walkersutton.com/?ref=app-help");
                        }
                        "help_issues" => {
                            open_url("https://github.com/walkersutton/cyclemetry/issues/new");
                        }
                        _ if id.starts_with("recent_gpx_") => {
                            if let Ok(idx) = id["recent_gpx_".len()..].parse::<usize>() {
                                let state = app_handle.state::<SharedRecentGpx>();
                                let files = state.lock().unwrap();
                                if let Some(path) = files.get(idx) {
                                    app_handle.emit("menu_open_recent_gpx", path.clone()).ok();
                                }
                            }
                        }
                        _ => {}
                    }
                });
            }

            #[cfg(not(target_os = "macos"))]
            {
                use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

                let settings =
                    MenuItem::with_id(app, "settings", "Settings...", true, Some("CmdOrCtrl+,"))?;
                let add_video =
                    MenuItem::with_id(app, "add_video", "Add Video...", true, None::<&str>)?;
                let file_submenu = Submenu::with_items(
                    app,
                    "File",
                    true,
                    &[
                        &settings,
                        &PredefinedMenuItem::separator(app)?,
                        &add_video,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::quit(app, None)?,
                    ],
                )?;

                let open_exports = MenuItem::with_id(
                    app,
                    "show_downloads",
                    "Open Exports Folder",
                    true,
                    Some("CmdOrCtrl+Shift+E"),
                )?;
                let overlays_save_path = MenuItem::with_id(
                    app,
                    "overlays_save_path",
                    "Choose Default Save Path…",
                    true,
                    None::<&str>,
                )?;
                let exports_submenu = Submenu::with_items(
                    app,
                    "Exports",
                    true,
                    &[
                        &open_exports,
                        &PredefinedMenuItem::separator(app)?,
                        &overlays_save_path,
                    ],
                )?;

                let edit_undo = MenuItem::with_id(app, "edit_undo", "Undo", true, None::<&str>)?;
                let edit_redo = MenuItem::with_id(app, "edit_redo", "Redo", true, None::<&str>)?;
                let edit_copy =
                    MenuItem::with_id(app, "edit_copy", "Copy Element", true, None::<&str>)?;
                let edit_paste =
                    MenuItem::with_id(app, "edit_paste", "Paste Element", true, None::<&str>)?;
                let edit_submenu = Submenu::with_items(
                    app,
                    "Edit",
                    true,
                    &[
                        &edit_undo,
                        &edit_redo,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::cut(app, None)?,
                        &edit_copy,
                        &edit_paste,
                        &PredefinedMenuItem::select_all(app, None)?,
                    ],
                )?;

                let new_tpl =
                    MenuItem::with_id(app, "new_template", "Create Template", true, None::<&str>)?;
                let save_tpl = MenuItem::with_id(
                    app,
                    "save_template",
                    "Save Template",
                    true,
                    Some("CmdOrCtrl+S"),
                )?;
                let select_template = MenuItem::with_id(
                    app,
                    "select_template",
                    "Select Template…",
                    true,
                    None::<&str>,
                )?;
                let show_tpl_dir = MenuItem::with_id(
                    app,
                    "show_templates",
                    "Show Templates Folder",
                    true,
                    None::<&str>,
                )?;
                let add_custom_font = MenuItem::with_id(
                    app,
                    "add_custom_font",
                    "Add Custom Font...",
                    true,
                    None::<&str>,
                )?;
                let templates_submenu = Submenu::with_items(
                    app,
                    "Templates",
                    true,
                    &[
                        &select_template,
                        &PredefinedMenuItem::separator(app)?,
                        &new_tpl,
                        &save_tpl,
                        &PredefinedMenuItem::separator(app)?,
                        &show_tpl_dir,
                        &PredefinedMenuItem::separator(app)?,
                        &add_custom_font,
                    ],
                )?;

                let act_open = MenuItem::with_id(
                    app,
                    "activities_open_gpx",
                    "Select Activity…",
                    true,
                    None::<&str>,
                )?;
                let act_show_folder = MenuItem::with_id(
                    app,
                    "activities_show_folder",
                    "Show Activities Folder",
                    true,
                    None::<&str>,
                )?;
                let activities_submenu = Submenu::with_items(
                    app,
                    "Activities",
                    true,
                    &[
                        &act_open,
                        &PredefinedMenuItem::separator(app)?,
                        &act_show_folder,
                    ],
                )?;

                let help_docs =
                    MenuItem::with_id(app, "help_docs", "Documentation", true, None::<&str>)?;
                let help_issues =
                    MenuItem::with_id(app, "help_issues", "Report an Issue", true, None::<&str>)?;
                let check_updates = MenuItem::with_id(
                    app,
                    "check_updates",
                    "Check for Updates...",
                    true,
                    None::<&str>,
                )?;
                let help_submenu = Submenu::with_items(
                    app,
                    "Help",
                    true,
                    &[
                        &check_updates,
                        &PredefinedMenuItem::separator(app)?,
                        &help_docs,
                        &help_issues,
                    ],
                )?;

                #[cfg(debug_assertions)]
                let dev_reset =
                    MenuItem::with_id(app, "dev_reset", "Reset App State", true, None::<&str>)?;
                #[cfg(debug_assertions)]
                let dev_build_label = MenuItem::with_id(
                    app,
                    "dev_build_info",
                    app_build_info(),
                    false,
                    None::<&str>,
                )?;
                #[cfg(debug_assertions)]
                let dev_submenu = Submenu::with_items(
                    app,
                    "Developer",
                    true,
                    &[
                        &dev_reset,
                        &PredefinedMenuItem::separator(app)?,
                        &dev_build_label,
                    ],
                )?;

                #[cfg(debug_assertions)]
                let menu = Menu::with_items(
                    app,
                    &[
                        &file_submenu,
                        &edit_submenu,
                        &templates_submenu,
                        &activities_submenu,
                        &exports_submenu,
                        &help_submenu,
                        &dev_submenu,
                    ],
                )?;
                #[cfg(not(debug_assertions))]
                let menu = Menu::with_items(
                    app,
                    &[
                        &file_submenu,
                        &edit_submenu,
                        &templates_submenu,
                        &activities_submenu,
                        &exports_submenu,
                        &help_submenu,
                    ],
                )?;
                app.set_menu(menu)?;

                app.on_menu_event(|app_handle, event| {
                    use tauri::Emitter;
                    let id = event.id().as_ref();
                    match id {
                        "edit_undo" => {
                            app_handle.emit("menu_undo", ()).ok();
                        }
                        "edit_redo" => {
                            app_handle.emit("menu_redo", ()).ok();
                        }
                        "edit_copy" => {
                            app_handle.emit("menu_copy", ()).ok();
                        }
                        "edit_paste" => {
                            app_handle.emit("menu_paste", ()).ok();
                        }
                        "about" => {
                            app_handle.emit("menu_about", ()).ok();
                        }
                        "settings" => {
                            app_handle.emit("menu_settings", ()).ok();
                        }
                        "activities_open_gpx" => {
                            app_handle.emit("menu_open_gpx", ()).ok();
                        }
                        "add_video" => {
                            app_handle.emit("menu_add_video", ()).ok();
                        }
                        "activities_show_folder" => {
                            app_handle.emit("menu_show_activities", ()).ok();
                        }
                        "save_template" => {
                            app_handle.emit("menu_save_template", ()).ok();
                        }
                        "new_template" => {
                            app_handle.emit("menu_new_template", ()).ok();
                        }
                        "select_template" => {
                            app_handle.emit("menu_show_template_dialog", ()).ok();
                        }
                        "add_custom_font" => {
                            app_handle.emit("menu_add_custom_font", ()).ok();
                        }
                        "show_downloads" => {
                            app_handle.emit("menu_show_downloads", ()).ok();
                        }
                        "overlays_save_path" => {
                            app_handle.emit("menu_settings", ()).ok();
                        }
                        "show_templates" => {
                            app_handle.emit("menu_open_templates_folder", ()).ok();
                        }
                        "check_updates" => {
                            app_handle.emit("check_for_updates", ()).ok();
                        }
                        "help_docs" => {
                            open_url("https://cyclemetry.walkersutton.com/?ref=app-help");
                        }
                        "help_issues" => {
                            open_url("https://github.com/walkersutton/cyclemetry/issues/new");
                        }
                        _ => {}
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::load_activity_cached;

    /// The parsed-activity cache must return the same parse for an unchanged
    /// file and re-parse when the file's content (stamp) changes — this is
    /// what collapses the N-parses-per-timeline-trim storm into one.
    #[test]
    fn parsed_activity_cache_hits_and_invalidates() {
        let gpx = |elev: &str| {
            format!(
                r#"<gpx><trk><trkseg>
                <trkpt lat="1" lon="2"><ele>{elev}</ele><time>2026-01-01T00:00:00Z</time></trkpt>
                <trkpt lat="1" lon="2.001"><ele>{elev}</ele><time>2026-01-01T00:00:02Z</time></trkpt>
                </trkseg></trk></gpx>"#
            )
        };
        let dir = std::env::temp_dir().join("cyclemetry-cache-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ride.gpx");
        let path_str = path.to_string_lossy().to_string();

        std::fs::write(&path, gpx("100")).unwrap();
        let a = load_activity_cached(&path_str).unwrap();
        let b = load_activity_cached(&path_str).unwrap();
        assert!(
            std::sync::Arc::ptr_eq(&a, &b),
            "unchanged file should hit cache"
        );

        // Different content length → different stamp → fresh parse.
        std::fs::write(&path, gpx("1234.5")).unwrap();
        let c = load_activity_cached(&path_str).unwrap();
        assert!(
            !std::sync::Arc::ptr_eq(&a, &c),
            "changed file should re-parse"
        );
        assert_ne!(a.elevation[0], c.elevation[0]);

        std::fs::remove_dir_all(&dir).ok();
    }
}
