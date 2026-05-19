//! Terrain sampling and geodesy helpers shared by UI systems.

use bevy::prelude::*;
use crate::cache::TileCache;
use crate::tile::{TileCoord, TileData, TileState};

const TILE_SIZE: f32 = 3601.0;
/// SRTM void sentinel (no data).
const SRTM_VOID: i16 = -32768;

#[derive(Resource, Default, Clone, Copy)]
pub struct CursorTerrain {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub terrain_m: Option<f32>,
}

/// Sample SRTM terrain height (MSL, meters) at WGS84 coordinates.
pub fn sample_terrain_m(cache: &TileCache, lat: f64, lon: f64) -> Option<f32> {
    let coord = TileCoord::from_world_coords(lat, lon);
    let TileState::Loaded(data) = cache.tiles.get(&coord)? else {
        return None;
    };
    let lat_base = coord.lat as f64;
    let lon_base = coord.lon as f64;
    let ny = 1.0 - (lat - lat_base);
    let nx = lon - lon_base;
    if !(0.0..=1.0).contains(&ny) || !(0.0..=1.0).contains(&nx) {
        return None;
    }
    Some(data.get_height_normalized(nx as f32, ny as f32))
}

/// Nearest DEM grid cell (no interpolation) — preserves peaks for max-sampling profiles.
pub fn sample_terrain_nearest_m(cache: &TileCache, lat: f64, lon: f64) -> Option<f32> {
    let coord = TileCoord::from_world_coords(lat, lon);
    let TileState::Loaded(data) = cache.tiles.get(&coord)? else {
        return None;
    };
    sample_terrain_nearest_on_tile(data, coord, lat, lon)
}

pub fn sample_terrain_nearest_on_tile(
    data: &TileData,
    coord: TileCoord,
    lat: f64,
    lon: f64,
) -> Option<f32> {
    let lat_base = coord.lat as f64;
    let lon_base = coord.lon as f64;
    let ny = 1.0 - (lat - lat_base);
    let nx = lon - lon_base;
    if !(0.0..=1.0).contains(&ny) || !(0.0..=1.0).contains(&nx) {
        return None;
    }
    let max_idx = data.size.saturating_sub(1);
    let px = ((nx * max_idx as f64).round() as usize).min(max_idx);
    let py = ((ny * max_idx as f64).round() as usize).min(max_idx);
    let h = data.get_height(px, py)?;
    if h == SRTM_VOID {
        return None;
    }
    Some(h as f32)
}

/// Cast a camera ray and return the first terrain hit.
pub fn pick_terrain_under_cursor(
    origin: Vec3,
    direction: Dir3,
    cache: &TileCache,
) -> Option<(f64, f64, f32)> {
    let direction = direction.as_vec3();
    let max_dist = 80_000.0;
    let step_size = 40.0;
    let num_steps = (max_dist / step_size) as usize;

    for i in 0..num_steps {
        let dist = i as f32 * step_size;
        let pos = origin + direction * dist;
        let lat = -pos.z / TILE_SIZE;
        let lon = pos.x / TILE_SIZE;
        let coord = TileCoord::from_world_coords(lat as f64, lon as f64);

        if let Some(TileState::Loaded(data)) = cache.tiles.get(&coord) {
            if let Some(h) = sample_terrain_at_loaded(data, coord, lat as f64, lon as f64) {
                if pos.y <= h {
                    return Some((lat as f64, lon as f64, h));
                }
            }
        }
    }
    None
}

fn sample_terrain_at_loaded(
    data: &crate::tile::TileData,
    coord: TileCoord,
    lat: f64,
    lon: f64,
) -> Option<f32> {
    let lat_base = coord.lat as f64;
    let lon_base = coord.lon as f64;
    let ny = 1.0 - (lat - lat_base);
    let nx = lon - lon_base;
    if !(0.0..=1.0).contains(&ny) || !(0.0..=1.0).contains(&nx) {
        return None;
    }
    Some(data.get_height_normalized(nx as f32, ny as f32))
}

pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos()
            * lat2.to_radians().cos()
            * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    R * c
}

/// Initial bearing from point 1 to point 2 (degrees, clockwise from north).
pub fn initial_bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let y = d_lon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * d_lon.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// Great-circle destination from (lat, lon) given bearing (deg) and distance (m).
pub fn destination_point(lat: f64, lon: f64, bearing_deg: f64, distance_m: f64) -> (f64, f64) {
    const R: f64 = 6_371_000.0;
    let brng = bearing_deg.to_radians();
    let lat1 = lat.to_radians();
    let lon1 = lon.to_radians();
    let ang = distance_m / R;
    let lat2 = lat1.sin() * ang.cos() + lat1.cos() * ang.sin() * brng.cos();
    let lat2 = lat2.asin();
    let lon2 = lon1
        + (lat1.cos() * ang.sin() * brng.sin()).atan2(ang.cos() - lat1.sin() * lat2.sin());
    (lat2.to_degrees(), lon2.to_degrees())
}
