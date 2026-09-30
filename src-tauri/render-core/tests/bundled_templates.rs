//! Every bundled template must survive the real deserializer. Templates are
//! plain JSON with no compile-time checking, so a schema change in
//! `template.rs` can silently break a shipped template until someone opens it
//! in the app. This walks `templates/<name>/<name>.json` and parses each one
//! the same way the app does.

use render_core::template::Template;

#[test]
fn all_bundled_templates_parse() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates");
    let mut parsed = 0;

    for entry in std::fs::read_dir(dir).expect("templates dir") {
        let path = entry.expect("dir entry").path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let json = path.join(format!("{name}.json"));
        if !json.exists() {
            continue; // `_assets` and friends
        }

        let text = std::fs::read_to_string(&json).expect("readable template");
        let raw: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: not valid JSON: {e}"));
        // Scale to 1080p on the way in, exercising the same path the app uses
        // for a non-native output resolution.
        Template::from_value_scaled(raw, Some((1920, 1080)))
            .unwrap_or_else(|e| panic!("{name}: failed to deserialize: {e}"));

        parsed += 1;
    }

    assert!(parsed > 0, "found no bundled templates to check");
}
