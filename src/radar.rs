use bevy::prelude::*;
use bevy::math::DVec3;

/// Radar configuration resource
#[derive(Resource, Clone, Debug)]
pub struct Radar {
    pub position: DVec3, // Lat (deg), Lon (deg), Alt (meters)
    pub enabled: bool,
    pub max_range: f32, // Max range in meters
}

impl Default for Radar {
    fn default() -> Self {
        Self {
            // Mont Agel coordinates (Wikipedia: 43.77528, 7.42639)
            position: DVec3::new(43.77528, 7.42639, 1148.0), 
            enabled: true,
            max_range: 400_000.0, // 400 km range
        }
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
        const R_EARTH: f64 = 6_371_000.0; // Meters
        const K_FACTOR: f64 = 4.0 / 3.0;
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
        let step_size = 500.0; 
        let num_steps = (total_dist / step_size).ceil() as usize;
        let num_steps = num_steps.max(5).min(200); 
        
        // Access Optimization: Cache the current tile data locally to avoid Hash lookups
        use crate::tile::{TileCoord, TileState};
        let mut current_tile_coord: Option<TileCoord> = None;
        let mut current_tile_data: Option<&crate::tile::TileData> = None;

        for i in 1..num_steps {
            let t = i as f64 / num_steps as f64;
            
            let cur_lat = start_lat + (target_lat - start_lat) * t;
            let cur_lon = start_lon + (target_lon - start_lon) * t;
            
            // Height of Ray
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
    radar: Res<Radar>,
) {
    if !radar.enabled {
        return;
    }

    // Convert Geo to World Coords
    // X = Lon * TileSize
    // Z = -Lat * TileSize
    // Y = Alt * HeightScale
    
    let tile_size = 3601.0;
    // Hardcoded height scale from mesh_builder (0.25)
    // ideally this should come from a resource, but for now matching the builder.
    let height_scale = 0.25; 
    
    let x = radar.position.y as f32 * tile_size;
    let z = -(radar.position.x as f32) * tile_size; // Lat is negative Z
    let y = radar.position.z as f32 * height_scale;

    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(100.0))), // Significant size to be seen
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.0, 1.0, 1.0), // Cyan
            emissive: LinearRgba::rgb(0.0, 5.0, 5.0), // Bright glow
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(x, y + 100.0, z), // Lift slightly
    ));
}
