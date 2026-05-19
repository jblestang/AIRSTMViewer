// SRTM Tile coordinate and data structures
use serde::{Deserialize, Serialize};

/// Represents a tile coordinate in the SRTM grid
/// SRTM tiles are 1° x 1° and named like N37W122
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileCoord {
    pub lat: i32,  // Latitude (south is negative)
    pub lon: i32,  // Longitude (west is negative)
}

#[allow(dead_code)]
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

use std::sync::Arc;

/// State of a tile in the system
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum TileState {
    /// Tile is being downloaded
    Loading,
    /// Tile data is loaded and ready
    Loaded(Arc<TileData>),
    /// Tile data is loaded and ready (non-Arc version for intermediate?)
    // Loaded(TileData), 
    /// Tile failed to load (404 or other error)
    Missing,
    /// Error occurred during loading
    Error(String),
}

/// SRTM tile elevation data
/// Standard SRTM 1 arc-second tiles are 3601x3601 samples
/// DATA FORMAT:
/// - 16-bit signed integers (i16)
/// - Big-endian byte order
/// - Height in meters relative to WGS84 EGM96 geoid
/// - Void data (unknown) is typically -32768
pub const SRTM_VOID: i16 = -32768;

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
    #[allow(dead_code)]
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
        
        // ALGORITHM: Bilinear Interpolation
        // We calculate the exact height at a sub-pixel position (nx, ny)
        // by weighting the 4 surrounding pixels.
        // Formula: f(x,y) = f(0,0)(1-x)(1-y) + f(1,0)x(1-y) + f(0,1)(1-x)y + f(1,1)xy
        let h00 = self.get_height(x0, y0).unwrap_or(0) as f32;
        let h10 = self.get_height(x1, y0).unwrap_or(0) as f32;
        let h01 = self.get_height(x0, y1).unwrap_or(0) as f32;
        let h11 = self.get_height(x1, y1).unwrap_or(0) as f32;
        
        // Linear interpolate X (Top and Bottom rows)
        let h0 = h00 * (1.0 - fx) + h10 * fx;
        let h1 = h01 * (1.0 - fx) + h11 * fx;
        
        // Linear interpolate Y
        h0 * (1.0 - fy) + h1 * fy
    }

    /// Inclusive DEM index range for one LOD vertex bin (integer math only, no gaps/overlap).
    #[inline]
    pub fn lod_dem_bin(
        &self,
        xi: usize,
        yi: usize,
        step: usize,
        vertices_per_row: usize,
    ) -> (usize, usize, usize, usize) {
        let step = step.max(1);
        let max_coord = self.size.saturating_sub(1);

        let x_start = xi.saturating_mul(step);
        let y_start = yi.saturating_mul(step);

        // x_end = x_start + step - 1 for interior bins; last bin absorbs remainder pixels.
        let x_end = if xi + 1 < vertices_per_row {
            x_start.saturating_add(step - 1)
        } else {
            max_coord
        };
        let y_end = if yi + 1 < vertices_per_row {
            y_start.saturating_add(step - 1)
        } else {
            max_coord
        };

        (
            x_start.min(max_coord),
            x_end.min(max_coord),
            y_start.min(max_coord),
            y_end.min(max_coord),
        )
    }

    /// LOD grid width in vertices for a given stride (covers all DEM indices 0..=size-1).
    #[inline]
    pub fn lod_vertices_per_row(step: usize, size: usize) -> usize {
        let step = step.max(1);
        let max_coord = size.saturating_sub(1);
        if step == 1 {
            max_coord + 1
        } else {
            max_coord / step + 1
        }
    }

    /// Map a DEM row/column index to WGS84 offset inside the tile (0.0 = west / north edge).
    #[inline]
    pub fn dem_index_to_lon_frac(x: usize, max_coord: usize) -> f64 {
        if max_coord == 0 {
            return 0.0;
        }
        x as f64 / max_coord as f64
    }

    #[inline]
    pub fn dem_index_to_lat_frac(y: usize, max_coord: usize) -> f64 {
        if max_coord == 0 {
            return 0.0;
        }
        // DEM y=0 is north; lat fraction increases southward.
        1.0 - (y as f64 / max_coord as f64)
    }

    /// Maximum height in an inclusive DEM index rectangle (void cells ignored).
    pub fn max_height_in_region(
        &self,
        x_start: usize,
        y_start: usize,
        x_end: usize,
        y_end: usize,
    ) -> Option<i16> {
        if self.size == 0 {
            return None;
        }
        let max_idx = self.size - 1;
        let x_start = x_start.min(max_idx);
        let y_start = y_start.min(max_idx);
        let x_end = x_end.min(max_idx);
        let y_end = y_end.min(max_idx);
        if x_start > x_end || y_start > y_end {
            return None;
        }

        let mut max_h: Option<i16> = None;
        for y in y_start..=y_end {
            for x in x_start..=x_end {
                if let Some(h) = self.get_height(x, y) {
                    if h != SRTM_VOID {
                        max_h = Some(max_h.map_or(h, |m| m.max(h)));
                    }
                }
            }
        }
        max_h
    }

    /// Height at a single DEM grid node (void → 0).
    #[inline]
    pub fn height_at_dem_index(&self, x: usize, y: usize) -> i16 {
        let max_idx = self.size.saturating_sub(1);
        let x = x.min(max_idx);
        let y = y.min(max_idx);
        match self.get_height(x, y) {
            Some(h) if h != SRTM_VOID => h,
            _ => 0,
        }
    }

    /// Max height over the non-overlapping DEM bin for a LOD mesh vertex.
    pub fn max_height_for_lod_vertex(
        &self,
        xi: usize,
        yi: usize,
        step: usize,
        vertices_per_row: usize,
    ) -> i16 {
        let (x_start, x_end, y_start, y_end) =
            self.lod_dem_bin(xi, yi, step, vertices_per_row);
        self.max_height_in_region(x_start, y_start, x_end, y_end)
            .unwrap_or(0)
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
    fn test_lod_bins_partition_dem_without_gaps() {
        let coord = TileCoord::new(0, 0);
        let tile = TileData::new(coord, 3601);
        let max_coord = 3600;

        for step in [1usize, 2, 8, 20, 40, 7] {
            let vpr = TileData::lod_vertices_per_row(step, tile.size);
            let mut x_count = vec![0usize; max_coord + 1];
            let mut y_count = vec![0usize; max_coord + 1];

            for xi in 0..vpr {
                for yi in 0..vpr {
                    let (xs, xe, ys, ye) = tile.lod_dem_bin(xi, yi, step, vpr);
                    assert!(xs <= xe && ys <= ye);
                    for x in xs..=xe {
                        x_count[x] += 1;
                    }
                    for y in ys..=ye {
                        y_count[y] += 1;
                    }
                }
            }

            assert!(x_count.iter().all(|&c| c == vpr), "step={step} x gaps/overlap");
            assert!(y_count.iter().all(|&c| c == vpr), "step={step} y gaps/overlap");
        }
    }

    #[test]
    fn test_max_height_for_lod_vertex() {
        let coord = TileCoord::new(0, 0);
        let mut tile = TileData::new(coord, 5);
        for y in 0..5 {
            for x in 0..5 {
                tile.set_height(x, y, (x + y) as i16);
            }
        }
        tile.set_height(1, 0, 100);

        // step=2 → bins [0..1] and [2..3] on a 5×5 grid (vertices_per_row=3)
        assert_eq!(tile.max_height_for_lod_vertex(0, 0, 2, 3), 100);
        assert_eq!(tile.max_height_for_lod_vertex(1, 0, 2, 3), 4);
        // step=1 → single-pixel bins
        tile.set_height(0, 0, 100);
        assert_eq!(tile.max_height_for_lod_vertex(0, 0, 1, 5), 100);
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
