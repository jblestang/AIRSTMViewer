// Async tile downloader
use crate::tile::{TileCoord, TileData};
use bevy::prelude::*;
use std::sync::{mpsc::{channel, Receiver, Sender}, Arc, Mutex};

/// Download request for a tile
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub coord: TileCoord,
}

/// Download result
#[derive(Debug)]
#[allow(dead_code)]
pub enum DownloadResult {
    Success(TileData),
    Missing(TileCoord),
    Error(TileCoord, String),
}

/// Resource managing tile downloads
#[derive(Resource)]
pub struct TileDownloader {
    request_tx: Mutex<Sender<DownloadRequest>>,
    result_rx: Arc<Mutex<Receiver<DownloadResult>>>,
}

impl TileDownloader {
    /// Create a new tile downloader
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel::<DownloadRequest>();
        let (result_tx, result_rx) = channel::<DownloadResult>();

        // Spawn worker thread for downloads
        std::thread::spawn(move || {
            Self::download_worker(request_rx, result_tx);
        });

        Self {
            request_tx: Mutex::new(request_tx),
            result_rx: Arc::new(Mutex::new(result_rx)),
        }
    }

    /// Request a tile download
    pub fn request_download(&self, coord: TileCoord) {
        if let Ok(tx) = self.request_tx.lock() {
            let _ = tx.send(DownloadRequest { coord });
        }
    }

    /// Poll for download results
    pub fn poll_results(&self) -> Vec<DownloadResult> {
        let mut results = Vec::new();
        if let Ok(rx) = self.result_rx.lock() {
            while let Ok(result) = rx.try_recv() {
                results.push(result);
            }
        }
        results
    }

    /// Worker thread that processes download requests
    fn download_worker(request_rx: Receiver<DownloadRequest>, result_tx: Sender<DownloadResult>) {
        // External downloads are disabled per user request
        let sources: Vec<&str> = vec![];

        while let Ok(request) = request_rx.recv() {
            let result = Self::download_tile(&request.coord, &sources);
            let _ = result_tx.send(result);
        }
    }

    /// Download a single tile
    fn download_tile(coord: &TileCoord, _sources: &[&str]) -> DownloadResult {
        // For now, we'll use a simpler approach: try to download from a public source
        // In production, you'd iterate through sources and handle authentication
        
        // let filename = coord.filename();
        
        // Try USGS EarthExplorer (note: this may require authentication)
        // For this demo, we'll simulate downloads or use local files
        
        // Attempt to download (this is a placeholder - real implementation would use reqwest)
        // For now, we'll just return Missing for tiles that don't exist locally
        
        //info!("Attempting to download tile: {}", filename);
        
        // Simulate download failure (in real implementation, use reqwest to fetch)
        // You would need to implement proper URL construction and HTTP requests here
        
        DownloadResult::Missing(*coord)
    }
}

impl Default for TileDownloader {
    fn default() -> Self {
        Self::new()
    }
}

/// System to process download results
pub fn process_downloads(
    downloader: Res<TileDownloader>,
    mut cache: ResMut<crate::cache::TileCache>,
) {
    use crate::tile::TileState;
    
    for result in downloader.poll_results() {
        match result {
            DownloadResult::Success(tile_data) => {
                info!("Downloaded tile: {:?}", tile_data.coord);
                
                // Save to disk cache (explicit deref to help compiler)
                if let Err(e) = cache.as_ref().save_to_disk(&tile_data) {
                    error!("Failed to save tile to disk: {}", e);
                }
                
                // Update cache with Arc
                cache.insert_tile(tile_data.coord, TileState::Loaded(std::sync::Arc::new(tile_data)));
            }
            DownloadResult::Missing(coord) => {
                //info!("Tile not found: {:?}. Using empty tile.", coord);
                let empty_data = crate::tile::TileData::new(coord, 3601);
                cache.insert_tile(coord, TileState::Loaded(std::sync::Arc::new(empty_data)));
            }
            DownloadResult::Error(coord, err) => {
                error!("Failed to download tile {:?}: {}", coord, err);
                cache.insert_tile(coord, TileState::Error(err));
            }
        }
    }
}
