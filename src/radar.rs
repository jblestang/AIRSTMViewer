use bevy::prelude::*;
use bevy::math::DVec3;

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
}

impl Default for Radars {
    fn default() -> Self {
        Self {
            stations: vec![
                Radar {
                    name: "Mont Agel".to_string(),
                    position: DVec3::new(43.77528, 7.42639, 1248.0), 
                    enabled: true,
                    color: Color::srgb(0.0, 1.0, 1.0), // Cyan
                    frequency: 1.3e9, // 1.3 GHz (L-Band)
                    transmit_power_dbm: 80.0, // 100 kW (Typical En-Route Peak)
                    gain_dbi: 35.0, // High gain antenna
                    sensitivity_dbm: -113.0, // High sensitivity
                },
                Radar {
                    name: "Sainte-Baume".to_string(),
                    position: DVec3::new(43.3337, 5.7866, 1148.0),
                    enabled: true,
                    color: Color::srgb(1.0, 0.0, 1.0), // Magenta
                    frequency: 1.3e9,
                    transmit_power_dbm: 80.0,
                    gain_dbi: 35.0,
                    sensitivity_dbm: -113.0,
                },
                Radar {
                    name: "Lyon (Mont Verdun)".to_string(),
                    position: DVec3::new(45.8498, 4.7795, 626.0),
                    enabled: true,
                    color: Color::srgb(1.0, 1.0, 0.0), // Yellow
                    frequency: 1.3e9,
                    transmit_power_dbm: 80.0,
                    gain_dbi: 35.0,
                    sensitivity_dbm: -113.0,
                },
            ],
        }
    }
}

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
            if radar.is_visible_raycast(target_lat, target_lon, target_alt, cache_snapshot) {
                return (true, Some(radar.color));
            }
        }
        (false, None)
    }
}

impl Radar {
    /// Calculate Maximum Detection Range using the Radar Range Equation
    /// Returns range in meters
    pub fn calculate_max_range(&self) -> f64 {
        const SPEED_OF_LIGHT: f64 = 299_792_458.0;
        const BOLTZMANN: f64 = 1.380649e-23;
        const REF_TEMP: f64 = 290.0;
        const DEFAULT_RCS: f64 = 5.0; // 5 m^2 (Typical fighter/small aircraft)

        // Convert decibels to linear units
        let p_t = 10.0_f64.powf((self.transmit_power_dbm - 30.0) / 10.0); // Watts
        let g = 10.0_f64.powf(self.gain_dbi / 10.0); // Linear Gain
        let p_min = 10.0_f64.powf((self.sensitivity_dbm - 30.0) / 10.0); // Watts

        let lambda = SPEED_OF_LIGHT / self.frequency;

        // Radar Range Equation:
        // R_max = [ (P_t * G^2 * lambda^2 * sigma) / ((4*pi)^3 * P_min) ] ^ (1/4)
        
        let numerator = p_t * g * g * lambda * lambda * DEFAULT_RCS;
        let denominator = (4.0 * std::f64::consts::PI).powi(3) * p_min;
        
        if denominator == 0.0 {
            return 0.0;
        }

        (numerator / denominator).powf(0.25)
    }

    /// Calculate if a target point is within Radio Line of Sight (LOS)
    /// Uses 4/3 Earth Radius approximation AND Physics-based Range Check
    pub fn is_visible(&self, target_lat: f64, target_lon: f64, target_alt: f32) -> bool {
        if !self.enabled {
            return false;
        }

        // Check against Physics Calculated Max Range first
        let max_physics_range = self.calculate_max_range();

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

    /// Calculate visibility with terrain occlusion (Raycasting)
    /// Optimized for performance: Cached TileData access to avoid hash lookups per step.
    pub fn is_visible_raycast(&self, target_lat: f64, target_lon: f64, target_alt: f32, cache_snapshot: &std::collections::HashMap<crate::tile::TileCoord, std::sync::Arc<crate::tile::TileData>>) -> bool {
        if !self.enabled {
            return false;
        }

        // 1. Fast Horizon Check
        if !self.is_visible(target_lat, target_lon, target_alt) {
            return false;
        }

        // 2. Perform Raymarching
        // Earth Constants
        const R_EARTH: f64 = 6_371_000.0;
        const R_EFF: f64 = R_EARTH * (4.0/3.0);
        
        let start_lat = self.position.x;
        let start_lon = self.position.y;
        let start_alt = self.position.z; 

        // Calculate total distance
        let d_lat = (target_lat - start_lat).to_radians();
        let d_lon = (target_lon - start_lon).to_radians();
        
        // Haversine calc
        let lat1 = start_lat.to_radians();
        let lat2 = target_lat.to_radians();
        let a = (d_lat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();
        let total_dist = R_EARTH * c;
        
        if total_dist < 100.0 {
            return true;
        }
        
        // Raymarch parameters
        // We march along the Great Circle path from source to target.
        // At each step, we check the height of the ray against the terrain height.
        let step_size = 500.0; // Meters. Smaller steps = higher precision but slower.
        let num_steps = (total_dist / step_size).ceil() as usize;
        // Clamp steps to avoid freezing on very long paths or over-calculating short ones
        let num_steps = num_steps.max(5).min(200); 
        
        // Access Optimization: Cache the current tile data locally to avoid Hash lookups
        use crate::tile::TileCoord;
        let mut current_tile_coord: Option<TileCoord> = None;
        let mut current_tile_data: Option<&crate::tile::TileData> = None;

        for i in 1..num_steps {
            let t = i as f64 / num_steps as f64;
            
            let cur_lat = start_lat + (target_lat - start_lat) * t;
            let cur_lon = start_lon + (target_lon - start_lon) * t;
            
            // Height of Ray Calculation
            // We interpolate linearly between Source Altitude and Target Altitude.
            // Then we subtract the "Earth Curvature Drop" which is the height lost due to the
            // earth curving away from the tangent plane of the start point.
            // Drop Formula: h = d^2 / (2 * R_eff)
            let dist_from_start = total_dist * t;
            let linear_h = start_alt + (target_alt as f64 - start_alt) * t;
            let earth_curvature_drop = (dist_from_start * (total_dist - dist_from_start)) / (2.0 * R_EFF);
            let ray_h = linear_h - earth_curvature_drop;
            
            if ray_h > 5000.0 {
                continue;
            }

            // Optimized Tile Lookup
            let coord = TileCoord::from_world_coords(cur_lat, cur_lon);
            
            // Update local cache if entered new tile
            if current_tile_coord != Some(coord) {
                 current_tile_coord = Some(coord);
                 // cache_snapshot is HashMap<TileCoord, Arc<TileData>>
                 if let Some(data_arc) = cache_snapshot.get(&coord) {
                     current_tile_data = Some(data_arc.as_ref());
                 } else {
                     current_tile_data = None;
                 }
            }

            // Check terrain if data available
            if let Some(data) = current_tile_data {
                // Inline logic from get_height_global to use direct reference
                let lat_base = coord.lat as f64;
                let lon_base = coord.lon as f64;
                
                let d_lat = cur_lat - lat_base;
                let d_lon = cur_lon - lon_base;
                
                let ny = (1.0 - d_lat) as f32; // Inverted Y for SRTM
                let nx = d_lon as f32;
                
                let terrain_h = data.get_height_normalized(nx, ny);
                
                if (terrain_h as f64) > ray_h {
                    return false; // Occluded
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

        let max_range_km = radar.calculate_max_range() / 1000.0;
        info!("Radar '{}' Physics Range: {:.1} km (Power: {:.1} dBm, Gain: {:.1} dBi)", 
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

/// System to continuously snap the radar marker to the ground surface
pub fn update_radar_position_system(
    radars: Res<Radars>,
    cache: Res<crate::cache::TileCache>,
    mut query: Query<(&mut Transform, &RadarMarker)>,
) {
    for (mut transform, marker) in query.iter_mut() {
        if marker.index >= radars.stations.len() {
            continue;
        }
        let radar = &radars.stations[marker.index];
        if !radar.enabled { continue; }

        let lat = radar.position.x;
        let lon = radar.position.y;
        
        // Check if we have data for this location
        let coord = crate::tile::TileCoord::from_world_coords(lat, lon);
        
        if let Some(crate::tile::TileState::Loaded(data)) = cache.tiles.get(&coord) {
             // Sample height
             let lat_base = coord.lat as f64;
             let lon_base = coord.lon as f64;
             
             let d_lat = lat - lat_base;
             let d_lon = lon - lon_base;
             
             // Y = (1.0 - d_lat) * 3600.0
             let y_pct = 1.0 - d_lat;
             let x_pct = d_lon;
             
             let pixel_x = (x_pct * 3600.0) as f32;
             let pixel_y = (y_pct * 3600.0) as f32;
             
             if let Some(h) = data.get_height(pixel_x as usize, pixel_y as usize) {
                 let terrain_height = h as f32; // Scale 1.0
                 
                 // Only update if significantly different
                 if (transform.translation.y - terrain_height).abs() > 10.0 {
                      transform.translation.y = terrain_height + 50.0; // Place on top
                 }
             }
        }
    }
}
