// Color mapping for terrain elevation
use bevy::prelude::*;

/// Color map for converting elevation to colors
#[derive(Debug, Clone, Resource)]
pub struct ColorMap {
    stops: Vec<(f32, Color)>,  // (elevation, color) pairs
}

impl Default for ColorMap {
    fn default() -> Self {
        Self::terrain()
    }
}

impl ColorMap {
    /// Create a standard terrain colormap
    /// Blue (water/low) -> Green (plains) -> Brown (mountains) -> White (peaks)
    pub fn terrain() -> Self {
        Self {
            stops: vec![
                (-500.0, Color::srgb(0.0, 0.2, 0.6)),      // Deep blue (below sea level)
                (0.0, Color::srgb(0.2, 0.4, 0.8)),         // Light blue (sea level)
                (100.0, Color::srgb(0.3, 0.6, 0.3)),       // Green (lowlands)
                (500.0, Color::srgb(0.5, 0.7, 0.3)),       // Yellow-green (hills)
                (1000.0, Color::srgb(0.6, 0.5, 0.3)),      // Brown (mountains)
                (2000.0, Color::srgb(0.7, 0.6, 0.5)),      // Light brown (high mountains)
                (3000.0, Color::srgb(0.9, 0.9, 0.9)),      // White (peaks)
                (5000.0, Color::srgb(1.0, 1.0, 1.0)),      // Pure white (very high peaks)
            ],
        }
    }

    /// Get color for a given elevation
    pub fn get_color(&self, elevation: f32) -> Color {
        // Handle edge cases
        if self.stops.is_empty() {
            return Color::srgb(0.5, 0.5, 0.5); // Gray
        }
        
        if elevation <= self.stops[0].0 {
            return self.stops[0].1;
        }
        
        if elevation >= self.stops[self.stops.len() - 1].0 {
            return self.stops[self.stops.len() - 1].1;
        }
        
        // Find the two stops to interpolate between
        for i in 0..self.stops.len() - 1 {
            let (elev0, color0) = self.stops[i];
            let (elev1, color1) = self.stops[i + 1];
            
            if elevation >= elev0 && elevation <= elev1 {
                // Linear interpolation
                let t = (elevation - elev0) / (elev1 - elev0);
                let c0 = color0.to_srgba();
                let c1 = color1.to_srgba();
                return Color::srgb(
                    c0.red * (1.0 - t) + c1.red * t,
                    c0.green * (1.0 - t) + c1.green * t,
                    c0.blue * (1.0 - t) + c1.blue * t,
                );
            }
        }
        
        Color::srgb(0.5, 0.5, 0.5) // Gray
    }

    /// Create a custom colormap from elevation-color pairs
    pub fn custom(mut stops: Vec<(f32, Color)>) -> Self {
        stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        Self { stops }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colormap_terrain() {
        let cmap = ColorMap::terrain();
        
        // Test sea level
        let color = cmap.get_color(0.0);
        assert_eq!(color, Color::srgb(0.2, 0.4, 0.8));
        
        // Test interpolation
        let color = cmap.get_color(50.0);
        let c = color.to_srgba();
        assert!(c.red > 0.2 && c.red < 0.3);
        assert!(c.green > 0.4 && c.green < 0.6);
    }

    #[test]
    fn test_colormap_bounds() {
        let cmap = ColorMap::terrain();
        
        // Below minimum
        let color = cmap.get_color(-1000.0);
        assert_eq!(color, Color::srgb(0.0, 0.2, 0.6));
        
        // Above maximum
        let color = cmap.get_color(10000.0);
        assert_eq!(color, Color::srgb(1.0, 1.0, 1.0));
    }
}
