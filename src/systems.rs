// Systems for coordinating tile loading and mesh updates
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures_lite::future;
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

/// Component for tracking background mesh generation tasks
#[derive(Component)]
pub struct MeshGenTask {
    task: Task<Mesh>,
    coord: TileCoord,
}

/// System to determine visible tiles and request loading
pub fn tile_loader_system(
    camera_query: Query<&Transform, With<Camera>>,
    mut cache: ResMut<TileCache>,
    downloader: Res<TileDownloader>,
) {
    let Ok(camera_transform) = camera_query.single() else {
        return;
    };

    // Calculate which tile the camera is over
    let cam_pos = camera_transform.translation;
    
    // Calculate tile coordinate from camera position
    // COORDINATE MAPPING:
    // World space Z corresponds to negative Latitude (North is negative Z).
    // The SRTM tile naming convention (e.g., N43) refers to the bottom-left corner.
    // However, our world space origin 0,0 is N0E0.
    // So: Lat_idx = ceil(-Z / 3601) - 1.
    let tile_size = 3601.0;
    let lat_idx = (-cam_pos.z / tile_size).ceil() as i32 - 1;
    let center_coord = TileCoord::new(
        lat_idx,
        (cam_pos.x / tile_size).floor() as i32,
    );

    // Calculate visible range based on camera height and viewing distance
    // Higher camera = more tiles visible
    // Higher camera = more tiles visible
    // Scale: 1 tile radius per 1000m height, up to max 20 tiles (approx 2000km view)
    let view_distance = (cam_pos.y / 1000.0).max(1.0).min(20.0); 
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
                    error!("Failed to load tile from disk ({}): {}", coord.filename(), e);
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

/// System to queue mesh generation tasks
pub fn mesh_update_system(
    mut commands: Commands,
    cache: Res<TileCache>,
    colormap: Res<ColorMap>,
    lod_manager: Res<LodManager>,
    tile_query: Query<(Entity, &TerrainTile)>,
    task_query: Query<&MeshGenTask>,
    radars: Res<crate::radar::Radars>,
    regen_query: Query<Entity, With<NeedsRegen>>,
    camera_query: Query<&Transform, With<Camera>>,
) {
    // Check if LOD changed globaly - if so, mark all tiles for regeneration
    // Note: With per-tile LOD, we might not need global triggers as much, 
    // but useful if user manually changes settings.
    if lod_manager.is_changed() {
        for (entity, _) in tile_query.iter() {
            commands.entity(entity).insert(NeedsRegen);
        }
    }
    
    // Regenerate meshes (remove existing, trigger new task)
    for entity in regen_query.iter() {
         commands.entity(entity).despawn(); 
    }

    let Ok(camera_transform) = camera_query.single() else {
        return;
    };
    let camera_pos = camera_transform.translation;

    // Prepare snapshot of cache for background threads (Lazy)
    let mut snapshot: Option<std::sync::Arc<std::collections::HashMap<TileCoord, std::sync::Arc<crate::tile::TileData>>>> = None;
    
    // Throttle: Only spawn a limited number of tasks per frame to keep UI responsive
    let mut tasks_spawned = 0;
    const MAX_TASKS_PER_FRAME: usize = 2;

    // Iterate loaded tiles and check if we need to spawn a task
    for (coord, tile_state) in cache.tiles.iter() {
        if let TileState::Loaded(data_arc) = tile_state {
            // Check if entity already exists
            // Optimization: We could store entities in a map for faster lookup, but iteration is okay for <100 tiles
            let exists = tile_query.iter().any(|(_, tile)| tile.coord == *coord);
            let pending = task_query.iter().any(|t| t.coord == *coord);
            
            if !exists && !pending {
                // Throttle check
                if tasks_spawned >= MAX_TASKS_PER_FRAME {
                    break; 
                }

                // Lazy Snapshot Creation
                if snapshot.is_none() {
                     snapshot = Some(std::sync::Arc::new(cache.get_snapshot()));
                }

                // Calculate Distance-based LOD
                let tile_size = 3601.0;
                // Center of tile in world space
                // x = (lon + 0.5) * size
                // z = -(lat + 0.5) * size
                let center_x = (coord.lon as f32 + 0.5) * tile_size;
                let center_z = -((coord.lat as f32 + 0.5) * tile_size);
                let tile_center = Vec3::new(center_x, 0.0, center_z);
                
                let distance = camera_pos.distance(tile_center);
                let lod_level = lod_manager.calculate_lod(distance);

                // ALGORITHM: Frustum Culling (Approximate)
                // Instead of full AABB frustum checks, we use a simple Dot Product check.
                // 1. Calculate vector from Camera to Tile Center.
                // 2. Calculate Camera Forward vector.
                // 3. Dot Product > Threshold implies the tile is roughly "in front" of the camera.
                // Threshold 0.2 approx corresponds to a wide FOV (allowing peripherals to load).
                let cam_forward = camera_transform.forward();
                let dir_to_tile = (tile_center - camera_pos).normalize_or_zero();
                
                let is_visible = cam_forward.dot(dir_to_tile) > 0.2;

                // Exception: Always generate very close tiles regardless of direction (for rotating)
                // Exception: Always generate very close tiles regardless of direction (for rotating)
                let is_close = distance < 20000.0; // Increased to 20km for better rotation feel
                
                if !is_visible && !is_close {
                    continue;
                }

                // Spawn Mesh Generation Task
                let thread_pool = AsyncComputeTaskPool::get();
                

                let coord = *coord;
                let data = data_arc.clone();
                let colormap = colormap.clone();
                let radars = radars.clone();
                let cache_snapshot = snapshot.as_ref().unwrap().clone();
                
                let task = thread_pool.spawn(async move {
                    let builder = TerrainMeshBuilder::new(lod_level);
                    builder.build_mesh(&data, &colormap, Some(&radars), Some(cache_snapshot.as_ref()))
                });

                commands.spawn(MeshGenTask { task, coord });
                tasks_spawned += 1;
                
                info!("Queued mesh generation for {:?} (LOD {}, Dist {:.0})", coord, lod_level, distance);
            }
        }
    }

    // Handle missing tiles (placeholders)
    for (coord, state) in cache.tiles.iter() {
        if matches!(state, TileState::Missing) {
            let exists = tile_query.iter().any(|(_, tile)| tile.coord == *coord);
            if !exists {
                 // For now, continue to spawn missing tiles on main thread (simple)
                 // Or we could adapt spawn_missing_tile to return a Mesh and do it here?
                 // Let's defer implementation of spawn_missing_tile or assume it exists
            }
        }
    }
}

/// Spawn a tile entity with mesh
fn spawn_tile_entity(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    colormap: &ColorMap,
    lod_manager: &LodManager,
    radars: Option<&crate::radar::Radars>,
    cache: Option<&TileCache>,
    coord: TileCoord,
    tile_data: Option<&crate::tile::TileData>,
) {
    let builder = TerrainMeshBuilder::new(lod_manager.current_level);
    
    // Create a snapshot of the cache for parallel access
    // This avoids accessing the Res<TileCache> from multiple threads
    let snapshot = cache.map(|c| c.get_snapshot());
    
    let mesh = if let Some(data) = tile_data {
        builder.build_mesh(data, colormap, radars, snapshot.as_ref())
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

/// System to poll mesh tasks and propagate results
pub fn process_mesh_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut MeshGenTask)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, mut mesh_task) in &mut tasks {
        if let Some(mesh) = future::block_on(future::poll_once(&mut mesh_task.task)) {
            // Task finished, spawn the real entity
            let coord = mesh_task.coord;
            
            // Calculate transform
            let tile_size = 3601.0;
            let x_offset = coord.lon as f32 * tile_size;
            let z_offset = -((coord.lat + 1) as f32) * tile_size;

            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 0.8,
                    metallic: 0.0,
                    cull_mode: None,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })),
                Transform::from_xyz(x_offset, 0.0, z_offset),
                TerrainTile { coord },
            ));

            // Remove the task entity
            commands.entity(entity).despawn();
            
            info!("Finished mesh generation for {:?}", coord);
        }
    }
}
