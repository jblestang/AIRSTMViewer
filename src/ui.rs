use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::cache::TileCache;
use crate::tile::TileCoord;

#[derive(Component)]
pub struct MouseCoordinatesText;

pub fn setup_ui(mut commands: Commands) {
    commands.spawn((
        Text::new("Lat: --\nLon: --\nAlt: --"),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(10.0),
            left: Val::Px(10.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        MouseCoordinatesText,
    ));
}

pub fn update_mouse_coordinates_system(
    window_query: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    cache: Res<TileCache>,
    mut text_query: Query<&mut Text, With<MouseCoordinatesText>>,
) {
    let (camera, camera_transform) = camera_query.single();
    let window = window_query.single();
    
    if let Some(cursor_position) = window.cursor_position() {
        if let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) {
             let origin = ray.origin;
             let direction = ray.direction;
             
             // Raymarch to find ground intersection
             // Max distance approx 50km
             let max_dist = 50_000.0;
             let step_size = 50.0; // 50m precision to start
             let num_steps = (max_dist / step_size) as usize;
             
             let tile_size = 3601.0;
             
             for i in 0..num_steps {
                 let dist = i as f32 * step_size;
                 let pos = origin + direction * dist;
                 
                 // Check if point is below terrain
                 // Convert World -> Geo
                 // X = Lon * Size -> Lon = X / Size
                 // Z = - (Lat + 1) * Size -> Lat = -Z/Size - 1 ??
                 // Let's invert the formula from systems.rs:
                 // z_offset = -((coord.lat + 1) as f32) * tile_size;
                 // So Z maps to North-South.
                 // Actually easier: Lat = -pos.z / tile_size. 
                 // Wait. Lat 43. z = -44*3601. Lat 44. z = -45*3601? No.
                 // Lat 43 origin is South-West (43N).
                 // In our Mesh, Z goes from 0 (North/Top) to Size (South/Bottom).
                 // And we offset the tile by z_offset.
                 
                 // Let's use the exact inverse:
                 // Lat = -pos.z / tile_size.  (If pos.z is -158444 then Lat is 44.0).
                 // But Lat 43.7 is in Tile 43.
                 // -43.7 * 3601 = -157363.
                 // So yes: Lat = -pos.z / tile_size.
                 
                 let lat = -pos.z / tile_size;
                 let lon = pos.x / tile_size;
                 
                 // Find tile
                 let coord = TileCoord::from_world_coords(lat as f64, lon as f64);
                 
                 if let Some(crate::tile::TileState::Loaded(data)) = cache.tiles.get(&coord) {
                     // Sample Exact Height
                     let lat_base = coord.lat as f64;
                     let lon_base = coord.lon as f64;
                     let d_lat = lat as f64 - lat_base;
                     let d_lon = lon as f64 - lon_base;
                     
                     let y_pct = 1.0 - d_lat;
                     let x_pct = d_lon;
                     
                     // Boundary check
                     if y_pct >= 0.0 && y_pct <= 1.0 && x_pct >= 0.0 && x_pct <= 1.0 {
                         let px = (x_pct * 3600.0) as usize;
                         let py = (y_pct * 3600.0) as usize;
                         
                         if let Some(h) = data.get_height(px, py) {
                             if pos.y <= h as f32 {
                                 // HIT!
                                 // Refine intersection? (Binary search could be added here)
                                 
                                 // Update Text
                                 for mut text in text_query.iter_mut() {
                                     text.0 = format!(
                                         "Lat: {:.5}\nLon: {:.5}\nAlt: {}m", 
                                         lat, lon, h
                                     );
                                 }
                                 return;
                             }
                         }
                     }
                 }
             }
             
             // No hit
             for mut text in text_query.iter_mut() {
                 text.0 = "Lat: --\nLon: --\nAlt: --".to_string();
             }
        }
    }
}
