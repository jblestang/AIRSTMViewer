mod cache;
mod camera;
mod colormap;
mod downloader;
mod lod;
mod mesh_builder;
mod mesh_cache;
mod systems;
mod tile;
mod radar;
mod ui;
mod geojson;
mod radar_panel;
mod terrain;
mod radial_profile;

use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "SRTM 3D Tile Viewer".to_string(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }),
            EguiPlugin::default(),
        ))
        .insert_resource(ClearColor(Color::BLACK))
        // Resources
        .init_resource::<cache::TileCache>()
        .init_resource::<mesh_cache::MeshCache>()
        .init_resource::<colormap::ColorMap>()
        .insert_resource(downloader::TileDownloader::new())
        .init_resource::<lod::LodManager>()
        .init_resource::<radar::Radars>()
        .init_resource::<radar_panel::RadarEditor>()
        .init_resource::<terrain::CursorTerrain>()
        .init_resource::<radial_profile::RadialProfileUi>()
        // Startup systems
        .add_systems(Startup, (
            setup_scene,
            camera::setup_camera,
            (geojson::load_sam_sites_system, radar::setup_radar_marker).chain(),
            ui::setup_ui,
            ui::setup_activity_panel,
        ))
        // Update systems
        .add_systems(Update, (
            camera::camera_flight_system,
            (
                crate::systems::tile_loader_system,
                crate::systems::process_tile_loads,
                downloader::process_downloads,
                crate::systems::mesh_update_system,
                crate::systems::process_mesh_tasks,
            ).chain(),
            crate::radar::update_radar_position_system,
            radar_panel::sync_radar_editor_system,
            radar_panel::radar_panel_keyboard_system,
            radial_profile::radial_profile_keyboard_system,
            radial_profile::update_cursor_terrain_system,
            ui::update_mouse_coordinates_system,
            ui::update_activity_panel,
            // crate::systems::cache_eviction_system, // Reverted
        ))
        .add_systems(EguiPrimaryContextPass, (
            radar_panel::setup_egui_style,
            radar_panel::radar_panel_ui_system,
            radial_profile::radial_profile_ui_system,
        ).chain())
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
    info!("  WASD: Move Forward/Back/Left/Right");
    info!("  Arrows: Move Up/Down (Altitude) and Strafe Left/Right");
    info!("  Shift + Arrows: Rotate Camera (Look)");
    info!("  Right-click + drag: Rotate Camera");
    info!("  U: Toggle radar parameter panel (validate to recalculate coverage)");
    info!("  V: Toggle radial terrain cross-section (500 km)");
    info!("  Mouse wheel: Zoom / Move Forward");
}
