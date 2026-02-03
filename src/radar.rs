use bevy::prelude::*;
use bevy::math::DVec3;

/// Individual Radar Station
#[derive(Clone, Debug)]
pub struct Radar {
    pub name: String,
    pub position: DVec3, // Lat (deg), Lon (deg), Alt (meters)
    pub enabled: bool,
    pub max_range: f32, // Max range in meters
    pub color: Color,
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
                    max_range: 515_000.0,
                    color: Color::srgb(0.0, 1.0, 1.0), // Cyan
                },
                Radar {
                    name: "Sainte-Baume".to_string(),
                    // Jouc de l'Aigle coordinates
                    position: DVec3::new(43.3337, 5.7866, 1148.0),
                    enabled: true,
                    max_range: 515_000.0,
                    color: Color::srgb(1.0, 0.0, 1.0), // Magenta
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
    /// Calculate if a target point is within Radio Line of Sight (LOS)
    /// Uses 4/3 Earth Radius approximation
    pub fn is_visible(&self, target_lat: f64, target_lon: f64, target_alt: f32) -> bool {
        if !self.enabled {
            return false;
        }

        // Earth constants
        // 4/3 Earth Radius Model
        // This is a standard approximation in radio propagation to account for atmospheric refraction
        // which bends radio waves towards the earth, effectively increasing the radio horizon.
        // Reference: ITU-R P.452 "Prediction procedure for the evaluation of interference between stations on the surface of the Earth at frequencies above about 0.7 GHz"
        const R_EARTH: f64 = 6_371_000.0; // Mean Earth Radius in Meters
        const K_FACTOR: f64 = 4.0 / 3.0;  // Standard Refactive Index
        const R_EFF: f64 = R_EARTH * K_FACTOR; // Effective radius (~8494 km)

        // Calculate Great Circle Distance (Haversine or simple spherical)
        let d_lat = (target_lat - self.position.x).to_radians();
        let d_lon = (target_lon - self.position.y).to_radians();
        let lat1 = self.position.x.to_radians();
        let lat2 = target_lat.to_radians();

        let a = (d_lat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();
        let dist = R_EARTH * c; // Surface distance

        if dist > self.max_range as f64 {
            return false;
        }

        // Radio Horizon formula (Geometric check without terrain)
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
