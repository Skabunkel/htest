//! Managed WebDriver server. When a playbook sets `manage_driver: true`, htest
//! spawns the driver itself (geckodriver / chromedriver), waits for its port to
//! accept connections, and kills it on drop — so a CI job can say "run this
//! playbook" without a separate step to start and stop the driver.

use crate::browser::BrowserKind;
use crate::error::Result;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A driver process owned by htest. Dropping it kills the process.
pub struct ManagedDriver {
    child: Child,
    /// Human name for logs/errors (e.g. "geckodriver").
    bin: String,
    /// URL to hand to the WebDriver backend.
    pub url: String,
}

impl ManagedDriver {
    /// Spawn the driver for `kind` on `port` and block until it's accepting
    /// connections. `path` overrides the executable (else the conventional
    /// name is looked up on `PATH`).
    pub fn start(kind: BrowserKind, path: Option<&Path>, port: u16) -> Result<Self> {
        let default_bin = match kind {
            BrowserKind::Firefox => "geckodriver",
            BrowserKind::Chrome => "chromedriver",
        };
        let bin = path
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| default_bin.to_string());

        let mut cmd = Command::new(&bin);
        // Port flag spelling differs between the two drivers.
        match kind {
            BrowserKind::Firefox => {
                cmd.arg("--port").arg(port.to_string());
            }
            BrowserKind::Chrome => {
                cmd.arg(format!("--port={port}"));
            }
        }
        // The driver's own logging would drown the test report; discard it.
        cmd.stdout(Stdio::null()).stderr(Stdio::null());

        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!("starting `{bin}` on port {port}: {e} (is it installed / on PATH?)")
        })?;

        let mut d = ManagedDriver {
            child,
            bin: bin.clone(),
            url: format!("http://localhost:{port}"),
        };
        d.wait_for_port(port, Duration::from_secs(10))?;
        Ok(d)
    }

    /// Poll the port until it accepts a TCP connection or the timeout elapses.
    fn wait_for_port(&mut self, port: u16, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            // If the driver died on startup, surface that instead of spinning.
            if let Ok(Some(status)) = self.child.try_wait() {
                anyhow::bail!("`{}` exited during startup ({status})", self.bin);
            }
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                anyhow::bail!(
                    "`{}` did not open port {port} within {:?}",
                    self.bin,
                    timeout
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for ManagedDriver {
    fn drop(&mut self) {
        // Best-effort teardown: kill the driver and reap it so no zombie or
        // orphaned browser lingers between runs.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
