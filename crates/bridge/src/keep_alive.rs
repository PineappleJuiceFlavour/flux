use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
pub struct KeepAlive {
    alive: Arc<AtomicBool>,
}

impl Default for KeepAlive {
    fn default() -> Self {
        Self::new()
    }
}

impl KeepAlive {
    pub fn new() -> Self {
        Self {
            alive: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn create_handle(&self) -> KeepAliveHandle {
        KeepAliveHandle {
            alive: Arc::clone(&self.alive),
        }
    }
}

impl Drop for KeepAlive {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone)]
pub struct KeepAliveHandle {
    alive: Arc<AtomicBool>,
}

impl KeepAliveHandle {
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
}
