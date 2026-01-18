//! Test Screen Capture
//!
//! Demonstrates the RemoteDesktop capture module with mock implementation.
//!
//! Run with:
//! ```bash
//! cargo run --example test_capture --features remotedesktop
//! ```

#[cfg(feature = "remotedesktop")]
use cosmic_connect_protocol::plugins::remotedesktop::capture::{QualityPreset, WaylandCapture};

#[cfg(feature = "remotedesktop")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("🖥️  RemoteDesktop Screen Capture Test");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    // Create capture instance
    println!("1. Creating WaylandCapture instance...");
    let mut capture = WaylandCapture::new().await?;
    println!("   ✓ Capture created");
    println!();

    // Enumerate monitors
    println!("2. Enumerating monitors...");
    let monitors = capture.enumerate_monitors().await?;
    println!("   ✓ Found {} monitor(s):", monitors.len());
    for monitor in &monitors {
        println!(
            "      - {} ({}x{} @ {}Hz) {}",
            monitor.name,
            monitor.width,
            monitor.height,
            monitor.refresh_rate,
            if monitor.is_primary { "[PRIMARY]" } else { "" }
        );
    }
    println!();

    // Select first monitor
    println!("3. Selecting monitor...");
    if let Some(monitor) = monitors.first() {
        capture.select_monitors(vec![monitor.id.clone()]);
        println!("   ✓ Selected: {}", monitor.name);
    }
    println!();

    // Request permission
    println!("4. Requesting screen capture permission...");
    capture.request_permission().await?;
    println!("   ✓ Permission granted (mock)");
    println!();

    // Start capture
    println!("5. Starting capture session...");
    capture.start_capture().await?;
    println!("   ✓ Capture session active");
    println!();

    // Capture frames
    println!("6. Capturing test frames...");
    for i in 1..=3 {
        let frame = capture.capture_frame().await?;
        println!(
            "   ✓ Frame {}: {}x{} {} ({} bytes)",
            i,
            frame.width,
            frame.height,
            frame.format.as_str(),
            frame.size()
        );

        // Convert to image and save first frame
        if i == 1 {
            #[cfg(feature = "remotedesktop")]
            if let Some(img) = frame.to_image_buffer() {
                let path = "/tmp/cosmic-connect-test-frame.png";
                img.save(path)?;
                println!("      📸 Saved to: {}", path);
            }
        }
    }
    println!();

    // Test quality presets
    println!("7. Testing quality presets...");
    let presets = [
        QualityPreset::Low,
        QualityPreset::Medium,
        QualityPreset::High,
    ];
    for preset in presets {
        let bitrate = preset.target_bitrate(1920, 1080, 30);
        println!("   - {}: {} Mbps", preset.as_str(), bitrate / 1_000_000);
    }
    println!();

    // Stop capture
    println!("8. Stopping capture session...");
    capture.stop_capture().await?;
    println!("   ✓ Capture stopped");
    println!();

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Test completed successfully!");
    println!();
    println!("Next steps:");
    println!("  • View test frame: open /tmp/cosmic-connect-test-frame.png");
    println!("  • Implement PipeWire integration for real capture");
    println!("  • Implement Desktop Portal via zbus for permissions");

    Ok(())
}

#[cfg(not(feature = "remotedesktop"))]
fn main() {
    eprintln!("❌ This example requires the 'remotedesktop' feature.");
    eprintln!("   Run with: cargo run --example test_capture --features remotedesktop");
    std::process::exit(1);
}
