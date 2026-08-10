//! Deserialization of the device/palette manifest exported from trmnl-core
//! (`trmnl-framework/db/data/framework_devices.yml`).

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

static FRAMEWORK_DEVICES: &str =
    include_str!("../../../trmnl-framework/db/data/framework_devices.yml");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FrameworkDevices {
    pub exported_from: String,
    pub devices: Vec<Device>,
    pub palettes: Vec<Palette>,
    pub picker_models: Vec<PickerModel>,
    pub picker_palettes: Vec<Palette>,
    pub example_plugins: HashMap<String, ExamplePlugin>,
}

impl FrameworkDevices {
    pub fn load() -> Self {
        let mut manifest: Self =
            serde_yaml::from_str(FRAMEWORK_DEVICES).expect("framework_devices.yml does not parse");
        manifest.picker_models.sort_by(|a, b| {
            let a_trmnl = a.kind == "trmnl";
            let b_trmnl = b.kind == "trmnl";
            b_trmnl.cmp(&a_trmnl).then_with(|| a.label.cmp(&b.label))
        });
        manifest
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Device {
    pub id: u32,
    pub name: String,
    pub screen_picker_name: String,
    pub keyname: String,
    pub visible: bool,
    pub scale_factor: f64,
    pub color_depth: u8,
    pub density_class: String,
    pub palette_ids: Vec<String>,
    pub css: DeviceCss,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeviceCss {
    pub keyname: String,
    pub screen_w: u32,
    pub screen_h: u32,
    pub ui_scale: f64,
    pub gap_scale: f64,
    pub size: String,
    pub bit_depth: u8,
    pub pixel_ratio: f64,
    pub dither_pixel_ratio: f64,
}

/// Used for both `palettes` and `picker_palettes`; `colors` may be null,
/// `[]`, a hex list, or missing entirely depending on the palette.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Palette {
    pub id: String,
    pub name: String,
    pub grays: u32,
    pub colors: Option<Vec<String>>,
    pub framework_class: String,
    pub grayscale_bit_depth: Option<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PickerModel {
    pub name: String,
    pub label: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
    pub colors: u32,
    pub bit_depth: u8,
    pub scale_factor: f64,
    pub rotation: i32,
    pub mime_type: String,
    pub offset_x: i32,
    pub offset_y: i32,
    pub kind: String,
    pub palette_ids: Vec<String>,
    pub preview_white_point: String,
    pub image_size_limit: u64,
    pub image_upload_supported: bool,
    /// Absent for models without a framework screen (e.g. `tidbyt`).
    pub css: Option<PickerModelCss>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PickerModelCss {
    pub classes: PickerModelCssClasses,
    /// CSS custom-property `[name, value]` pairs, e.g. `["--screen-w", "1040px"]`.
    pub variables: Vec<(String, String)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PickerModelCssClasses {
    pub device: String,
    pub size: String,
    pub density: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExamplePlugin {
    pub name: String,
    pub description: String,
    pub image_url: String,
    pub image_dark_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_bundled_manifest() {
        let manifest = FrameworkDevices::load();
        assert_eq!(manifest.exported_from, "trmnl-core");
        assert!(!manifest.devices.is_empty());
        assert!(!manifest.palettes.is_empty());
        assert!(!manifest.picker_models.is_empty());
        assert!(!manifest.picker_palettes.is_empty());
        assert!(!manifest.example_plugins.is_empty());

        let trmnl_x = manifest
            .devices
            .iter()
            .find(|d| d.keyname == "v2")
            .expect("TRMNL X device present");
        assert_eq!(trmnl_x.css.screen_w, 1040);
        assert_eq!(trmnl_x.css.screen_h, 780);

        let trmnl_count = manifest
            .picker_models
            .iter()
            .filter(|m| m.kind == "trmnl")
            .count();
        assert!(trmnl_count > 0);
        let (trmnl, rest) = manifest.picker_models.split_at(trmnl_count);
        assert!(trmnl.iter().all(|m| m.kind == "trmnl"));
        assert!(trmnl.windows(2).all(|w| w[0].label <= w[1].label));
        assert!(rest.windows(2).all(|w| w[0].label <= w[1].label));
    }
}
