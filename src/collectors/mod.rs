use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Log stream collector for macOS Unified Log System
pub mod log_collector;

/// Metrics collector for system resource monitoring
pub mod metrics_collector;

/// Disk/filesystem collector for disk I/O monitoring
pub mod disk_collector;

pub use disk_collector::DiskCollector;
pub use log_collector::LogCollector;
pub use metrics_collector::MetricsCollector;

#[cfg(unix)]
fn set_nonblocking<T: std::os::fd::AsRawFd>(stream: &T) -> std::io::Result<()> {
    let fd = stream.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(std::io::Error::last_os_error());
    }

    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(not(unix))]
fn set_nonblocking<T>(_stream: &T) -> std::io::Result<()> {
    Ok(())
}

fn wait_for_retry(delay: Duration, running: &Arc<Mutex<bool>>) {
    let deadline = Instant::now() + delay;
    while *running.lock().unwrap() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(100)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_wait_stops_when_collector_stops() {
        let running = Arc::new(Mutex::new(true));
        let shutdown = Arc::clone(&running);
        let shutdown_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            *shutdown.lock().unwrap() = false;
        });
        let started = Instant::now();

        wait_for_retry(Duration::from_secs(5), &running);
        shutdown_thread.join().unwrap();

        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
