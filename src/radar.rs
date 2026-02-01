use bevy::prelude::*;
use bevy::math::DVec3;

/// Radar configuration resource
#[derive(Resource, Clone, Debug)]
pub struct Radar {
    pub position: DVec3, // Lat (deg), Lon (deg), Alt (meters)
    pub enabled: bool,
    pub max_range: f32, // Max range in meters
    pub horizon_map: Vec<f32>, // Max elevation angle (radians) for each azimuth (0.1 deg steps)
}

impl Default for Radar {
    fn default() -> Self {
        Self {
            // Mont Agel coordinates
            position: DVec3::new(43.7686, 7.4217, 1148.0), 
            enabled: true,
            max_range: 400_000.0, // 400 km range
            horizon_map: vec![-std::f32::consts::FRAC_PI_2; 3600], // Init with -90 deg
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
        // Since range is small compared to Earth, flat approximation or spherical is fine.
        // Let's use Haversine for accuracy.
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

        // Check Horizon Map (Radial Sweep)
        // Calculate Azimuth (bearing) from Radar to Target
        let y = d_lon.sin() * lat2.cos();
        let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * d_lon.cos();
        let azimuth = y.atan2(x).to_degrees(); // -180 to +180
        
        // Map azimuth to 0..3600 index
        let azimuth_normalized = if azimuth < 0.0 { azimuth + 360.0 } else { azimuth };
        let index = (azimuth_normalized * 10.0).round() as usize % 3600;
        
        let horizon_angle = self.horizon_map[index];
        
        // Calculate Target Elevation Angle
        // Angle = atan2(TargetHeight - RadarHeight - Drop, Dist)
        // Drop = dist^2 / (2 * R_eff)
        // Actually, let's use the explicit height diff logic
        
        let drop = (dist * dist) / (2.0 * R_EFF);
        let target_relative_h = (target_alt as f64 - self.position.z) - drop;
        
        let target_angle = target_relative_h.atan2(dist) as f32;
        
        target_angle >= horizon_angle
    }

    /// Calculate visibility with terrain occlusion (Raycasting) - Legacy/Slow
    pub fn is_visible_raycast(&self, target_lat: f64, target_lon: f64, target_alt: f32, cache: &crate::cache::TileCache) -> bool {
        // Just forward to the optimized map check! 
        // The map is updated by the system.
        self.is_visible(target_lat, target_lon, target_alt)
    }
}

/// System to update the Radar Horizon Map (Radial Sweep)
pub fn update_radar_viewshed(
    mut radar: ResMut<Radar>,
    cache: Res<crate::cache::TileCache>,
    time: Res<Time>,
    mut timer: Local<f32>,
) {
    if !radar.enabled {
        return;
    }
    
    // Throttle updates: Run once every 1.0 second
    *timer += time.delta_secs();
    if *timer < 1.0 {
        return;
    }
    *timer = 0.0;
    
    // Only update if cache has changed? 
    // For now, let's run it every frame or throttle it?
    // It takes some time. Let's rely on Rayon.
    
    use rayon::prelude::*;
    
    // Create a temporary buffer for results
    let new_horizon: Vec<f32> = (0..3600).into_par_iter().map(|i| {
        let azimuth = (i as f64) / 10.0;
        let azimuth_rad = azimuth.to_radians();
        
        // Raymarch outbound
        const R_EARTH: f64 = 6_371_000.0;
        const R_EFF: f64 = R_EARTH * (4.0/3.0);
        let max_range = radar.max_range as f64;
        
        let start_lat = radar.position.x.to_radians();
        let start_lon = radar.position.y.to_radians();
        
        let sin_start_lat = start_lat.sin();
        let cos_start_lat = start_lat.cos();
        
        let mut max_angle = -std::f32::consts::FRAC_PI_2;
        
        // Step size: 100 meters? Matches grid resolution approx.
        let step_size = 100.0;
        let num_steps = (max_range / step_size) as usize;
        
        // Optimization: start a bit away from radar to avoid self-occlusion artifacts if radar is on ground
        let start_step = 10; 
        
        for s in start_step..num_steps {
            let dist = s as f64 * step_size;
            
            // Calculate lat/lon at distance 'dist' and azimuth 'azimuth_rad'
            // Destination point given distance and bearing from start point
            let ang_dist = dist / R_EARTH; // Angular distance on sphere
            
            let sin_ang_dist = ang_dist.sin();
            let cos_ang_dist = ang_dist.cos();
            
            let lat2 = (sin_start_lat * cos_ang_dist + cos_start_lat * sin_ang_dist * azimuth_rad.cos()).asin();
            let lon2 = start_lon + (azimuth_rad.sin() * sin_ang_dist * cos_start_lat).atan2(cos_ang_dist - sin_start_lat * lat2.sin());
            
            let lat_deg = lat2.to_degrees();
            let lon_deg = lon2.to_degrees();
            
            if let Some(h) = cache.get_height_global(lat_deg, lon_deg) {
                 // Calculate elevation angle
                 let drop = (dist * dist) / (2.0 * R_EFF);
                 let h_relative = (h as f64 - radar.position.z) - drop;
                 let angle = h_relative.atan2(dist) as f32;
                 
                 if angle > max_angle {
                     max_angle = angle;
                 }
            }
        }
        
        // Initial horizon (geometric horizon if flat ocean)
        // distance to horizon d = sqrt(2*h*R_eff).
        // angle = -acos(R_eff / (R_eff + h)). Approx -sqrt(2h/R).
        // Actually, if we see nothing, the horizon is the geometric limits.
        // But let's stick to terrain max.
        
        max_angle
    }).collect();
    
    radar.horizon_map = new_horizon;
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
        Mesh3d(meshes.add(Sphere::new(500.0))), // Significant size to be seen
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.0, 1.0, 1.0), // Cyan
            emissive: LinearRgba::rgb(0.0, 5.0, 5.0), // Bright glow
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(x, y + 500.0, z), // Lift slightly
    ));
}
