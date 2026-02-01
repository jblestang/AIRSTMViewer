// SRTM Tile coordinate and data structures
use serde::{Deserialize, Serialize};

/// Represents a tile coordinate in the SRTM grid
/// SRTM tiles are 1° x 1° and named like N37W122
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileCoord {
    pub lat: i32,  // Latitude (south is negative)
    pub lon: i32,  // Longitude (west is negative)
}

impl TileCoord {
    pub fn new(lat: i32, lon: i32) -> Self {
        Self { lat, lon }
    }

    /// Convert world coordinates (lat, lon in degrees) to tile coordinate
    pub fn from_world_coords(lat: f64, lon: f64) -> Self {
        Self {
            lat: lat.floor() as i32,
            lon: lon.floor() as i32,
        }
    }

    /// Get the filename for this tile (e.g., "N37W122.hgt")
    pub fn filename(&self) -> String {
        let lat_prefix = if self.lat >= 0 { 'N' } else { 'S' };
        let lon_prefix = if self.lon >= 0 { 'E' } else { 'W' };
        format!(
            "{}{:02}{}{:03}.hgt",
            lat_prefix,
            self.lat.abs(),
            lon_prefix,
            self.lon.abs()
        )
    }

    /// Get neighboring tiles (8 surrounding tiles)
    pub fn neighbors(&self) -> Vec<TileCoord> {
        let mut neighbors = Vec::new();
        for dlat in -1..=1 {
            for dlon in -1..=1 {
                if dlat == 0 && dlon == 0 {
                    continue;
                }
                neighbors.push(TileCoord::new(self.lat + dlat, self.lon + dlon));
            }
        }
        neighbors
    }
}

/// State of a tile in the system
#[derive(Debug, Clone, PartialEq)]
pub enum TileState {
    /// Tile is being downloaded
    Loading,
    /// Tile data is loaded and ready
    Loaded(TileData),
    /// Tile failed to load (404 or other error)
    Missing,
    /// Error occurred during loading
    Error(String),
}

/// SRTM tile elevation data
/// Standard SRTM 1 arc-second tiles are 3601x3601 samples
#[derive(Debug, Clone, PartialEq)]
pub struct TileData {
    pub coord: TileCoord,
    pub size: usize,  // Grid size (typically 3601 for SRTM1)
    pub heights: Vec<i16>,  // Height data in meters (row-major order)
}

impl TileData {
    /// Create a new tile with given size
    pub fn new(coord: TileCoord, size: usize) -> Self {
        Self {
            coord,
            size,
            heights: vec![0; size * size],
        }
    }

    /// Get height at grid position (x, y)
    pub fn get_height(&self, x: usize, y: usize) -> Option<i16> {
        if x < self.size && y < self.size {
            Some(self.heights[y * self.size + x])
        } else {
            None
        }
    }

    /// Set height at grid position (x, y)
    pub fn set_height(&mut self, x: usize, y: usize, height: i16) {
        if x < self.size && y < self.size {
            self.heights[y * self.size + x] = height;
        }
    }

    /// Get interpolated height at normalized position (0.0 to 1.0)
    pub fn get_height_normalized(&self, nx: f32, ny: f32) -> f32 {
        let x = (nx * (self.size - 1) as f32).clamp(0.0, (self.size - 1) as f32);
        let y = (ny * (self.size - 1) as f32).clamp(0.0, (self.size - 1) as f32);
        
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(self.size - 1);
        let y1 = (y0 + 1).min(self.size - 1);
        
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;
        
        // Bilinear interpolation
        let h00 = self.get_height(x0, y0).unwrap_or(0) as f32;
        let h10 = self.get_height(x1, y0).unwrap_or(0) as f32;
        let h01 = self.get_height(x0, y1).unwrap_or(0) as f32;
        let h11 = self.get_height(x1, y1).unwrap_or(0) as f32;
        
        let h0 = h00 * (1.0 - fx) + h10 * fx;
        let h1 = h01 * (1.0 - fx) + h11 * fx;
        
        h0 * (1.0 - fy) + h1 * fy
    }

    /// Get min and max heights in the tile
    pub fn height_range(&self) -> (i16, i16) {
        let mut min = i16::MAX;
        let mut max = i16::MIN;
        for &h in &self.heights {
            min = min.min(h);
            max = max.max(h);
        }
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_coord_filename() {
        assert_eq!(TileCoord::new(37, -122).filename(), "N37W122.hgt");
        assert_eq!(TileCoord::new(-33, 151).filename(), "S33E151.hgt");
        assert_eq!(TileCoord::new(0, 0).filename(), "N00E000.hgt");
    }

    #[test]
    fn test_from_world_coords() {
        assert_eq!(TileCoord::from_world_coords(37.7749, -122.4194), TileCoord::new(37, -123));
        assert_eq!(TileCoord::from_world_coords(-33.8688, 151.2093), TileCoord::new(-34, 151));
    }

    #[test]
    fn test_neighbors() {
        let coord = TileCoord::new(0, 0);
        let neighbors = coord.neighbors();
        assert_eq!(neighbors.len(), 8);
        assert!(neighbors.contains(&TileCoord::new(-1, -1)));
        assert!(neighbors.contains(&TileCoord::new(1, 1)));
    }
}
