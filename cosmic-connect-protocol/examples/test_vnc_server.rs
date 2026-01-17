//! Test VNC Server
//!
//! Demonstrates the complete VNC server implementation with RFB 3.8 protocol.
//!
//! ## Usage
//!
//! 1. Start the server:
//! ```bash
//! cargo run --example test_vnc_server --features remotedesktop
//! ```
//!
//! 2. Connect with a VNC client:
//! ```bash
//! # TigerVNC
//! vncviewer localhost:5900
//!
//! # macOS Screen Sharing
//! open vnc://localhost:5900
//!
//! # RealVNC
//! vncviewer localhost::5900
//! ```
//!
//! ## Features
//!
//! - RFB 3.8 protocol handshake
//! - VNC authentication with auto-generated password
//! - Screen capture via Wayland (mock)
//! - LZ4 frame encoding
//! - Framebuffer updates at 30 FPS
//! - Keyboard and mouse input (logged, not yet forwarded)

#[cfg(feature = "remotedesktop")]
use cosmic_connect_protocol::plugins::remotedesktop::{
    capture::WaylandCapture,
    vnc::VncServer,
};

#[cfg(feature = "remotedesktop")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!();
    println!("╔═══════════════════════════════════════════════════════╗");
    println!("║         COSMIC Connect VNC Server Test               ║");
    println!("╚═══════════════════════════════════════════════════════╝");
    println!();

    // Create capture instance
    println!("1. Initializing screen capture...");
    let mut capture = WaylandCapture::new().await?;

    // Setup capture
    let monitors = capture.enumerate_monitors().await?;
    println!("   ✓ Found {} monitor(s)", monitors.len());

    if let Some(monitor) = monitors.first() {
        println!("     - {} ({}x{} @ {}Hz)",
            monitor.name,
            monitor.width,
            monitor.height,
            monitor.refresh_rate
        );
        capture.select_monitors(vec![monitor.id.clone()]);
    }

    capture.request_permission().await?;
    println!("   ✓ Permission granted (mock)");

    capture.start_capture().await?;
    println!("   ✓ Capture session active");
    println!();

    // Create VNC server with auto-generated password
    println!("2. Starting VNC server...");
    let (mut server, password) = VncServer::with_generated_password(5900);

    println!("   ✓ VNC server created");
    println!("   ✓ Listening on: 0.0.0.0:5900");
    println!("   ✓ Password: {}", password);
    println!();

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📺 VNC Server Ready!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Connect with a VNC client:");
    println!();
    println!("  TigerVNC:");
    println!("    vncviewer localhost:5900");
    println!();
    println!("  macOS Screen Sharing:");
    println!("    open vnc://localhost:5900");
    println!();
    println!("  RealVNC:");
    println!("    vncviewer localhost::5900");
    println!();
    println!("  Password: {}", password);
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    println!("3. Waiting for client connection...");
    println!("   (Press Ctrl+C to stop)");
    println!();

    // Start VNC server (blocks until client disconnects)
    if let Err(e) = server.start(capture).await {
        eprintln!("❌ VNC server error: {}", e);
        std::process::exit(1);
    }

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ VNC server test completed!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    Ok(())
}

#[cfg(not(feature = "remotedesktop"))]
fn main() {
    eprintln!("❌ This example requires the 'remotedesktop' feature.");
    eprintln!("   Run with: cargo run --example test_vnc_server --features remotedesktop");
    std::process::exit(1);
}
