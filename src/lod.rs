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
        // Tile size is approx 3601.0 units.
        // LOD 1 = 13M verts (Too heavy)
        // LOD 2 = 3.2M verts
        // LOD 4 = 800k verts (Reasonable Max)
        // LOD 8 = 200k verts
        // LOD 16 = 50k verts
        
        // Thresholds based on Tile Size (3600)
        if camera_distance < 5000.0 {
            4 // Max detail (approx 1.5 tiles distance)
        } else if camera_distance < 10000.0 {
            8 // Medium detail (approx 3 tiles)
        } else {
            16 // Low detail
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
    if let Ok(camera_transform) = camera_query.get_single() {
        let camera_height = camera_transform.translation.y.abs();
        let new_level = lod_manager.calculate_lod(camera_height);
        
        // Only mutate if actually changed to avoid triggering change detection
        if new_level != lod_manager.current_level {
            info!("LOD changed: {} -> {}", lod_manager.current_level, new_level);
            lod_manager.current_level = new_level;
        }
    }
}
