use std::sync::Arc;
use tokio::sync::RwLock;

use livtet_core::search::SearchIndex;

pub type ArcMut<T> = Arc<RwLock<T>>;

#[derive(Clone)]
pub struct AppState {
    pub search_index: ArcMut<Option<SearchIndex>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            search_index: Arc::new(RwLock::new(None)),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}