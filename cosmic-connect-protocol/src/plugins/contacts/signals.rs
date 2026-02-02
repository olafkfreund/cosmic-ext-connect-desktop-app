//! DBus Signals for Contacts
//!
//! Provides DBus signal interface for contact synchronization events.
//! Allows desktop applications to be notified of contact changes in real-time.
//!
//! ## DBus Interface
//!
//! **Service**: `org.cosmic.Connect`
//! **Object Path**: `/org/cosmic/Connect/Contacts`
//! **Interface**: `org.cosmic.Connect.Contacts`
//!
//! ### Signals
//!
//! - **ContactAdded** (device_id: String, uid: String, name: String)
//!   - Emitted when a new contact is synced
//!
//! - **ContactUpdated** (device_id: String, uid: String, name: String)
//!   - Emitted when an existing contact is updated
//!
//! - **ContactDeleted** (device_id: String, uid: String)
//!   - Emitted when a contact is removed
//!
//! - **SyncCompleted** (device_id: String, total: u32, added: u32, updated: u32)
//!   - Emitted when a full sync operation completes
//!
//! ## Usage
//!
//! ### Emitting Signals (from daemon)
//!
//! ```rust,ignore
//! use cosmic_connect_protocol::plugins::contacts::signals::ContactsSignals;
//!
//! let signals = ContactsSignals::new().await?;
//!
//! // When contact is added
//! signals.emit_contact_added("device-123", "contact-456", "John Doe").await?;
//!
//! // When sync completes
//! signals.emit_sync_completed("device-123", 100, 5, 2).await?;
//! ```
//!
//! ### Listening for Signals (from UI apps)
//!
//! ```python
//! import dbus
//! from dbus.mainloop.glib import DBusGMainLoop
//! from gi.repository import GLib
//!
//! DBusGMainLoop(set_as_default=True)
//! bus = dbus.SessionBus()
//!
//! def contact_added_handler(device_id, uid, name):
//!     print(f"New contact: {name} from {device_id}")
//!
//! bus.add_signal_receiver(
//!     contact_added_handler,
//!     signal_name="ContactAdded",
//!     dbus_interface="org.cosmic.Connect.Contacts",
//!     path="/org/cosmic/Connect/Contacts"
//! )
//!
//! loop = GLib.MainLoop()
//! loop.run()
//! ```

use tracing::{debug, info};
use zbus::object_server::SignalEmitter;
use zbus::{interface, Connection};

/// DBus object path for contacts signals
pub const CONTACTS_OBJECT_PATH: &str = "/org/cosmic/Connect/Contacts";

/// DBus signal interface for contacts
pub struct ContactsSignals {
    connection: Connection,
}

impl ContactsSignals {
    /// Create new contacts signals interface and register on DBus
    ///
    /// Connects to the session bus and registers the interface at the contacts object path.
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        info!("Initializing contacts DBus signals");

        let connection = Connection::session().await?;

        let signals = Self {
            connection: connection.clone(),
        };

        connection
            .object_server()
            .at(CONTACTS_OBJECT_PATH, signals.clone())
            .await?;

        info!(
            "Contacts DBus signals registered at {}",
            CONTACTS_OBJECT_PATH
        );

        Ok(signals)
    }

    /// Emit signal when contact is added
    pub async fn emit_contact_added(
        &self,
        device_id: &str,
        uid: &str,
        name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!(
            "Emitting ContactAdded signal: device={}, uid={}, name={}",
            device_id, uid, name
        );

        let iface_ref = self
            .connection
            .object_server()
            .interface::<_, ContactsSignals>(CONTACTS_OBJECT_PATH)
            .await?;

        Self::contact_added(iface_ref.signal_emitter(), device_id, uid, name).await?;

        info!("Contact added: {} ({})", name, uid);
        Ok(())
    }

    /// Emit signal when contact is updated
    pub async fn emit_contact_updated(
        &self,
        device_id: &str,
        uid: &str,
        name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!(
            "Emitting ContactUpdated signal: device={}, uid={}, name={}",
            device_id, uid, name
        );

        let iface_ref = self
            .connection
            .object_server()
            .interface::<_, ContactsSignals>(CONTACTS_OBJECT_PATH)
            .await?;

        Self::contact_updated(iface_ref.signal_emitter(), device_id, uid, name).await?;

        info!("Contact updated: {} ({})", name, uid);
        Ok(())
    }

    /// Emit signal when contact is deleted
    pub async fn emit_contact_deleted(
        &self,
        device_id: &str,
        uid: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!(
            "Emitting ContactDeleted signal: device={}, uid={}",
            device_id, uid
        );

        let iface_ref = self
            .connection
            .object_server()
            .interface::<_, ContactsSignals>(CONTACTS_OBJECT_PATH)
            .await?;

        Self::contact_deleted(iface_ref.signal_emitter(), device_id, uid).await?;

        info!("Contact deleted: {}", uid);
        Ok(())
    }

    /// Emit signal when sync operation completes
    pub async fn emit_sync_completed(
        &self,
        device_id: &str,
        total: u32,
        added: u32,
        updated: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!(
            "Emitting SyncCompleted signal: device={}, total={}, added={}, updated={}",
            device_id, total, added, updated
        );

        let iface_ref = self
            .connection
            .object_server()
            .interface::<_, ContactsSignals>(CONTACTS_OBJECT_PATH)
            .await?;

        Self::sync_completed(iface_ref.signal_emitter(), device_id, total, added, updated).await?;

        info!(
            "Contacts sync completed for {}: {} total ({} added, {} updated)",
            device_id, total, added, updated
        );
        Ok(())
    }

    /// Emit signal for batch contact changes
    pub async fn emit_contacts_changed(
        &self,
        device_id: &str,
        changed_uids: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!(
            "Emitting ContactsChanged signal: device={}, count={}",
            device_id,
            changed_uids.len()
        );

        let iface_ref = self
            .connection
            .object_server()
            .interface::<_, ContactsSignals>(CONTACTS_OBJECT_PATH)
            .await?;

        Self::contacts_changed(iface_ref.signal_emitter(), device_id, &changed_uids).await?;

        info!("{} contacts changed for {}", changed_uids.len(), device_id);
        Ok(())
    }
}

impl Clone for ContactsSignals {
    fn clone(&self) -> Self {
        Self {
            connection: self.connection.clone(),
        }
    }
}

#[interface(name = "org.cosmic.Connect.Contacts")]
impl ContactsSignals {
    /// Signal emitted when a new contact is added
    ///
    /// # Arguments
    /// * `device_id` - Device ID that the contact belongs to
    /// * `uid` - Unique contact identifier
    /// * `name` - Contact display name
    #[zbus(signal)]
    async fn contact_added(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        uid: &str,
        name: &str,
    ) -> zbus::Result<()>;

    /// Signal emitted when a contact is updated
    ///
    /// # Arguments
    /// * `device_id` - Device ID that the contact belongs to
    /// * `uid` - Unique contact identifier
    /// * `name` - Updated contact display name
    #[zbus(signal)]
    async fn contact_updated(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        uid: &str,
        name: &str,
    ) -> zbus::Result<()>;

    /// Signal emitted when a contact is deleted
    ///
    /// # Arguments
    /// * `device_id` - Device ID that the contact belonged to
    /// * `uid` - Unique contact identifier
    #[zbus(signal)]
    async fn contact_deleted(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        uid: &str,
    ) -> zbus::Result<()>;

    /// Signal emitted when a sync operation completes
    ///
    /// # Arguments
    /// * `device_id` - Device ID that was synced
    /// * `total` - Total number of contacts
    /// * `added` - Number of contacts added
    /// * `updated` - Number of contacts updated
    #[zbus(signal)]
    async fn sync_completed(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        total: u32,
        added: u32,
        updated: u32,
    ) -> zbus::Result<()>;

    /// Signal emitted when multiple contacts change
    ///
    /// # Arguments
    /// * `device_id` - Device ID where contacts changed
    /// * `changed_uids` - List of contact UIDs that changed
    #[zbus(signal)]
    async fn contacts_changed(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        changed_uids: &[String],
    ) -> zbus::Result<()>;
}

/// Contact event type for unified handling
#[derive(Debug, Clone)]
pub enum ContactEvent {
    /// Contact was added
    Added {
        device_id: String,
        uid: String,
        name: Option<String>,
    },
    /// Contact was updated
    Updated {
        device_id: String,
        uid: String,
        name: Option<String>,
    },
    /// Contact was deleted
    Deleted { device_id: String, uid: String },
    /// Sync operation completed
    SyncCompleted {
        device_id: String,
        total: u32,
        added: u32,
        updated: u32,
    },
}

impl ContactEvent {
    /// Emit this event via DBus signals
    pub async fn emit(&self, signals: &ContactsSignals) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            ContactEvent::Added {
                device_id,
                uid,
                name,
            } => {
                signals
                    .emit_contact_added(device_id, uid, name.as_deref().unwrap_or("Unknown"))
                    .await
            }
            ContactEvent::Updated {
                device_id,
                uid,
                name,
            } => {
                signals
                    .emit_contact_updated(device_id, uid, name.as_deref().unwrap_or("Unknown"))
                    .await
            }
            ContactEvent::Deleted { device_id, uid } => {
                signals.emit_contact_deleted(device_id, uid).await
            }
            ContactEvent::SyncCompleted {
                device_id,
                total,
                added,
                updated,
            } => {
                signals
                    .emit_sync_completed(device_id, *total, *added, *updated)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_signals_creation() {
        // Note: This test requires a DBus session bus to be available
        // In CI environments without DBus, this will fail gracefully
        match ContactsSignals::new().await {
            Ok(_) => {
                // Successfully connected to session bus
            }
            Err(e) => {
                // Expected in environments without DBus
                eprintln!("Note: DBus session bus not available: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_contact_event_types() {
        let event = ContactEvent::Added {
            device_id: "device-123".to_string(),
            uid: "contact-456".to_string(),
            name: Some("John Doe".to_string()),
        };

        match event {
            ContactEvent::Added { ref name, .. } => {
                assert_eq!(name.as_deref(), Some("John Doe"));
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_contact_event_updated() {
        let event = ContactEvent::Updated {
            device_id: "device-456".to_string(),
            uid: "contact-789".to_string(),
            name: Some("Jane Smith".to_string()),
        };

        match event {
            ContactEvent::Updated {
                ref device_id,
                ref uid,
                ref name,
            } => {
                assert_eq!(device_id, "device-456");
                assert_eq!(uid, "contact-789");
                assert_eq!(name.as_deref(), Some("Jane Smith"));
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_contact_event_deleted() {
        let event = ContactEvent::Deleted {
            device_id: "device-123".to_string(),
            uid: "contact-999".to_string(),
        };

        match event {
            ContactEvent::Deleted {
                ref device_id,
                ref uid,
            } => {
                assert_eq!(device_id, "device-123");
                assert_eq!(uid, "contact-999");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_contact_event_sync_completed() {
        let event = ContactEvent::SyncCompleted {
            device_id: "device-abc".to_string(),
            total: 100,
            added: 10,
            updated: 5,
        };

        match event {
            ContactEvent::SyncCompleted {
                ref device_id,
                total,
                added,
                updated,
            } => {
                assert_eq!(device_id, "device-abc");
                assert_eq!(total, 100);
                assert_eq!(added, 10);
                assert_eq!(updated, 5);
            }
            _ => panic!("Wrong event type"),
        }
    }
}
