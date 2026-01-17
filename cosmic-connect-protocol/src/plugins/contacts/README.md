# Contacts Plugin - Database Storage & DBus Signals

## Current Status

✅ **Completed:**
- Database module interface (`database.rs`)
- DBus signals module (`signals.rs`)
- Integration with Contacts plugin
- vCard parsing and storage logic
- Event emission for contact changes
- Comprehensive test structure

⏳ **Pending: SQLite Database Implementation**

The database module currently provides a **stub implementation** that logs operations but doesn't persist data. Full implementation requires adding SQLite dependency.

## Architecture

```
Mobile Device  →  Contacts Plugin  →  Database (SQLite)
                          ↓
                    DBus Signals  →  Desktop Apps (GNOME Contacts, etc.)
```

### Data Flow

1. **Sync Request**: Desktop requests contact UIDs with timestamps
2. **Response Handling**: Plugin receives UID/timestamp pairs, identifies new/updated contacts
3. **vCard Request**: Plugin requests full vCard data for changed contacts
4. **Parsing & Storage**:
   - Parse vCard to extract name, phones, emails
   - Store to SQLite database (if enabled)
   - Emit DBus signals for UI updates
5. **UI Integration**: Desktop apps listen for DBus signals and update their UI

## Database Schema

### contacts table
```sql
CREATE TABLE contacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uid TEXT UNIQUE NOT NULL,
    device_id TEXT NOT NULL,
    name TEXT,
    vcard_data TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_contacts_device ON contacts(device_id);
CREATE INDEX idx_contacts_name ON contacts(name);
```

### contact_phones table
```sql
CREATE TABLE contact_phones (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    contact_id INTEGER NOT NULL,
    phone_number TEXT NOT NULL,
    phone_type TEXT,
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

CREATE INDEX idx_phones_contact ON contact_phones(contact_id);
```

### contact_emails table
```sql
CREATE TABLE contact_emails (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    contact_id INTEGER NOT NULL,
    email TEXT NOT NULL,
    email_type TEXT,
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
);

CREATE INDEX idx_emails_contact ON contact_emails(contact_id);
```

## DBus Interface

### Service Details

- **Service Name**: `org.cosmic.Connect`
- **Object Path**: `/org/cosmic/Connect/Contacts`
- **Interface**: `org.cosmic.Connect.Contacts`

### Signals

#### ContactAdded
Emitted when a new contact is synced from mobile device.

**Signature**: `ContactAdded(device_id: String, uid: String, name: String)`

**Example**:
```python
bus.add_signal_receiver(
    handler_func,
    signal_name="ContactAdded",
    dbus_interface="org.cosmic.Connect.Contacts",
    path="/org/cosmic/Connect/Contacts"
)
```

#### ContactUpdated
Emitted when an existing contact is updated.

**Signature**: `ContactUpdated(device_id: String, uid: String, name: String)`

#### ContactDeleted
Emitted when a contact is removed.

**Signature**: `ContactDeleted(device_id: String, uid: String)`

#### SyncCompleted
Emitted when a full sync operation finishes.

**Signature**: `SyncCompleted(device_id: String, total: u32, added: u32, updated: u32)`

## Implementation Guide

### Step 1: Add SQLite Dependency

Update `cosmic-connect-protocol/Cargo.toml`:

```toml
[dependencies]
# Existing dependencies...

# SQLite database (optional, behind feature flag)
rusqlite = { version = "0.32", features = ["bundled"], optional = true }

[features]
default = []
contacts-database = ["rusqlite"]
```

### Step 2: Implement Database Methods

Replace stub methods in `database.rs`:

```rust
use rusqlite::{Connection, params, OptionalExtension};

pub struct ContactsDatabase {
    conn: Connection,
}

impl ContactsDatabase {
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create tables
        conn.execute_batch(include_str!("schema.sql"))?;

        Ok(Self { conn })
    }

    pub async fn upsert_contact(&mut self, contact: Contact) -> Result<i64> {
        let now = chrono::Utc::now().timestamp_millis();

        self.conn.execute(
            "INSERT INTO contacts (uid, device_id, name, vcard_data, timestamp, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(uid) DO UPDATE SET
                name = excluded.name,
                vcard_data = excluded.vcard_data,
                timestamp = excluded.timestamp,
                updated_at = excluded.updated_at
             WHERE excluded.timestamp > contacts.timestamp",
            params![
                contact.uid,
                contact.device_id,
                contact.name,
                contact.vcard_data,
                contact.timestamp,
                now
            ],
        )?;

        let contact_id = self.conn.last_insert_rowid();

        // Delete old phones/emails
        self.conn.execute("DELETE FROM contact_phones WHERE contact_id = ?1", params![contact_id])?;
        self.conn.execute("DELETE FROM contact_emails WHERE contact_id = ?1", params![contact_id])?;

        // Insert phones
        for phone in contact.phone_numbers {
            self.conn.execute(
                "INSERT INTO contact_phones (contact_id, phone_number, phone_type) VALUES (?1, ?2, ?3)",
                params![contact_id, phone.number, phone.phone_type],
            )?;
        }

        // Insert emails
        for email in contact.emails {
            self.conn.execute(
                "INSERT INTO contact_emails (contact_id, email, email_type) VALUES (?1, ?2, ?3)",
                params![contact_id, email.address, email.email_type],
            )?;
        }

        Ok(contact_id)
    }

    pub async fn get_contact(&self, uid: &str) -> Result<Option<Contact>> {
        let contact: Option<(String, String, Option<String>, String, i64)> = self.conn
            .query_row(
                "SELECT uid, device_id, name, vcard_data, timestamp FROM contacts WHERE uid = ?1",
                params![uid],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;

        // ... fetch phones and emails, construct Contact

        Ok(contact.map(|_| Contact { /* ... */ }))
    }
}
```

### Step 3: Implement DBus Signals

In `cosmic-connect-daemon/src/main.rs`, integrate with zbus:

```rust
use zbus::{Connection, interface, SignalContext};

#[derive(Clone)]
struct ContactsInterface;

#[interface(name = "org.cosmic.Connect.Contacts")]
impl ContactsInterface {
    #[zbus(signal)]
    async fn contact_added(
        signal_ctx: &SignalContext<'_>,
        device_id: &str,
        uid: &str,
        name: &str
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn contact_updated(
        signal_ctx: &SignalContext<'_>,
        device_id: &str,
        uid: &str,
        name: &str
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn contact_deleted(
        signal_ctx: &SignalContext<'_>,
        device_id: &str,
        uid: &str
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn sync_completed(
        signal_ctx: &SignalContext<'_>,
        device_id: &str,
        total: u32,
        added: u32,
        updated: u32
    ) -> zbus::Result<()>;
}

// In daemon initialization:
async fn init_contacts_dbus(conn: &Connection) -> Result<()> {
    conn.object_server()
        .at("/org/cosmic/Connect/Contacts", ContactsInterface)
        .await?;

    Ok(())
}
```

Then update `signals.rs` to use the real zbus connection:

```rust
pub struct ContactsSignals {
    connection: zbus::Connection,
}

impl ContactsSignals {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let connection = zbus::Connection::session().await?;
        Ok(Self { connection })
    }

    pub async fn contact_added(&self, device_id: &str, uid: &str, name: &str)
        -> Result<(), Box<dyn std::error::Error>>
    {
        let iface_ref = self.connection
            .object_server()
            .interface::<_, ContactsInterface>("/org/cosmic/Connect/Contacts")
            .await?;

        ContactsInterface::contact_added(
            iface_ref.signal_context(),
            device_id,
            uid,
            name
        ).await?;

        Ok(())
    }
}
```

### Step 4: Initialize in Plugin

In plugin initialization (daemon):

```rust
let mut contacts_plugin = ContactsPlugin::new();

// Initialize database
let db_path = config_dir.join("contacts.db");
contacts_plugin.init_database(db_path.to_str().unwrap()).await?;

// Initialize DBus signals
contacts_plugin.init_signals().await?;
```

## Usage Examples

### Listening for Contact Changes (Python)

```python
#!/usr/bin/env python3
import dbus
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DBusGMainLoop(set_as_default=True)

def contact_added(device_id, uid, name):
    print(f"✓ New contact: {name}")
    print(f"  Device: {device_id}")
    print(f"  UID: {uid}")

def sync_completed(device_id, total, added, updated):
    print(f"🔄 Sync completed for {device_id}")
    print(f"  Total: {total}, Added: {added}, Updated: {updated}")

bus = dbus.SessionBus()

bus.add_signal_receiver(
    contact_added,
    signal_name="ContactAdded",
    dbus_interface="org.cosmic.Connect.Contacts",
    path="/org/cosmic/Connect/Contacts"
)

bus.add_signal_receiver(
    sync_completed,
    signal_name="SyncCompleted",
    dbus_interface="org.cosmic.Connect.Contacts",
    path="/org/cosmic/Connect/Contacts"
)

print("Listening for contact changes...")
loop = GLib.MainLoop()
loop.run()
```

### Querying Database (Rust)

```rust
use cosmic_connect_protocol::plugins::contacts::database::ContactsDatabase;

let db = ContactsDatabase::new("~/.config/cosmic-connect/contacts.db").await?;

// Get all contacts
let contacts = db.get_all_contacts().await?;
for contact in contacts {
    println!("{}: {} phone(s), {} email(s)",
        contact.name.unwrap_or_else(|| "Unknown".to_string()),
        contact.phone_numbers.len(),
        contact.emails.len()
    );
}

// Search contacts
let results = db.search_contacts("john").await?;
println!("Found {} contacts matching 'john'", results.len());
```

## Integration with Desktop Apps

### GNOME Contacts
GNOME Contacts can subscribe to DBus signals and update its UI in real-time:

```javascript
const ContactsService = imports.gi.Folks;

// Listen for COSMIC Connect contact updates
const proxy = new Gio.DBusProxy({
    g_bus_type: Gio.BusType.SESSION,
    g_name: 'org.cosmic.Connect',
    g_object_path: '/org/cosmic/Connect/Contacts',
    g_interface_name: 'org.cosmic.Connect.Contacts',
});

proxy.connectSignal('ContactAdded', (proxy, sender, [device_id, uid, name]) => {
    // Update GNOME Contacts UI
    refreshContactsList();
});
```

### KDE Contacts (Akonadi)
Create an Akonadi resource to import from COSMIC Connect database:

```cpp
class CosmicConnectResource : public Akonadi::ResourceBase {
    void retrieveItems(const Akonadi::Collection& collection) {
        // Read from COSMIC Connect SQLite database
        // Convert to Akonadi contacts
        // Emit itemsRetrieved()
    }
};
```

## Testing

### Manual Testing

1. **Start daemon with debug logging:**
   ```bash
   RUST_LOG=debug cosmic-connect-daemon
   ```

2. **Trigger contact sync from mobile device**

3. **Check logs for:**
   ```
   [INFO] Contact added: John Doe (contact-123)
   [DEBUG] Contact contact-123 stored to database
   [INFO] Emitting ContactAdded signal: device=device-456, uid=contact-123, name=John Doe
   ```

4. **Query database:**
   ```bash
   sqlite3 ~/.config/cosmic-connect/contacts.db
   SELECT name, COUNT(*) as phones FROM contacts
   JOIN contact_phones ON contacts.id = contact_phones.contact_id
   GROUP BY contacts.id;
   ```

5. **Monitor DBus signals:**
   ```bash
   dbus-monitor "interface='org.cosmic.Connect.Contacts'"
   ```

## Security & Privacy

### Data Protection
- Database stored in user config directory (`~/.config/cosmic-connect/`)
- File permissions: 0600 (user read/write only)
- No network access - local storage only

### GDPR Compliance
- User can delete all contacts: `ContactsDatabase::delete_device_contacts()`
- Export contacts: Query database and export as vCard
- Clear cache: `ContactsPlugin::clear_cache()`

## Performance Considerations

- **Batch Operations**: Use transactions for bulk inserts
- **Indexing**: Create indexes on frequently queried columns (device_id, name)
- **Lazy Loading**: Only fetch phones/emails when needed
- **Cache**: Keep recent contacts in memory

## Future Enhancements

1. **Conflict Resolution**: Handle concurrent updates from multiple devices
2. **Photo Sync**: Store contact photos from vCard PHOTO field
3. **Groups**: Support vCard groups/categories
4. **Merge Detection**: Identify duplicate contacts across devices
5. **Export/Import**: vCard file import/export functionality

## References

- [vCard 2.1 Specification](https://www.w3.org/TR/vcard-rdf/)
- [DBus Specification](https://dbus.freedesktop.org/doc/dbus-specification.html)
- [rusqlite Documentation](https://docs.rs/rusqlite/)
- [zbus Documentation](https://docs.rs/zbus/)
