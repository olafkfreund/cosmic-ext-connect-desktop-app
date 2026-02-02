//! Contacts Database Storage
//!
//! Provides persistent storage for synchronized contacts using SQLite.
//! Stores contact information including vCard data, phone numbers, emails, and metadata.
//!
//! ## Schema
//!
//! ### contacts table
//! - `id`: INTEGER PRIMARY KEY AUTOINCREMENT
//! - `uid`: TEXT UNIQUE NOT NULL (device-specific contact ID)
//! - `device_id`: TEXT NOT NULL (source device)
//! - `name`: TEXT (full name from FN field)
//! - `vcard_data`: TEXT NOT NULL (raw vCard 2.1 data)
//! - `timestamp`: INTEGER NOT NULL (last modification time, milliseconds)
//! - `created_at`: INTEGER NOT NULL (when synced to desktop)
//! - `updated_at`: INTEGER NOT NULL (when last updated)
//!
//! ### contact_phones table
//! - `id`: INTEGER PRIMARY KEY AUTOINCREMENT
//! - `contact_id`: INTEGER NOT NULL (FK to contacts.id)
//! - `phone_number`: TEXT NOT NULL
//! - `phone_type`: TEXT (HOME, WORK, CELL, etc.)
//!
//! ### contact_emails table
//! - `id`: INTEGER PRIMARY KEY AUTOINCREMENT
//! - `contact_id`: INTEGER NOT NULL (FK to contacts.id)
//! - `email`: TEXT NOT NULL
//! - `email_type`: TEXT (HOME, WORK, etc.)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use cosmic_connect_protocol::plugins::contacts::database::ContactsDatabase;
//!
//! // Initialize database
//! let mut db = ContactsDatabase::new("/path/to/contacts.db").await?;
//!
//! // Store contact
//! let contact = Contact {
//!     uid: "contact-123".to_string(),
//!     device_id: "device-456".to_string(),
//!     name: Some("John Doe".to_string()),
//!     vcard_data: "BEGIN:VCARD...END:VCARD".to_string(),
//!     timestamp: 1234567890000,
//!     phone_numbers: vec!["+1234567890".to_string()],
//!     emails: vec!["john@example.com".to_string()],
//! };
//!
//! db.upsert_contact(contact).await?;
//! ```

use crate::{ProtocolError, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

/// Contact information
#[derive(Debug, Clone)]
pub struct Contact {
    /// Unique identifier (device-specific)
    pub uid: String,
    /// Source device ID
    pub device_id: String,
    /// Full name
    pub name: Option<String>,
    /// Raw vCard 2.1 data
    pub vcard_data: String,
    /// Last modification timestamp (milliseconds)
    pub timestamp: i64,
    /// Phone numbers
    pub phone_numbers: Vec<PhoneNumber>,
    /// Email addresses
    pub emails: Vec<Email>,
}

/// Phone number with optional type
#[derive(Debug, Clone)]
pub struct PhoneNumber {
    pub number: String,
    pub phone_type: Option<String>,
}

/// Email address with optional type
#[derive(Debug, Clone)]
pub struct Email {
    pub address: String,
    pub email_type: Option<String>,
}

/// Contacts database interface
pub struct ContactsDatabase {
    conn: Arc<Mutex<Connection>>,
    db_path: String,
}

impl ContactsDatabase {
    /// Create or open contacts database
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let path_str = db_path.as_ref().to_string_lossy().to_string();
        info!("Initializing contacts database at: {}", path_str);

        // Ensure directory exists
        if let Some(parent) = db_path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ProtocolError::Io(std::io::Error::other(
                    format!("Failed to create database directory: {}", e),
                ))
            })?;
        }

        let conn = Connection::open(&db_path).map_err(|e| {
            ProtocolError::Plugin(format!("Failed to open contacts database: {}", e))
        })?;

        // Create tables
        conn.execute_batch(
            "BEGIN;
            CREATE TABLE IF NOT EXISTS contacts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uid TEXT NOT NULL,
                device_id TEXT NOT NULL,
                name TEXT,
                vcard_data TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now') * 1000),
                UNIQUE(uid, device_id)
            );
            CREATE INDEX IF NOT EXISTS idx_contacts_uid ON contacts(uid);
            CREATE INDEX IF NOT EXISTS idx_contacts_device_id ON contacts(device_id);
            CREATE INDEX IF NOT EXISTS idx_contacts_name ON contacts(name);

            CREATE TABLE IF NOT EXISTS contact_phones (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                contact_id INTEGER NOT NULL,
                phone_number TEXT NOT NULL,
                phone_type TEXT,
                FOREIGN KEY(contact_id) REFERENCES contacts(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_contact_phones_number ON contact_phones(phone_number);

            CREATE TABLE IF NOT EXISTS contact_emails (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                contact_id INTEGER NOT NULL,
                email TEXT NOT NULL,
                email_type TEXT,
                FOREIGN KEY(contact_id) REFERENCES contacts(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_contact_emails_email ON contact_emails(email);
            COMMIT;",
        )
        .map_err(|e| ProtocolError::Plugin(format!("Failed to create tables: {}", e)))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: path_str,
        })
    }

    /// Insert or update a contact
    ///
    /// If contact with same UID exists, update only if timestamp is newer.
    pub async fn upsert_contact(&mut self, contact: Contact) -> Result<i64> {
        debug!(
            "Upserting contact: uid={}, name={:?}, timestamp={}",
            contact.uid, contact.name, contact.timestamp
        );

        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction()
            .map_err(|e| ProtocolError::Plugin(format!("Failed to start transaction: {}", e)))?;

        // Check if contact exists and is newer
        let existing_timestamp: Option<i64> = tx
            .query_row(
                "SELECT timestamp FROM contacts WHERE uid = ? AND device_id = ?",
                params![contact.uid, contact.device_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| ProtocolError::Plugin(format!("Failed to query contact: {}", e)))?;

        if let Some(ts) = existing_timestamp {
            if ts >= contact.timestamp {
                debug!("Contact {} is already up to date or newer", contact.uid);
                // Return existing ID if possible, but we don't have it easily here without another query
                // Just return 0 to indicate no update was needed
                return Ok(0);
            }
        }

        // Insert or update contact
        tx.execute(
            "INSERT INTO contacts (uid, device_id, name, vcard_data, timestamp, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(uid, device_id) DO UPDATE SET
               name = excluded.name,
               vcard_data = excluded.vcard_data,
               timestamp = excluded.timestamp,
               updated_at = excluded.updated_at",
            params![
                contact.uid,
                contact.device_id,
                contact.name,
                contact.vcard_data,
                contact.timestamp,
                chrono::Utc::now().timestamp_millis()
            ],
        )
        .map_err(|e| ProtocolError::Plugin(format!("Failed to upsert contact: {}", e)))?;

        let contact_id: i64 = tx
            .query_row(
                "SELECT id FROM contacts WHERE uid = ? AND device_id = ?",
                params![contact.uid, contact.device_id],
                |row| row.get(0),
            )
            .map_err(|e| ProtocolError::Plugin(format!("Failed to get contact ID: {}", e)))?;

        // Delete existing phones and emails
        tx.execute(
            "DELETE FROM contact_phones WHERE contact_id = ?",
            params![contact_id],
        )
        .map_err(|e| ProtocolError::Plugin(format!("Failed to delete phones: {}", e)))?;

        tx.execute(
            "DELETE FROM contact_emails WHERE contact_id = ?",
            params![contact_id],
        )
        .map_err(|e| ProtocolError::Plugin(format!("Failed to delete emails: {}", e)))?;

        // Insert phones
        for phone in contact.phone_numbers {
            tx.execute(
                "INSERT INTO contact_phones (contact_id, phone_number, phone_type) VALUES (?, ?, ?)",
                params![contact_id, phone.number, phone.phone_type],
            ).map_err(|e| ProtocolError::Plugin(format!("Failed to insert phone: {}", e)))?;
        }

        // Insert emails
        for email in contact.emails {
            tx.execute(
                "INSERT INTO contact_emails (contact_id, email, email_type) VALUES (?, ?, ?)",
                params![contact_id, email.address, email.email_type],
            )
            .map_err(|e| ProtocolError::Plugin(format!("Failed to insert email: {}", e)))?;
        }

        tx.commit()
            .map_err(|e| ProtocolError::Plugin(format!("Failed to commit transaction: {}", e)))?;

        info!(
            "Contact upserted: {} ({})",
            contact.name.as_deref().unwrap_or("Unknown"),
            contact.uid
        );

        Ok(contact_id)
    }

    /// Get contact by UID
    pub async fn get_contact(&self, uid: &str) -> Result<Option<Contact>> {
        debug!("Fetching contact: {}", uid);

        let conn = self.conn.lock().unwrap();

        let contact_row = conn
            .query_row(
                "SELECT id, device_id, name, vcard_data, timestamp FROM contacts WHERE uid = ?",
                params![uid],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| ProtocolError::Plugin(format!("Failed to query contact: {}", e)))?;

        if let Some((id, device_id, name, vcard_data, timestamp)) = contact_row {
            // Fetch phones
            let mut stmt = conn
                .prepare("SELECT phone_number, phone_type FROM contact_phones WHERE contact_id = ?")
                .map_err(|e| {
                    ProtocolError::Plugin(format!("Failed to prepare phone query: {}", e))
                })?;
            let phone_rows = stmt
                .query_map(params![id], |row| {
                    Ok(PhoneNumber {
                        number: row.get(0)?,
                        phone_type: row.get(1)?,
                    })
                })
                .map_err(|e| ProtocolError::Plugin(format!("Failed to query phones: {}", e)))?;

            let mut phone_numbers = Vec::new();
            for phone in phone_rows {
                phone_numbers.push(phone.map_err(|e| {
                    ProtocolError::Plugin(format!("Failed to read phone row: {}", e))
                })?);
            }

            // Fetch emails
            let mut stmt = conn
                .prepare("SELECT email, email_type FROM contact_emails WHERE contact_id = ?")
                .map_err(|e| {
                    ProtocolError::Plugin(format!("Failed to prepare email query: {}", e))
                })?;
            let email_rows = stmt
                .query_map(params![id], |row| {
                    Ok(Email {
                        address: row.get(0)?,
                        email_type: row.get(1)?,
                    })
                })
                .map_err(|e| ProtocolError::Plugin(format!("Failed to query emails: {}", e)))?;

            let mut emails = Vec::new();
            for email in email_rows {
                emails.push(email.map_err(|e| {
                    ProtocolError::Plugin(format!("Failed to read email row: {}", e))
                })?);
            }

            Ok(Some(Contact {
                uid: uid.to_string(),
                device_id,
                name,
                vcard_data,
                timestamp,
                phone_numbers,
                emails,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get all contacts for a device
    pub async fn get_contacts_by_device(&self, device_id: &str) -> Result<Vec<Contact>> {
        debug!("Fetching contacts for device: {}", device_id);

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT uid FROM contacts WHERE device_id = ? ORDER BY name ASC")
            .map_err(|e| {
                ProtocolError::Plugin(format!("Failed to prepare contacts query: {}", e))
            })?;

        let uids_iter = stmt
            .query_map(params![device_id], |row| row.get::<_, String>(0))
            .map_err(|e| ProtocolError::Plugin(format!("Failed to query contact UIDs: {}", e)))?;

        let uids: Vec<String> = uids_iter
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| ProtocolError::Plugin(format!("Failed to collect UIDs: {}", e)))?;
        drop(stmt);
        drop(conn); // Release lock

        let mut contacts = Vec::new();
        for uid in uids {
            if let Some(contact) = self.get_contact(&uid).await? {
                contacts.push(contact);
            }
        }

        Ok(contacts)
    }

    /// Get all contacts
    pub async fn get_all_contacts(&self) -> Result<Vec<Contact>> {
        debug!("Fetching all contacts");

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT uid FROM contacts ORDER BY name ASC")
            .map_err(|e| {
                ProtocolError::Plugin(format!("Failed to prepare contacts query: {}", e))
            })?;

        let uids: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| ProtocolError::Plugin(format!("Failed to query contact UIDs: {}", e)))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| ProtocolError::Plugin(format!("Failed to collect UIDs: {}", e)))?;

        drop(stmt);
        drop(conn);

        let mut contacts = Vec::new();
        for uid in uids {
            if let Some(contact) = self.get_contact(&uid).await? {
                contacts.push(contact);
            }
        }

        Ok(contacts)
    }

    /// Delete contact by UID
    pub async fn delete_contact(&mut self, uid: &str) -> Result<bool> {
        debug!("Deleting contact: {}", uid);

        let conn = self.conn.lock().unwrap();
        let count = conn
            .execute("DELETE FROM contacts WHERE uid = ?", params![uid])
            .map_err(|e| ProtocolError::Plugin(format!("Failed to delete contact: {}", e)))?;

        Ok(count > 0)
    }

    /// Delete all contacts for a device
    pub async fn delete_device_contacts(&mut self, device_id: &str) -> Result<usize> {
        debug!("Deleting all contacts for device: {}", device_id);

        let conn = self.conn.lock().unwrap();
        let count = conn
            .execute(
                "DELETE FROM contacts WHERE device_id = ?",
                params![device_id],
            )
            .map_err(|e| {
                ProtocolError::Plugin(format!("Failed to delete device contacts: {}", e))
            })?;

        Ok(count)
    }

    /// Search contacts by name
    pub async fn search_contacts(&self, query: &str) -> Result<Vec<Contact>> {
        debug!("Searching contacts: {}", query);
        let search_pattern = format!("%{}%", query);

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT c.uid 
             FROM contacts c
             LEFT JOIN contact_phones p ON c.id = p.contact_id
             LEFT JOIN contact_emails e ON c.id = e.contact_id
             WHERE c.name LIKE ?1 
                OR p.phone_number LIKE ?1 
                OR e.email LIKE ?1
             ORDER BY c.name ASC",
            )
            .map_err(|e| ProtocolError::Plugin(format!("Failed to prepare search query: {}", e)))?;

        let uids: Vec<String> = stmt
            .query_map(params![search_pattern], |row| row.get(0))
            .map_err(|e| ProtocolError::Plugin(format!("Failed to query search UIDs: {}", e)))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| ProtocolError::Plugin(format!("Failed to collect search UIDs: {}", e)))?;

        drop(stmt);
        drop(conn);

        let mut contacts = Vec::new();
        for uid in uids {
            if let Some(contact) = self.get_contact(&uid).await? {
                contacts.push(contact);
            }
        }

        Ok(contacts)
    }

    /// Get contact count
    pub async fn get_contact_count(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM contacts", [], |row| row.get(0))
            .map_err(|e| ProtocolError::Plugin(format!("Failed to count contacts: {}", e)))?;

        Ok(count as usize)
    }

    /// Get database path
    pub fn db_path(&self) -> &str {
        &self.db_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_database_creation() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_contacts.db");

        let db = ContactsDatabase::new(&db_path).await;
        assert!(db.is_ok());
    }

    #[tokio::test]
    async fn test_contact_structure() {
        let contact = Contact {
            uid: "test-123".to_string(),
            device_id: "device-456".to_string(),
            name: Some("Test User".to_string()),
            vcard_data: "BEGIN:VCARD\nEND:VCARD".to_string(),
            timestamp: 1234567890000,
            phone_numbers: vec![PhoneNumber {
                number: "+1234567890".to_string(),
                phone_type: Some("CELL".to_string()),
            }],
            emails: vec![Email {
                address: "test@example.com".to_string(),
                email_type: Some("HOME".to_string()),
            }],
        };

        assert_eq!(contact.uid, "test-123");
        assert_eq!(contact.phone_numbers.len(), 1);
        assert_eq!(contact.emails.len(), 1);
    }
}
