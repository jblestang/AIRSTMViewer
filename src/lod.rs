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
    pub fn calculate_lod(&self, camera_distance: f32) -> usize {
        // Closer = higher detail (lower LOD value)
        // Further = lower detail (higher LOD value)
        
        if camera_distance < 50.0 {
            1 // Full resolution
        } else if camera_distance < 100.0 {
            2 // Half resolution
        } else if camera_distance < 200.0 {
            4 // Quarter resolution
        } else if camera_distance < 400.0 {
            8
        } else {
            16 // Very low resolution for distant views
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
