// Level of Detail management
use bevy::prelude::*;

/// LOD manager resource
#[derive(Resource)]
pub struct LodManager {
    pub current_level: usize,
    pub zoom_distance: f32,
}

impl Default for LodManager {
    fn default() -> Self {
        Self {
            current_level: 4,
            zoom_distance: 100.0,
        }
    }
}

impl LodManager {
    /// Calculate LOD level based on camera distance/zoom
    /// Calculate LOD level based on camera distance/zoom
    pub fn calculate_lod(&self, camera_distance: f32) -> usize {
        // ALGORITHM: Discrete Level of Detail
        // We select a "step size" (stride) for the mesh grid based on distance.
        // The step size MUST be a divisor of (size-1) i.e. 3600 to ensure the
        // edges of the tile align perfectly with neighbors without T-junctions or gaps.
        // Valid divisors of 3600: 1, 2, 3, 4, 5, 6, 8, 9, 10, 12, 15, 16, 18, 20...
        // 
        // LOD 8  = 3600/8 = 450 grid => 202,500 verts (High)
        // LOD 20 = 3600/20 = 180 grid => 32,400 verts (Medium)
        // LOD 40 = 3600/40 = 90 grid  => 8,100 verts (Low)
        
        // Thresholds based on Tile Size (3600)
        if camera_distance < 5000.0 {
            8 // High detail
        } else if camera_distance < 15000.0 {
            20 // Medium detail
        } else {
            40 // Low detail
        }
    }

    /// Update LOD based on camera position
    pub fn update_from_camera(&mut self, camera_height: f32) {
        let new_level = self.calculate_lod(camera_height);
        if new_level != self.current_level {
            info!("LOD changed: {} -> {}", self.current_level, new_level);
            self.current_level = new_level;
        }
    }
}

/// System to update LOD based on camera
pub fn update_lod_system(
    mut lod_manager: ResMut<LodManager>,
    camera_query: Query<&Transform, With<Camera>>,
) {
    if let Ok(camera_transform) = camera_query.single() {
        let camera_height = camera_transform.translation.y.abs();
        let new_level = lod_manager.calculate_lod(camera_height);
        
        // Only mutate if actually changed to avoid triggering change detection
        if new_level != lod_manager.current_level {
            info!("LOD changed: {} -> {}", lod_manager.current_level, new_level);
            lod_manager.current_level = new_level;
        }
    }
}
