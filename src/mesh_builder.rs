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
            height_scale: 0.001, // Convert meters to reasonable world units
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
    pub fn build_mesh(&self, tile: &TileData, colormap: &ColorMap) -> Mesh {
        let step = self.lod_level;
        let size = tile.size;
        
        // Calculate number of vertices (excluding last row/column)
        let max_coord = size - 1;
        let grid_size = (max_coord - 1) / step + 1;
        
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        let mut indices = Vec::new();
        
        // Generate vertices
        // IMPORTANT: SRTM tiles overlap at edges - the last row/column of one tile
        // is the same as the first row/column of the next tile.
        // We skip the last row and column to avoid duplication and ensure seamless edges.
        let max_coord = size - 1;
        
        for y in (0..max_coord).step_by(step) {
            for x in (0..max_coord).step_by(step) {
                let height = tile.get_height(x, y).unwrap_or(0) as f32;
                
                // Position vertices to span the full tile width (0 to 3601)
                // Even though we skip the last row/column of vertices,
                // we scale positions so the mesh spans the full tile
                let px = (x as f32 / (max_coord - 1) as f32) * (size as f32) * self.scale;
                let py = height * self.height_scale;
                let pz = (y as f32 / (max_coord - 1) as f32) * (size as f32) * self.scale;
                
                positions.push([px, py, pz]);
                
                // Color based on elevation
                let color = colormap.get_color(height);
                let c = color.to_srgba();
                colors.push([c.red, c.green, c.blue, 1.0]);
            }
        }
        
        // Generate triangle indices
        // Each grid cell creates 2 triangles
        for y in 0..(grid_size - 1) {
            for x in 0..(grid_size - 1) {
                let i0 = y * grid_size + x;
                let i1 = i0 + 1;
                let i2 = i0 + grid_size;
                let i3 = i2 + 1;
                
                // Triangle 1
                indices.push(i0 as u32);
                indices.push(i2 as u32);
                indices.push(i1 as u32);
                
                // Triangle 2
                indices.push(i1 as u32);
                indices.push(i2 as u32);
                indices.push(i3 as u32);
            }
        }
        
        // Calculate normals
        let normals = self.calculate_normals(&positions, &indices);
        
        // Build mesh
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
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
        
        // Generate triangles
        for y in 0..(grid_size - 1) {
            for x in 0..(grid_size - 1) {
                let i0 = y * grid_size + x;
                let i1 = i0 + 1;
                let i2 = i0 + grid_size;
                let i3 = i2 + 1;
                
                indices.push(i0 as u32);
                indices.push(i2 as u32);
                indices.push(i1 as u32);
                
                indices.push(i1 as u32);
                indices.push(i2 as u32);
                indices.push(i3 as u32);
            }
        }
        
        let normals = self.calculate_normals(&positions, &indices);
        
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
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
