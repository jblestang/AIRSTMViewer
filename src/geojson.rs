use bevy::prelude::*;
use std::path::PathBuf;
use serde_json::Value;
use crate::radar::{Radar, Radars};
use bevy::math::DVec3;

const GEOJSON_URL: &str = "https://climateviewer.org/layers/geojson/2018/Fortress-Russia-SAM-Sites-ClimateViewer-3D.geojson";
const CACHE_FILENAME: &str = "sam_sites.geojson";

pub fn load_sam_sites_system(mut radars: ResMut<Radars>) {
    let cache_dir = get_cache_dir();
    let cache_path = cache_dir.join(CACHE_FILENAME);

    let geojson_str = if cache_path.exists() {
        info!("Loading SAM sites from cache: {:?}", cache_path);
        std::fs::read_to_string(&cache_path).unwrap_or_default()
    } else {
        info!("Downloading SAM sites from: {}", GEOJSON_URL);
        match download_geojson(GEOJSON_URL) {
            Ok(data) => {
                if !cache_dir.exists() {
                    let _ = std::fs::create_dir_all(&cache_dir);
                }
                if let Err(e) = std::fs::write(&cache_path, &data) {
                    error!("Failed to cache GeoJSON: {}", e);
                }
                data
            }
            Err(e) => {
                error!("Failed to download SAM sites: {}", e);
                return;
            }
        }
    };

    if geojson_str.is_empty() {
        return;
    }

    //parse_and_inject_sites(geojson_str, &mut radars);
}

fn get_cache_dir() -> PathBuf {
    let current_dir = std::env::current_dir()
        .expect("Could not determine current directory");
    
    if current_dir.ends_with("assets") {
        current_dir
    } else {
        current_dir.join("assets")
    }
}

fn download_geojson(url: &str) -> Result<String, String> {
    reqwest::blocking::get(url)
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

fn parse_and_inject_sites(json_str: String, radars: &mut Radars) {
    let v: Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to parse GeoJSON: {}", e);
            return;
        }
    };

    let features = match v["features"].as_array() {
        Some(f) => f,
        None => {
            error!("GeoJSON features not found or not an array");
            return;
        }
    };

    let mut count = 0;
    for feature in features {
        // Extract Geometry
        let geom_type = feature["geometry"]["type"].as_str().unwrap_or("");
        if geom_type != "Point" {
            continue;
        }

        let coords = match feature["geometry"]["coordinates"].as_array() {
            Some(c) if c.len() >= 2 => c,
            _ => continue,
        };

        let lon = coords[0].as_f64().unwrap_or(0.0);
        let lat = coords[1].as_f64().unwrap_or(0.0);
        let alt = coords.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0);

        // Extract Metadata
        let name = feature["properties"]["name"].as_str().unwrap_or("Unknown SAM");
        let description = feature["properties"]["description"].as_str().unwrap_or("");

        // Identify Radar Kind from description
        let kind = identify_kind(name, description);

        // Create Radar and add to resource
        let radar = Radar::from_kind(name, DVec3::new(lat, lon, alt), &kind);
        radars.stations.push(radar);
        count += 1;
    }

    info!("Injected {} SAM sites into simulation", count);
}

fn identify_kind(name: &str, description: &str) -> String {
    let desc_upper = description.to_uppercase();
    let name_upper = name.to_uppercase();

    // Check for specific radar models first
    if desc_upper.contains("BIG BIRD") || desc_upper.contains("64N6") {
        "BIG BIRD".to_string()
    } else if desc_upper.contains("DON-2N") || name_upper.contains("DON-2N") {
        "DON-2N".to_string()
    } else if desc_upper.contains("FLAP LID") || desc_upper.contains("5N63") {
        "FLAP LID".to_string()
    } else if desc_upper.contains("CLAM SHELL") || desc_upper.contains("5N66") || desc_upper.contains("76N6") {
        "CLAM SHELL".to_string()
    } else if desc_upper.contains("TOMB STONE") || desc_upper.contains("30N6") {
        "TOMB STONE".to_string()
    } else if desc_upper.contains("GRILL PAN") || desc_upper.contains("9S32") {
        "GRILL PAN".to_string()
    } else if desc_upper.contains("SNOW DRIFT") || desc_upper.contains("9S18") {
        "SNOW DRIFT".to_string()
    } else if desc_upper.contains("CHEESE BOARD") || desc_upper.contains("9S117") {
        "CHEESE BOARD".to_string()
    } else if desc_upper.contains("BILL BOARD") || desc_upper.contains("9S15") {
        "BILL BOARD".to_string()
    } else if desc_upper.contains("GRAVE STONE") || desc_upper.contains("92N6") {
        "GRAVE STONE".to_string()
    // Generic system matches
    } else if desc_upper.contains("S-400") || name_upper.contains("S-400") {
        "S-400".to_string()
    } else if desc_upper.contains("S-300V") || name_upper.contains("S-300V") {
        "S-300V".to_string()
    } else if desc_upper.contains("S-300") || name_upper.contains("S-300") {
        "S-300P".to_string()
    } else if desc_upper.contains("BUK") || desc_upper.contains("SA-11") || desc_upper.contains("SA-17") {
        "BUK".to_string()
    } else if desc_upper.contains("TOR") || desc_upper.contains("SA-15") {
        "TOR".to_string()
    } else if desc_upper.contains("PANTSIR") || desc_upper.contains("SA-22") {
        "PANTSIR".to_string()
    } else {
        "MIL_AQ".to_string()
    }
}
