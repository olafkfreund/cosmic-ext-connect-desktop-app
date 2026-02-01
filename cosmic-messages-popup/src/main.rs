//! COSMIC Messages Popup
//!
//! A COSMIC-native messaging popup that displays web-based messaging interfaces
//! (Google Messages, WhatsApp, Telegram, etc.) when notifications arrive from
//! cosmic-connect. This provides full RCS/messaging support without needing to
//! reverse-engineer proprietary APIs.
//!
//! ## Features
//!
//! - WebView integration for web messaging services
//! - D-Bus interface for cosmic-connect notification integration
//! - Session persistence for each messenger
//! - Configurable popup settings
//! - Keyboard shortcuts for quick access
//!
//! ## Architecture
//!
//! ```text
//! cosmic-connect-daemon
//!         │
//!         ▼
//!   ┌──────────────┐    D-Bus     ┌─────────────────────────────┐
//!   │ Notification │─────────────▶│   cosmic-messages-popup     │
//!   │   Service    │              │                             │
//!   └──────────────┘              │  ┌───────────────────────┐  │
//!                                 │  │     WebView (wry)     │  │
//!                                 │  │ messages.google.com   │  │
//!                                 │  │ web.whatsapp.com      │  │
//!                                 │  │ web.telegram.org      │  │
//!                                 │  └───────────────────────┘  │
//!                                 └─────────────────────────────┘
//! ```

use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod app;
mod config;
mod dbus;
mod gtk_webview;
mod notification;
mod webview;

pub use app::{Message, MessagesPopup};
pub use config::Config;
pub use dbus::{DbusCommand, NotificationData};

/// COSMIC Messages Popup - Web-based messaging interface
#[derive(Parser, Debug)]
#[command(name = "cosmic-messages-popup")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Show the popup immediately on startup
    #[arg(short, long)]
    show: bool,

    /// Open a specific messenger (google-messages, whatsapp, telegram, etc.)
    #[arg(short, long)]
    messenger: Option<String>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Run in daemon mode (listen for D-Bus commands without showing window)
    #[arg(long)]
    daemon: bool,
}

fn main() -> cosmic::iced::Result {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize logging
    let filter = if args.debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true))
        .with(filter)
        .init();

    info!("Starting COSMIC Messages Popup");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));

    // Start GTK event loop in background thread
    // Note: GTK is initialized INSIDE the thread (GTK requires all ops on same thread)
    let _gtk_handle = gtk_webview::start_gtk_event_loop();
    info!("GTK event loop thread spawned");

    // Create D-Bus channel and store receiver for polling
    let (dbus_sender, dbus_receiver) = app::create_dbus_channel();
    app::set_dbus_receiver(dbus_receiver);

    // Start D-Bus service in background
    let dbus_sender_clone = dbus_sender.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async {
            match dbus::start_dbus_service(dbus_sender_clone).await {
                Ok(_conn) => {
                    info!("D-Bus service started successfully");
                    // Keep connection alive
                    std::future::pending::<()>().await;
                }
                Err(e) => {
                    error!("Failed to start D-Bus service: {}", e);
                }
            }
        });
    });

    // Handle daemon mode
    if args.daemon {
        info!("Running in daemon mode - waiting for D-Bus commands");
        // In daemon mode, we just keep the D-Bus service running
        // The application will be shown via D-Bus commands
    }

    // Handle initial messenger selection
    if let Some(messenger) = args.messenger.clone() {
        info!("Opening messenger: {}", messenger);
        let sender = dbus_sender.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                let _ = sender.send(DbusCommand::ShowMessenger(messenger)).await;
            });
        });
    }

    // Configure COSMIC application settings
    let settings = cosmic::app::Settings::default()
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(300.0)
                .min_height(400.0),
        )
        .exit_on_close(false);

    // Run the application
    cosmic::app::run::<MessagesPopup>(settings, dbus_sender)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_parsing() {
        let args = Args::parse_from(["cosmic-messages-popup"]);
        assert!(!args.show);
        assert!(args.messenger.is_none());
        assert!(!args.debug);
    }

    #[test]
    fn test_args_with_options() {
        let args = Args::parse_from(["cosmic-messages-popup", "--show", "--messenger", "whatsapp"]);
        assert!(args.show);
        assert_eq!(args.messenger, Some("whatsapp".to_string()));
    }
}
