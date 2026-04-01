use std::sync::mpsc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Stub implementation of SystemAudioCapture for Linux.
/// Linux does not have native system audio loopback like macOS (ScreenCaptureKit)
/// or Windows (WASAPI loopback). This stub allows compilation and development on Linux.
pub struct SystemAudioCapture {
    is_capturing: Arc<AtomicBool>,
}

impl SystemAudioCapture {
    pub fn new() -> Self {
        Self {
            is_capturing: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&self) -> Result<mpsc::Receiver<Vec<u8>>, String> {
        Err("System audio capture is not supported on Linux. Use macOS or Windows.".to_string())
    }

    pub fn stop(&self) {
        self.is_capturing.store(false, Ordering::SeqCst);
    }

    pub fn is_capturing(&self) -> bool {
        self.is_capturing.load(Ordering::SeqCst)
    }
}

impl Default for SystemAudioCapture {
    fn default() -> Self {
        Self::new()
    }
}
