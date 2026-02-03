mod cache;
mod camera;
mod colormap;
mod downloader;
mod lod;
mod mesh_builder;
mod systems;
mod tile;
mod radar;
mod ui;

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "SRTM 3D Tile Viewer".to_string(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::BLACK))
        // Resources
        .init_resource::<cache::TileCache>()
        .init_resource::<colormap::ColorMap>()
        .init_resource::<downloader::TileDownloader>()
        .init_resource::<lod::LodManager>()
        .init_resource::<radar::Radar>()
        // Startup systems
        .add_systems(Startup, (
            setup_scene,
            camera::setup_camera,
            radar::setup_radar_marker,
            ui::setup_ui,
        ))
        // Update systems
        // Update systems
        .add_systems(Update, (
            camera::camera_flight_system,
            lod::update_lod_system,
            systems::tile_loader_system,
            systems::mesh_update_system,
            systems::process_mesh_tasks,
            radar::update_radar_position_system,
            ui::update_mouse_coordinates_system,
        ))
        .add_systems(Update, (
            downloader::process_downloads,
            // mesh_update_system is already above
        ))
        .run();
}

/// Setup the 3D scene with lighting
fn setup_scene(mut commands: Commands) {
    // Directional light (sun) - stronger for better mesh visibility
    commands.spawn((
        DirectionalLight {
            illuminance: 15000.0,
            shadows_enabled: false, // Disable shadows for cleaner mesh view
            ..default()
        },
        Transform::from_xyz(50.0, 100.0, 50.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // No ambient light - black background, only mesh visible

    info!("SRTM Viewer initialized");
    info!("Controls:");
    info!("  WASD / Arrow Keys: Move camera");
    info!("  Q/E or Shift/Space: Move up/down");
    info!("  Right-click + drag: Rotate camera");
    info!("  Mouse wheel: Zoom in/out");
}
