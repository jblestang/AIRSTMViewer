// Tile cache management
use crate::tile::{TileCoord, TileData, TileState};
use std::collections::HashMap;
use std::path::PathBuf;
use bevy::prelude::*;

/// Resource managing the tile cache
#[derive(Resource)]
pub struct TileCache {
    pub tiles: HashMap<TileCoord, TileState>,
    cache_dir: PathBuf,
}

impl TileCache {
    /// Create a new tile cache
    pub fn new() -> Self {
        let cache_dir = Self::get_cache_dir();
        
        // Create cache directory if it doesn't exist
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir)
                .expect("Failed to create cache directory");
        }
        
        Self {
            tiles: HashMap::new(),
            cache_dir,
        }
    }

    /// Get the cache directory path
    fn get_cache_dir() -> PathBuf {
        // Use local "assets" directory in the project
        let current_dir = std::env::current_dir()
            .expect("Could not determine current directory");
        current_dir.join("assets")
    }

    /// Get the file path for a tile in the cache
    pub fn get_tile_path(&self, coord: &TileCoord) -> PathBuf {
        self.cache_dir.join(coord.filename())
    }

    /// Check if a tile exists in the cache
    pub fn has_tile(&self, coord: &TileCoord) -> bool {
        self.tiles.contains_key(coord)
    }

    /// Get tile state
    pub fn get_tile(&self, coord: &TileCoord) -> Option<&TileState> {
        self.tiles.get(coord)
    }

    /// Insert or update a tile
    pub fn insert_tile(&mut self, coord: TileCoord, state: TileState) {
        self.tiles.insert(coord, state);
    }
    
    /// Insert loaded tile data (helper)
    pub fn insert_data(&mut self, coord: TileCoord, data: TileData) {
        self.tiles.insert(coord, TileState::Loaded(std::sync::Arc::new(data)));
    }

    /// Mark a tile as loading
    pub fn mark_loading(&mut self, coord: TileCoord) {
        self.tiles.insert(coord, TileState::Loading);
    }

    /// Check if tile file exists on disk
    pub fn is_cached_on_disk(&self, coord: &TileCoord) -> bool {
        self.get_tile_path(coord).exists()
    }

    /// Load tile from disk cache
    pub fn load_from_disk(&self, coord: &TileCoord) -> Result<TileData, String> {
        let path = self.get_tile_path(coord);
        
        if !path.exists() {
            return Err(format!("Tile file not found: {:?}", path));
        }

        let data = std::fs::read(&path)
            .map_err(|e| format!("Failed to read tile file: {}", e))?;

        // SRTM files are raw binary, big-endian i16 values
        // SRTM1 (1 arc-second) is 3601x3601 = 12,967,201 samples = 25,934,402 bytes
        let expected_size = 3601 * 3601 * 2;
        
        if data.len() != expected_size {
            return Err(format!(
                "Invalid tile size: expected {} bytes, got {}",
                expected_size,
                data.len()
            ));
        }

        let mut tile = TileData::new(*coord, 3601);
        
        // Parse big-endian i16 values
        // SRTM file format specification:
        // - Rows are ordered NORTH to SOUTH (first row = northernmost)
        // - Columns are ordered WEST to EAST (first column = westernmost)
        // - Filename indicates the LOWER-LEFT (southwest) corner
        // - In our coordinate system, we need to flip Y-axis only
        use byteorder::{BigEndian, ReadBytesExt};
        use std::io::Cursor;
        
        let mut cursor = Cursor::new(data);
        for y in 0..tile.size {
            for x in 0..tile.size {
                tile.heights[y * tile.size + x] = cursor
                    .read_i16::<BigEndian>()
                    .map_err(|e| format!("Failed to parse height data: {}", e))?;
            }
        }

        Ok(tile)
    }

    /// Save tile to disk cache
    pub fn save_to_disk(&self, tile: &TileData) -> Result<(), String> {
        let path = self.get_tile_path(&tile.coord);
        
        use byteorder::{BigEndian, WriteBytesExt};
        use std::io::Cursor;
        
        let mut buffer = Cursor::new(Vec::new());
        
        for &height in &tile.heights {
            buffer
                .write_i16::<BigEndian>(height)
                .map_err(|e| format!("Failed to write height data: {}", e))?;
        }
        
        std::fs::write(&path, buffer.into_inner())
            .map_err(|e| format!("Failed to write tile file: {}", e))?;
        
        Ok(())
    }

    /// Get all loaded tiles
    pub fn loaded_tiles(&self) -> Vec<(TileCoord, &TileData)> {
        self.tiles
            .iter()
            .filter_map(|(coord, state)| {
                if let TileState::Loaded(data) = state {
                    Some((*coord, data.as_ref()))
                } else {
                    None
                }
            })
            .collect()
    }
    
    /// Get snapshot of all loaded tiles (cheap Arc clone)
    pub fn get_snapshot(&self) -> HashMap<TileCoord, std::sync::Arc<TileData>> {
         self.tiles
            .iter()
            .filter_map(|(coord, state)| {
                if let TileState::Loaded(data) = state {
                    Some((*coord, data.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get height at any global coordinate (lat/lon)
    /// Returns None if tile is not loaded or out of bounds
    pub fn get_height_global(&self, lat: f64, lon: f64) -> Option<f32> {
        let coord = TileCoord::from_world_coords(lat, lon);
        
        if let Some(TileState::Loaded(data)) = self.tiles.get(&coord) {
            // Calculate normalized position within tile
            // Tile origin (lat, lon) is lower-left (South-West) usually?
            // Wait, TileCoord::from_world_coords flan down.
            // Example: Lat 43.5 -> Tile 43. 
            // Offset = 0.5.
            
            // Standard SRTM: 
            // Rows 0..3600 (North to South).
            // Cols 0..3600 (West to East).
            
            // Local lat offset from top: (lat_base + 1) - lat
            let lat_base = coord.lat as f64;
            let lon_base = coord.lon as f64;
            
            let d_lat = lat - lat_base; // 0.0 to 1.0 (South to North)
            let d_lon = lon - lon_base; // 0.0 to 1.0 (West to East)
            
            // SRTM Image Y: 0 is North. 1.0 is South.
            // So ny = 1.0 - d_lat
            let ny = 1.0 - d_lat;
            let nx = d_lon;
            
            if nx >= 0.0 && nx <= 1.0 && ny >= 0.0 && ny <= 1.0 {
                return Some(data.get_height_normalized(nx as f32, ny as f32));
            }
        }
        None
    }

    /// Clear all tiles from memory (keeps disk cache)
    pub fn clear_memory(&mut self) {
        self.tiles.clear();
    }
}

impl Default for TileCache {
    fn default() -> Self {
        Self::new()
    }
}
