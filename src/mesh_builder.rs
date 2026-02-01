// Triangle mesh generation for terrain
use crate::colormap::ColorMap;
use crate::tile::TileData;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};

/// Build a terrain mesh from tile data
pub struct TerrainMeshBuilder {
    pub lod_level: usize,  // Level of detail (1 = full res, 2 = half res, etc.)
    pub scale: f32,        // Horizontal scale factor
    pub height_scale: f32, // Vertical exaggeration
}

impl Default for TerrainMeshBuilder {
    fn default() -> Self {
        Self {
            lod_level: 1,
            scale: 1.0,
            height_scale: 0.25, // Exaggerate height for better visibility (0.25 as per user request)
        }
    }
}

impl TerrainMeshBuilder {
    /// Create a new mesh builder with specified LOD
    pub fn new(lod_level: usize) -> Self {
        Self {
            lod_level,
            ..Default::default()
        }
    }

    /// Build a mesh from tile data using triangles
    pub fn build_mesh(&self, tile: &TileData, colormap: &ColorMap, radar: Option<&crate::radar::Radar>, cache: Option<&crate::cache::TileCache>) -> Mesh {
        let step = self.lod_level;
        let size = tile.size;
        
        // Calculate number of vertices (excluding last row/column)
        let max_coord = size - 1;
        let grid_size = (max_coord - 1) / step + 1;
        
        // We need to generate vertices up to max_coord inclusive
        let vertices_per_row = max_coord / step + 1;
        
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        let mut indices = Vec::new();
        
        // Tile origin in World Coordinates (lat/lon)
        // Tile N43E007 origin is 43N, 7E.
        // x index 0..3600 maps to 0..1 deg.
        let tile_lat_base = tile.coord.lat as f64;
        let tile_lon_base = tile.coord.lon as f64;
        
        for y in (0..=max_coord).step_by(step) {
            for x in (0..=max_coord).step_by(step) {
                let height = tile.get_height(x, y).unwrap_or(0) as f32;
                
                // Position vertices to span the full tile width (0 to 3601)
                let px = (x as f32 / max_coord as f32) * (size as f32) * self.scale;
                let py = height * self.height_scale;
                let pz = (y as f32 / max_coord as f32) * (size as f32) * self.scale;
                
                positions.push([px, py, pz]);
                
                // Determine color
                let mut color = colormap.get_color(height).to_srgba();
                
                // Overlay Radar Visibility
                if let Some(r) = radar {
                    // Calculate geographic position of vertex
                    // Lat increases with Y? NO.
                    // Filenames: "Nxx". 
                    // Y=0 is North edge? Or South?
                    // Standard SRTM: Row 0 is North. Row 3600 is South.
                    // My previous analysis: "N43" means 43 to 44.
                    // And I map Z = -(lat+1)*size + local_z.
                    // local_z goes 0..size.
                    // If row 0 is North, then row 0 corresponds to lat+1.
                    // If row 3600 is South, corresponds to lat.
                    // Let's assume standard SRTM: Row 0 = North Limit (e.g. 44). Row 3600 = South Limit (43).
                    // So vertex_lat = (lat + 1) - (y / 3600).
                    
                    let v_lat = (tile_lat_base + 1.0) - (y as f64 / max_coord as f64);
                    let v_lon = tile_lon_base + (x as f64 / max_coord as f64);
                    
                    let visible = if let Some(c) = cache {
                        r.is_visible_raycast(v_lat, v_lon, height as f32, c)
                    } else {
                        r.is_visible(v_lat, v_lon, height as f32)
                    };

                    if visible {
                        // Green for visible
                        color = Color::srgb(0.0, 1.0, 0.0).into();
                    } else {
                        // Red for hidden
                        // color = Color::srgb(1.0, 0.0, 0.0);
                        // Or Keep original color but dimmed? Or distinct Red?
                        color = Color::srgb(1.0, 0.0, 0.0).into();
                    }
                }
                
                colors.push([color.red, color.green, color.blue, 1.0]);
            }
        }
        
        // Generate wireframe indices (optimized: min lines)
        // Grid size is number of cells
        let cell_cols = vertices_per_row - 1;
        let cell_rows = vertices_per_row - 1;
        
        for y in 0..cell_rows {
            for x in 0..cell_cols {
                let i0 = y * vertices_per_row + x;
                let i1 = i0 + 1;
                let i2 = i0 + vertices_per_row;
                // let i3 = i2 + 1; 
                
                // Optimized Wireframe: Top, Left, Diagonal
                indices.push(i0 as u32); indices.push(i1 as u32); // Top (i0-i1)
                indices.push(i0 as u32); indices.push(i2 as u32); // Left (i0-i2)
                indices.push(i1 as u32); indices.push(i2 as u32); // Diagonal (i1-i2)
                
                // If last column, draw Right edge
                if x == cell_cols - 1 {
                    let i3 = i2 + 1;
                     indices.push(i1 as u32); indices.push(i3 as u32); // Right (i1-i3)
                }
                
                // If last row, draw Bottom edge
                if y == cell_rows - 1 {
                    let i3 = i2 + 1;
                     indices.push(i2 as u32); indices.push(i3 as u32); // Bottom (i2-i3)
                }
            }
        }
        
        // Dummy normals for wireframe (Unlit material doesn't use them, but shader expects attribute)
        let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
        
        // Build mesh as LineList for wireframe
        let mut mesh = Mesh::new(PrimitiveTopology::LineList, Default::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32(indices));
        
        mesh
    }

    /// Build a placeholder mesh for missing tiles (red at height 0)
    pub fn build_missing_mesh(&self) -> Mesh {
        let size = 100; // Simple low-res grid for missing tiles
        let step = self.lod_level.max(10);
        let grid_size = size / step + 1;
        
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        let mut indices = Vec::new();
        
        // Generate flat red grid
        for y in (0..=size).step_by(step) {
            for x in (0..=size).step_by(step) {
                // Use absolute coordinates to match terrain tiles
                let px = x as f32 * self.scale;
                let py = 0.0; // Height 0
                let pz = y as f32 * self.scale;
                
                positions.push([px, py, pz]);
                colors.push([1.0, 0.0, 0.0, 1.0]); // Red
            }
        }
        
        // Generate wireframe indices for missing tile
        let grid_w = size / step; // Number of cells
        
        for y in 0..grid_w {
            for x in 0..grid_w {
                let i0 = y * (grid_w + 1) + x;
                let i1 = i0 + 1;
                let i2 = i0 + (grid_w + 1);
                
                // Wireframe lines
                indices.push(i0 as u32); indices.push(i1 as u32); // Top
                indices.push(i0 as u32); indices.push(i2 as u32); // Left
                indices.push(i1 as u32); indices.push(i2 as u32); // Diagonal
                
                // Right and Bottom edges
                if x == grid_w - 1 {
                    let i3 = i2 + 1;
                    indices.push(i1 as u32); indices.push(i3 as u32);
                }
                if y == grid_w - 1 {
                    let i3 = i2 + 1;
                    indices.push(i2 as u32); indices.push(i3 as u32);
                }
            }
        }
        
        let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
        
        let mut mesh = Mesh::new(PrimitiveTopology::LineList, Default::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32(indices));
        
        mesh
    }

    /// Calculate normals for the mesh
    fn calculate_normals(&self, positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
        let mut normals = vec![[0.0f32, 0.0, 0.0]; positions.len()];
        
        // Calculate face normals and accumulate
        for triangle in indices.chunks(3) {
            let i0 = triangle[0] as usize;
            let i1 = triangle[1] as usize;
            let i2 = triangle[2] as usize;
            
            let p0 = Vec3::from(positions[i0]);
            let p1 = Vec3::from(positions[i1]);
            let p2 = Vec3::from(positions[i2]);
            
            let edge1 = p1 - p0;
            let edge2 = p2 - p0;
            let normal = edge1.cross(edge2);
            
            // Accumulate normals
            normals[i0][0] += normal.x;
            normals[i0][1] += normal.y;
            normals[i0][2] += normal.z;
            
            normals[i1][0] += normal.x;
            normals[i1][1] += normal.y;
            normals[i1][2] += normal.z;
            
            normals[i2][0] += normal.x;
            normals[i2][1] += normal.y;
            normals[i2][2] += normal.z;
        }
        
        // Normalize
        for normal in &mut normals {
            let len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            if len > 0.0 {
                normal[0] /= len;
                normal[1] /= len;
                normal[2] /= len;
            } else {
                normal[1] = 1.0; // Default to up
            }
        }
        
        normals
    }
}
