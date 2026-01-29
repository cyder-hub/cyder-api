use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct CacheMetrics {
    hits: Arc<AtomicU64>,
    misses: Arc<AtomicU64>,
    sets: Arc<AtomicU64>,
    deletes: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
}

impl Default for CacheMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of cache metrics at a point in time
#[derive(Debug, Clone, Serialize)]
pub struct CacheMetricsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub sets: u64,
    pub deletes: u64,
    pub errors: u64,
    pub hit_rate: f64,
    pub total_requests: u64,
}

impl CacheMetrics {
    pub fn new() -> Self {
        Self {
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
            sets: Arc::new(AtomicU64::new(0)),
            deletes: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(AtomicU64::new(0)),
        }
    }
    
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_set(&self) {
        self.sets.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_delete(&self) {
        self.deletes.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        
        if total == 0 {
            return 0.0;
        }
        
        hits as f64 / total as f64
    }
    
    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }
    
    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }
    
    pub fn sets(&self) -> u64 {
        self.sets.load(Ordering::Relaxed)
    }
    
    pub fn deletes(&self) -> u64 {
        self.deletes.load(Ordering::Relaxed)
    }
    
    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }
    
    pub fn total_requests(&self) -> u64 {
        self.hits.load(Ordering::Relaxed) + self.misses.load(Ordering::Relaxed)
    }
    
    /// Get a snapshot of all metrics at this point in time
    pub fn snapshot(&self) -> CacheMetricsSnapshot {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let sets = self.sets.load(Ordering::Relaxed);
        let deletes = self.deletes.load(Ordering::Relaxed);
        let errors = self.errors.load(Ordering::Relaxed);
        let total_requests = hits + misses;
        let hit_rate = if total_requests == 0 {
            0.0
        } else {
            hits as f64 / total_requests as f64
        };
        
        CacheMetricsSnapshot {
            hits,
            misses,
            sets,
            deletes,
            errors,
            hit_rate,
            total_requests,
        }
    }
    
    /// Reset all metrics to zero
    pub fn reset(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.sets.store(0, Ordering::Relaxed);
        self.deletes.store(0, Ordering::Relaxed);
        self.errors.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_metrics_snapshot() {
        let metrics = CacheMetrics::new();
        
        metrics.record_hit();
        metrics.record_hit();
        metrics.record_miss();
        metrics.record_set();
        
        let snapshot = metrics.snapshot();
        
        assert_eq!(snapshot.hits, 2);
        assert_eq!(snapshot.misses, 1);
        assert_eq!(snapshot.sets, 1);
        assert_eq!(snapshot.total_requests, 3);
        assert!((snapshot.hit_rate - 0.666).abs() < 0.01);
    }
    
    #[test]
    fn test_metrics_reset() {
        let metrics = CacheMetrics::new();
        
        metrics.record_hit();
        metrics.record_miss();
        
        assert_eq!(metrics.hits(), 1);
        assert_eq!(metrics.misses(), 1);
        
        metrics.reset();
        
        assert_eq!(metrics.hits(), 0);
        assert_eq!(metrics.misses(), 0);
    }
}
