// Systems for coordinating tile loading and mesh updates
use bevy::prelude::*;
use crate::cache::TileCache;
use crate::colormap::ColorMap;
use crate::downloader::TileDownloader;
use crate::lod::LodManager;
use crate::mesh_builder::TerrainMeshBuilder;
use crate::tile::{TileCoord, TileState};

/// Component marking a terrain tile entity
#[derive(Component)]
pub struct TerrainTile {
    pub coord: TileCoord,
}

/// Marker for tiles that need mesh regeneration
#[derive(Component)]
pub struct NeedsRegen;

/// System to determine visible tiles and request loading
pub fn tile_loader_system(
    camera_query: Query<&Transform, With<Camera>>,
    mut cache: ResMut<TileCache>,
    downloader: Res<TileDownloader>,
) {
    let Ok(camera_transform) = camera_query.get_single() else {
        return;
    };

    // Calculate which tile the camera is over
    let cam_pos = camera_transform.translation;
    
    // Calculate tile coordinate from camera position
    // North = -Z. So Z = -(lat+1) * size.
    // lat+1 = -Z/size. lat = -Z/size - 1.
    let tile_size = 3601.0;
    let lat_idx = (-cam_pos.z / tile_size).ceil() as i32 - 1;
    let center_coord = TileCoord::new(
        lat_idx,
        (cam_pos.x / tile_size).floor() as i32,
    );

    // Calculate visible range based on camera height and viewing distance
    // Higher camera = more tiles visible
    let view_distance = (cam_pos.y / 1000.0).max(3.0).min(20.0); // 3-20 tiles in each direction
    let tile_radius = view_distance.ceil() as i32;
    
    // Load all tiles within viewing distance
    let mut tiles_to_load = Vec::new();
    for dlat in -tile_radius..=tile_radius {
        for dlon in -tile_radius..=tile_radius {
            tiles_to_load.push(TileCoord::new(
                center_coord.lat + dlat,
                center_coord.lon + dlon,
            ));
        }
    }

    for coord in tiles_to_load {
        // Skip if already loaded or loading
        if cache.has_tile(&coord) {
            continue;
        }

        // Check disk cache first
        if cache.as_ref().is_cached_on_disk(&coord) {
            match cache.as_ref().load_from_disk(&coord) {
                Ok(tile_data) => {
                    info!("Loaded tile from disk cache: {:?}", coord);
                    cache.insert_tile(coord, TileState::Loaded(std::sync::Arc::new(tile_data)));
                }
                Err(e) => {
                    error!("Failed to load tile from disk: {}", e);
                    cache.insert_tile(coord, TileState::Error(e));
                }
            }
        } else {
            // Request download
            cache.mark_loading(coord);
            downloader.request_download(coord);
            info!("Requesting download for tile: {:?}", coord);
        }
    }
}

/// System to create/update meshes for loaded tiles
pub fn mesh_update_system(
    mut commands: Commands,
    cache: Res<TileCache>,
    colormap: Res<ColorMap>,
    lod_manager: Res<LodManager>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    tile_query: Query<(Entity, &TerrainTile)>,
    regen_query: Query<Entity, With<NeedsRegen>>,
    radar: Res<crate::radar::Radar>,
) {
    // Check if LOD changed - if so, mark all tiles for regeneration
    if lod_manager.is_changed() {
        for (entity, _) in tile_query.iter() {
            commands.entity(entity).insert(NeedsRegen);
        }
    }

    // Create meshes for newly loaded tiles
    for (coord, tile_data) in cache.loaded_tiles() {
        // Check if entity already exists
        let exists = tile_query.iter().any(|(_, tile)| tile.coord == coord);
        
        if !exists {
            spawn_tile_entity(
                &mut commands,
                &mut meshes,
                &mut materials,
                &colormap,
                &lod_manager,
                Some(&radar),
                Some(&cache), // Pass cache
                coord,
                Some(tile_data),
            );
        }
    }

    // Create placeholder meshes for missing tiles
    for (coord, state) in cache.tiles.iter() {
        if matches!(state, TileState::Missing) {
            let exists = tile_query.iter().any(|(_, tile)| tile.coord == *coord);
            
            if !exists {
                spawn_tile_entity(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &colormap,
                    &lod_manager,
                    None,
                    None,
                    *coord,
                    None,
                );
            }
        }
    }

    // Regenerate meshes for tiles marked for regeneration
    for entity in regen_query.iter() {
        // For now, just remove the marker
        // In a full implementation, you'd regenerate the mesh
        commands.entity(entity).remove::<NeedsRegen>();
    }
}

/// Spawn a tile entity with mesh
fn spawn_tile_entity(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    colormap: &ColorMap,
    lod_manager: &LodManager,
    radar: Option<&crate::radar::Radar>,
    cache: Option<&TileCache>,
    coord: TileCoord,
    tile_data: Option<&crate::tile::TileData>,
) {
    let builder = TerrainMeshBuilder::new(lod_manager.current_level);
    
    // Create a snapshot of the cache for parallel access
    // This avoids accessing the Res<TileCache> from multiple threads
    let snapshot = cache.map(|c| c.get_snapshot());
    
    let mesh = if let Some(data) = tile_data {
        builder.build_mesh(data, colormap, radar, snapshot.as_ref())
    } else {
        builder.build_missing_mesh()
    };

    // Position the tile in world space
    // Coordinate System:
    // X = Longitude (East+)
    // Z = Latitude (North is -Z, South is +Z)
    // SRTM Tile Origin is South-West corner (lat, lon)
    // Mesh generates pz=0 (North edge) to pz=size (South edge)
    // So we need to place the tile origin at -(lat + 1)
    let tile_size = 3601.0;
    let x_offset = coord.lon as f32 * tile_size;
    let z_offset = -((coord.lat + 1) as f32) * tile_size;
    


    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.8,
            metallic: 0.0,
            cull_mode: None,  // Disable backface culling - normals point inward
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_xyz(x_offset, 0.0, z_offset),
        TerrainTile { coord },
    ));
    
    


    info!("Spawned tile entity: {:?}", coord);
}
