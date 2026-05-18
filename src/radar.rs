use bevy::prelude::*;
use bevy::math::DVec3;
use crate::tile::TileCoord;

/// Individual Radar Station
#[derive(Clone, Debug)]
pub struct Radar {
    pub name: String,
    pub position: DVec3, // Lat (deg), Lon (deg), Alt (meters)
    pub enabled: bool,
    pub color: Color,
    
    // Physics Parameters
    pub frequency: f64,       // Hz (e.g. 1.3e9 for 1.3 GHz)
    pub transmit_power_dbm: f64, // dBm (e.g. 60.0 for 1kW)
    pub gain_dbi: f64,        // dBi (e.g. 30.0)
    pub sensitivity_dbm: f64, // dBm (e.g. -100.0)
}

/// Resource holding all radar stations
#[derive(Resource, Clone, Debug)]
pub struct Radars {
    pub stations: Vec<Radar>,
    pub target_altitude_agl: f32, // Target altitude above ground (meters)
    pub preset_index: usize,
    pub target_rcs: f64, // Target Radar Cross Section (m^2)
    pub rcs_preset_index: usize,
}

impl Default for Radars {
    fn default() -> Self {
        Self {
            target_altitude_agl: 0.0, // Default to 0m (Ground)
            preset_index: 0,
            target_rcs: 5.0, // Default to 5m^2 (Small Fighter)
            rcs_preset_index: 2, // Index of 5.0 in [0.1, 1.0, 5.0, 10.0]
            stations: vec![
                //Radar::from_kind("Mont Agel", DVec3::new(43.77, 7.4183, 1248.0), "CIVIL_ATCR"),
                Radar::from_kind("Sainte-Baume", DVec3::new(43.3337, 5.7866, 1148.0), "CIVIL_ATCR"),
                Radar::from_kind("Lyon", DVec3::new(45.8498, 4.7795, 626.0), "MIL_AQ"),
            ],
        }
    }
}

impl Radar {
    /// Create a radar with parameters based on its type/kind
    pub fn from_kind(name: &str, pos: DVec3, kind: &str) -> Self {
        let (freq, power, gain, sens, color) = match kind {
            
            "MIL_AQ" => (3.0e9, 68.0, 40.0, -112.0, Color::srgb(1.0, 1.0, 0.0)), 
            "CIVIL_ATCR" => (1.3e9, 55.0, 35.0, -113.0, Color::srgb(0.0, 1.0, 1.0)), 
            _ => (2.0e9, 60.0, 35.0, -110.0, Color::srgb(0.7, 0.7, 0.7)), 
        };

        Self {
            name: name.to_string(),
            position: pos,
            enabled: true,
            color,
            frequency: freq,
            transmit_power_dbm: power,
            gain_dbi: gain,
            sensitivity_dbm: sens,
        }
    }
}

pub const ALTITUDE_PRESETS: [f32; 8] = [0.0, 30.0, 100.0, 150.0, 500.0, 1000.0, 5000.0, 10000.0];
pub const RCS_PRESETS: [f64; 5] = [0.1, 1.0, 5.0, 10.0, 100.0];

impl Radars {
    /// Check if a point is visible by ANY enabled radar station.
    /// Returns (is_visible, color_of_station)
    pub fn check_visibility(
        &self,
        target_lat: f64,
        target_lon: f64,
        target_alt: f32,
        cache_snapshot: &std::collections::HashMap<crate::tile::TileCoord, std::sync::Arc<crate::tile::TileData>>,
    ) -> (bool, Option<Color>) {
        for radar in &self.stations {
            if !radar.enabled { continue; }
            if radar.is_visible_raycast(target_lat, target_lon, target_alt, self.target_rcs, cache_snapshot) {
                return (true, Some(radar.color));
            }
        }
        (false, None)
    }
    /// Identify which radars can actually reach a specific tile
    /// This is used to cull the list of radars checked per vertex
    pub fn get_relevant_radars(&self, tile_coord: TileCoord) -> Vec<usize> {
        let mut relevant = Vec::new();
        
        let tile_lat = tile_coord.lat as f64 + 0.5; // Tile center
        let tile_lon = tile_coord.lon as f64 + 0.5;
        
        // Earth constant for distance check
        const R_EARTH: f64 = 6_371_000.0;
        
        for (idx, radar) in self.stations.iter().enumerate() {
            if !radar.enabled { continue; }
            
            // 1. Physics Range check (Approximate distance to tile)
            let max_range = radar.calculate_max_range(self.target_rcs);
            
            // Haversine distance to tile center
            let d_lat = (tile_lat - radar.position.x).to_radians();
            let d_lon = (tile_lon - radar.position.y).to_radians();
            let a = (d_lat / 2.0).sin().powi(2)
                + radar.position.x.to_radians().cos() * tile_lat.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
            let c = 2.0 * a.sqrt().asin();
            let dist_to_center = R_EARTH * c;
            
            // Buffer of 150km (approx diagonal of 1x1 degree tile at equator)
            if dist_to_center > max_range + 150_000.0 {
                continue;
            }
            
            relevant.push(idx);
        }
        relevant
    }
}

pub fn update_radar_settings_system(
    mut radars: ResMut<Radars>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
) {
    let alt_pressed = keyboard_input.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    
    if alt_pressed {
         if keyboard_input.just_pressed(KeyCode::ArrowUp) {
             let new_idx = (radars.preset_index + 1).min(ALTITUDE_PRESETS.len() - 1);
             if new_idx != radars.preset_index {
                 radars.preset_index = new_idx;
                 radars.target_altitude_agl = ALTITUDE_PRESETS[new_idx];
                 info!("Radar Coverage AGL set to {:.0} m", radars.target_altitude_agl);
             }
         }
         
         if keyboard_input.just_pressed(KeyCode::ArrowDown) {
             if radars.preset_index > 0 {
                  radars.preset_index -= 1;
                  radars.target_altitude_agl = ALTITUDE_PRESETS[radars.preset_index];
                  info!("Radar Coverage AGL set to {:.0} m", radars.target_altitude_agl);
             }
         }

         if keyboard_input.just_pressed(KeyCode::ArrowRight) {
             let new_idx = (radars.rcs_preset_index + 1).min(RCS_PRESETS.len() - 1);
             if new_idx != radars.rcs_preset_index {
                 radars.rcs_preset_index = new_idx;
                 radars.target_rcs = RCS_PRESETS[new_idx];
                 
                 let mut total_range_km = 0.0;
                 for radar in &radars.stations {
                    total_range_km += radar.calculate_max_range(radars.target_rcs) / 1000.0;
                 }
                 let avg_range = if radars.stations.is_empty() { 0.0 } else { total_range_km / radars.stations.len() as f64 };
                 info!("Radar Target RCS set to {:.1} m^2 ({} stations updated, Avg Range: {:.1} km)", 
                       radars.target_rcs, radars.stations.len(), avg_range);
             }
         }

         if keyboard_input.just_pressed(KeyCode::ArrowLeft) {
             if radars.rcs_preset_index > 0 {
                 radars.rcs_preset_index -= 1;
                 radars.target_rcs = RCS_PRESETS[radars.rcs_preset_index];
                 
                 let mut total_range_km = 0.0;
                 for radar in &radars.stations {
                    total_range_km += radar.calculate_max_range(radars.target_rcs) / 1000.0;
                 }
                 let avg_range = if radars.stations.is_empty() { 0.0 } else { total_range_km / radars.stations.len() as f64 };
                 info!("Radar Target RCS set to {:.1} m^2 ({} stations updated, Avg Range: {:.1} km)", 
                       radars.target_rcs, radars.stations.len(), avg_range);
             }
         }
    }
}

impl Radar {
    /// Calculate Maximum Detection Range using the Radar Range Equation
    /// Returns range in meters
    pub fn calculate_max_range(&self, target_rcs: f64) -> f64 {
        const SPEED_OF_LIGHT: f64 = 299_792_458.0;
        // const BOLTZMANN: f64 = 1.380649e-23;
        // const REF_TEMP: f64 = 290.0;
        // const DEFAULT_RCS: f64 = 5.0; // REMOVED - Using argument now

        // Convert decibels to linear units
        let p_t = 10.0_f64.powf((self.transmit_power_dbm - 30.0) / 10.0); // Watts
        let g = 10.0_f64.powf(self.gain_dbi / 10.0); // Linear Gain
        let p_min = 10.0_f64.powf((self.sensitivity_dbm - 30.0) / 10.0); // Watts

        let lambda = SPEED_OF_LIGHT / self.frequency;

        // Radar Range Equation:
        // R_max = [ (P_t * G^2 * lambda^2 * sigma) / ((4*pi)^3 * P_min) ] ^ (1/4)
        
        let numerator = p_t * g * g * lambda * lambda * target_rcs;
        let denominator = (4.0 * std::f64::consts::PI).powi(3) * p_min;
        
        if denominator == 0.0 {
            return 0.0;
        }

        (numerator / denominator).powf(0.25)
    }

    /// Calculate if a target point is within Radio Line of Sight (LOS)
    /// Uses 4/3 Earth Radius approximation AND Physics-based Range Check
    pub fn is_visible(&self, target_lat: f64, target_lon: f64, target_alt: f32, target_rcs: f64) -> bool {
        if !self.enabled {
            return false;
        }

        // Check against Physics Calculated Max Range first
        let max_physics_range = self.calculate_max_range(target_rcs);

        // Earth constants
        // 4/3 Earth Radius Model
        const R_EARTH: f64 = 6_371_000.0; // Mean Earth Radius in Meters
        const R_EFF: f64 = R_EARTH * (4.0/3.0); // Effective radius

        // Calculate Great Circle Distance
        let d_lat = (target_lat - self.position.x).to_radians();
        let d_lon = (target_lon - self.position.y).to_radians();
        let lat1 = self.position.x.to_radians();
        let lat2 = target_lat.to_radians();

        let a = (d_lat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();
        let dist = R_EARTH * c; // Surface distance

        // 1. Physics Range Check
        if dist > max_physics_range {
            return false; 
        }

        // 2. Radio Horizon Check (Geometric)
        let h_radar = self.position.z.max(0.0);
        let h_target = target_alt.max(0.0) as f64;

        let d_radar = (2.0 * h_radar * R_EFF).sqrt();
        let d_target = (2.0 * h_target * R_EFF).sqrt();

        dist <= (d_radar + d_target)
    }

    /// Calculate visibility with terrain occlusion (Raycasting).
    /// Prefer `is_visible_raycast_precomputed` in hot loops — it avoids recomputing
    /// `calculate_max_range` and the haversine on every vertex.
    pub fn is_visible_raycast(&self, target_lat: f64, target_lon: f64, target_alt: f32, target_rcs: f64, cache_snapshot: &std::collections::HashMap<crate::tile::TileCoord, std::sync::Arc<crate::tile::TileData>>) -> bool {
        let max_range = self.calculate_max_range(target_rcs);
        self.is_visible_raycast_precomputed(target_lat, target_lon, target_alt, max_range, cache_snapshot)
    }

    /// Hot-loop version: accepts a precomputed `max_range` to avoid the 3× `f64::powf()`
    /// inside `calculate_max_range()` being called once per vertex per radar.
    /// Also computes the haversine **once** (the original code computed it twice: once in
    /// `is_visible()` for the range/horizon check, then again at the top of the raycast
    /// for `total_dist`).
    pub fn is_visible_raycast_precomputed(
        &self,
        target_lat: f64,
        target_lon: f64,
        target_alt: f32,
        precomputed_max_range: f64,
        cache_snapshot: &std::collections::HashMap<crate::tile::TileCoord, std::sync::Arc<crate::tile::TileData>>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        const R_EARTH: f64 = 6_371_000.0;
        const R_EFF: f64   = R_EARTH * (4.0 / 3.0);

        let start_lat = self.position.x;
        let start_lon = self.position.y;
        let start_alt = self.position.z;

        // Haversine distance — computed ONCE and reused for range, horizon, and ray steps.
        let d_lat = (target_lat - start_lat).to_radians();
        let d_lon = (target_lon - start_lon).to_radians();
        let lat1  = start_lat.to_radians();
        let lat2  = target_lat.to_radians();
        let a = (d_lat / 2.0).sin().powi(2)
              + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();
        let total_dist = R_EARTH * c;

        // 1. Physics range check (uses precomputed max_range — no powf call here)
        if total_dist > precomputed_max_range {
            return false;
        }

        // 2. Radio horizon check (4/3 Earth radius model)
        let h_radar  = self.position.z.max(0.0);
        let h_target = target_alt.max(0.0) as f64;
        let d_radar  = (2.0 * h_radar  * R_EFF).sqrt();
        let d_target = (2.0 * h_target * R_EFF).sqrt();
        if total_dist > d_radar + d_target {
            return false;
        }

        if total_dist < 100.0 {
            return true;
        }

        // 3. Terrain raymarching — step size matched to range for minimal samples
        let step_size = if total_dist > 50_000.0 { 1000.0 } else { 500.0 };
        let num_steps = (total_dist / step_size).ceil() as usize;
        let num_steps = num_steps.max(2).min(1000);

        use crate::tile::TileCoord;
        let mut current_tile_coord: Option<TileCoord> = None;
        let mut current_tile_data: Option<&crate::tile::TileData> = None;

        for i in 1..num_steps {
            let t = i as f64 / num_steps as f64;

            let cur_lat = start_lat + (target_lat - start_lat) * t;
            let cur_lon = start_lon + (target_lon - start_lon) * t;

            let dist_from_start    = total_dist * t;
            let linear_h           = start_alt + (target_alt as f64 - start_alt) * t;
            let earth_curvature_drop = (dist_from_start * (total_dist - dist_from_start)) / (2.0 * R_EFF);
            let ray_h              = linear_h - earth_curvature_drop;

            if ray_h > 5000.0 {
                continue;
            }

            let coord = TileCoord::from_world_coords(cur_lat, cur_lon);

            if current_tile_coord != Some(coord) {
                current_tile_coord = Some(coord);
                current_tile_data  = cache_snapshot.get(&coord).map(|d| d.as_ref());
            }

            if let Some(data) = current_tile_data {
                let lat_base = coord.lat as f64;
                let lon_base = coord.lon as f64;
                let dl = cur_lat - lat_base;
                let dl2 = cur_lon - lon_base;
                let ny = (1.0 - dl) as f32;
                let nx = dl2 as f32;
                let terrain_h = data.get_height_normalized(nx, ny);
                if (terrain_h as f64) > ray_h {
                    return false;
                }
            }
        }

        true
    }
}

/// System to spawn a visual marker at the radar position
pub fn setup_radar_marker(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    radars: Res<Radars>,
) {
    let tile_size = 3601.0;
    let height_scale = 1.0; 

    for (index, radar) in radars.stations.iter().enumerate() {
        if !radar.enabled {
            continue;
        }

        let max_range_km = radar.calculate_max_range(5.0) / 1000.0; // Use 5.0m^2 for visualization default
        info!("Radar '{}' Physics Range (RCS 5m^2): {:.1} km (Power: {:.1} dBm, Gain: {:.1} dBi)", 
              radar.name, max_range_km, radar.transmit_power_dbm, radar.gain_dbi);

        let x = radar.position.y as f32 * tile_size;
        let z = -(radar.position.x as f32) * tile_size; 
        let y = radar.position.z as f32 * height_scale;

        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(100.0))), 
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: radar.color, 
                emissive: LinearRgba::from(radar.color) * 5.0, 
                unlit: true,
                ..default()
            })),
            Transform::from_xyz(x, y + 100.0, z), 
            RadarMarker { index },
        ));
    }
}

#[derive(Component)]
pub struct RadarMarker {
    pub index: usize,
}

/// System to continuously snap the radar marker and the radar resource to the ground surface
pub fn update_radar_position_system(
    mut radars: ResMut<Radars>,
    cache: Res<crate::cache::TileCache>,
    mut query: Query<(&mut Transform, &RadarMarker)>,
) {
    for (mut transform, marker) in query.iter_mut() {
        if marker.index >= radars.stations.len() {
            continue;
        }
        
        let (lat, lon) = {
            let radar = &radars.stations[marker.index];
            (radar.position.x, radar.position.y)
        };
        
        // Check if we have data for this location
        let coord = crate::tile::TileCoord::from_world_coords(lat, lon);
        
        if let Some(crate::tile::TileState::Loaded(data)) = cache.tiles.get(&coord) {
             // Sample height
             let lat_base = coord.lat as f64;
             let lon_base = coord.lon as f64;
             
             let d_lat = lat - lat_base;
             let d_lon = lon - lon_base;
             
             // Inverted Y for SRTM
             let y_pct = 1.0 - d_lat;
             let x_pct = d_lon;
             
             let pixel_x = (x_pct * 3600.0) as f32;
             let pixel_y = (y_pct * 3600.0) as f32;
             
             if let Some(h) = data.get_height(pixel_x as usize, pixel_y as usize) {
                  let terrain_height = h as f32; 
                  
                  // Sync Physics Position in Resource
                  let radar = &mut radars.stations[marker.index];
                  if (radar.position.z - terrain_height as f64).abs() > 0.5 {
                      radar.position.z = terrain_height as f64;
                  }

                  // Sync Visual Marker Transform
                  if (transform.translation.y - terrain_height).abs() > 0.5 {
                       transform.translation.y = terrain_height + 50.0; 
                  }
             }
        }
    }
}
