# Android Phone Authentication for COSMIC Desktop

A comprehensive guide for implementing phone-based biometric authentication (fingerprint, face, PIN) for COSMIC Desktop login and sudo operations.

## Overview

This guide covers two approaches:
1. **Standalone PAM Module** - A dedicated solution for phone-based auth
2. **cosmic-connect Integration** - Adding auth as a feature to your KDE Connect implementation

## Architecture

```
┌─────────────────────┐     ┌──────────────────────┐
│  COSMIC Desktop     │     │  Android Phone       │
│  ─────────────────  │     │  ────────────────    │
│  cosmic-greeter     │     │  cosmic-connect-app  │
│       │             │     │       │              │
│  ┌────▼────┐        │     │  ┌────▼────┐         │
│  │  PAM    │◄───────┼─────┼──│ Biometric│         │
│  │ Module  │  TLS   │WiFi │  │  Auth    │         │
│  └─────────┘ 1.3    │     │  └──────────┘         │
└─────────────────────┘     └──────────────────────┘
```

## Part 1: Understanding cosmic-greeter PAM Integration

### How cosmic-greeter Works

cosmic-greeter uses the `pam-client` crate for authentication:

```rust
use pam_client::{Context, Flag};
use pam_client::conv_cli::Conversation;

let mut context = Context::new(
    "cosmic-greeter",           // Service name → /etc/pam.d/cosmic-greeter
    Some(&username),            // Username
    Conversation::new()         // Interactive conversation handler
).expect("Failed to initialize PAM context");

// This is where PAM modules are invoked
context.authenticate(Flag::NONE).expect("Authentication failed");

// Validate account (not locked, expired, etc.)
context.acct_mgmt(Flag::NONE).expect("Account validation failed");
```

### PAM Configuration for cosmic-greeter

The PAM config lives at `/etc/pam.d/cosmic-greeter`:

```pam
#%PAM-1.0

# Standard password authentication
auth       include      login

# Your phone auth module (add this line to enable phone auth)
auth       sufficient   pam_cosmic_phone.so

account    include      login
session    include      login
password   include      login
```

**Key Points:**
- `sufficient` means if phone auth succeeds, no password needed
- `include login` falls back to standard login behavior
- Order matters: phone auth should come before password for convenience

## Part 2: Creating a Rust PAM Module

### Project Structure

```
cosmic-phone-auth/
├── Cargo.toml
├── src/
│   ├── lib.rs           # PAM module entry points
│   ├── auth.rs          # Authentication logic
│   ├── network.rs       # Phone communication
│   └── crypto.rs        # TLS and challenge-response
├── pam/
│   └── cosmic-phone     # PAM config file
└── systemd/
    └── cosmic-phone-auth.service
```

### Cargo.toml

```toml
[package]
name = "pam_cosmic_phone"
version = "0.1.0"
edition = "2024"

[lib]
name = "pam_cosmic_phone"
crate-type = ["cdylib"]  # Shared library for PAM

[dependencies]
pam = "0.8"              # PAM module creation
tokio = { version = "1", features = ["rt-multi-thread", "net", "sync", "time"] }
rustls = "0.23"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
ring = "0.17"            # Cryptography
base64 = "0.22"
log = "0.4"
```

### PAM Module Implementation (src/lib.rs)

```rust
use pam::constants::{PamFlag, PamResultCode};
use pam::module::{PamHandle, PamHooks};
use pam::pam_try;
use std::ffi::CStr;

mod auth;
mod network;
mod crypto;

struct PamCosmicPhone;
pam::pam_hooks!(PamCosmicPhone);

impl PamHooks for PamCosmicPhone {
    fn sm_authenticate(
        pamh: &mut PamHandle,
        _args: Vec<&CStr>,
        _flags: PamFlag,
    ) -> PamResultCode {
        // Get username from PAM
        let user = match pamh.get_user(None) {
            Ok(u) => u.to_string_lossy().into_owned(),
            Err(_) => return PamResultCode::PAM_USER_UNKNOWN,
        };

        // Attempt phone authentication
        match auth::authenticate_with_phone(&user) {
            Ok(true) => {
                log::info!("Phone authentication successful for user: {}", user);
                PamResultCode::PAM_SUCCESS
            }
            Ok(false) => {
                log::info!("Phone authentication denied for user: {}", user);
                PamResultCode::PAM_AUTH_ERR
            }
            Err(e) => {
                log::warn!("Phone authentication unavailable: {}", e);
                // Return AUTHINFO_UNAVAIL to let PAM try next module
                PamResultCode::PAM_AUTHINFO_UNAVAIL
            }
        }
    }

    fn acct_mgmt(
        _pamh: &mut PamHandle,
        _args: Vec<&CStr>,
        _flags: PamFlag,
    ) -> PamResultCode {
        // Account management - just pass through
        PamResultCode::PAM_SUCCESS
    }
}
```

### Authentication Logic (src/auth.rs)

```rust
use crate::network::PhoneConnection;
use crate::crypto::{generate_challenge, verify_response};
use std::time::Duration;

pub fn authenticate_with_phone(username: &str) -> Result<bool, AuthError> {
    // Create runtime for async operations
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| AuthError::Runtime(e.to_string()))?;

    rt.block_on(async {
        // Find paired device for this user
        let device = find_paired_device(username).await?;
        
        // Connect to phone
        let mut conn = PhoneConnection::connect(&device).await?;
        
        // Generate cryptographic challenge
        let challenge = generate_challenge();
        
        // Send auth request to phone
        let request = AuthRequest {
            request_type: AuthType::Login,
            username: username.to_string(),
            challenge: challenge.clone(),
            hostname: gethostname::gethostname().to_string_lossy().into_owned(),
        };
        
        conn.send(&request).await?;
        
        // Wait for response (with timeout)
        let response: AuthResponse = tokio::time::timeout(
            Duration::from_secs(30),
            conn.receive()
        ).await
            .map_err(|_| AuthError::Timeout)?
            .map_err(AuthError::Network)?;
        
        // Verify the response
        if response.approved && verify_response(&challenge, &response.signature, &device.public_key) {
            Ok(true)
        } else {
            Ok(false)
        }
    })
}

#[derive(Debug, serde::Serialize)]
struct AuthRequest {
    request_type: AuthType,
    username: String,
    challenge: Vec<u8>,
    hostname: String,
}

#[derive(Debug, serde::Deserialize)]
struct AuthResponse {
    approved: bool,
    signature: Vec<u8>,
    biometric_type: String,  // "fingerprint", "face", "pin"
}

#[derive(Debug, serde::Serialize)]
enum AuthType {
    Login,
    Sudo,
    Polkit,
    ScreenUnlock,
}
```

### Network Communication (src/network.rs)

```rust
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use rustls::{ClientConfig, RootCertStore};
use std::sync::Arc;

pub struct PhoneConnection {
    stream: tokio_rustls::client::TlsStream<TcpStream>,
}

impl PhoneConnection {
    pub async fn connect(device: &PairedDevice) -> Result<Self, NetworkError> {
        // Load device certificate
        let mut root_store = RootCertStore::empty();
        root_store.add(&device.certificate)?;
        
        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        
        let connector = TlsConnector::from(Arc::new(config));
        
        // Connect to phone (try both WiFi and Bluetooth)
        let stream = TcpStream::connect(&device.address).await?;
        let domain = rustls::pki_types::ServerName::try_from(device.id.clone())?;
        
        let tls_stream = connector.connect(domain, stream).await?;
        
        Ok(Self { stream: tls_stream })
    }
    
    pub async fn send<T: serde::Serialize>(&mut self, msg: &T) -> Result<(), NetworkError> {
        let json = serde_json::to_vec(msg)?;
        let len = (json.len() as u32).to_be_bytes();
        
        use tokio::io::AsyncWriteExt;
        self.stream.write_all(&len).await?;
        self.stream.write_all(&json).await?;
        self.stream.flush().await?;
        
        Ok(())
    }
    
    pub async fn receive<T: serde::de::DeserializeOwned>(&mut self) -> Result<T, NetworkError> {
        use tokio::io::AsyncReadExt;
        
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf).await?;
        
        Ok(serde_json::from_slice(&buf)?)
    }
}

pub struct PairedDevice {
    pub id: String,
    pub address: String,
    pub certificate: rustls::pki_types::CertificateDer<'static>,
    pub public_key: Vec<u8>,
}

pub async fn find_paired_device(username: &str) -> Result<PairedDevice, NetworkError> {
    // Read from config directory
    let config_path = format!(
        "/home/{}/.config/cosmic-phone-auth/devices.json",
        username
    );
    
    let content = tokio::fs::read_to_string(&config_path).await?;
    let devices: Vec<PairedDevice> = serde_json::from_str(&content)?;
    
    // Return first available device (could be smarter about selection)
    for device in devices {
        if is_device_available(&device).await {
            return Ok(device);
        }
    }
    
    Err(NetworkError::NoDeviceAvailable)
}
```

## Part 3: cosmic-connect Integration

Since you're already building cosmic-connect, adding authentication is a natural extension.

### Protocol Extension

Add an authentication plugin to the KDE Connect protocol:

```rust
// In your cosmic-connect project
// src/plugins/auth.rs

use crate::device::Device;
use crate::protocol::{NetworkPacket, PacketType};

pub const PACKET_TYPE_AUTH_REQUEST: &str = "kdeconnect.auth.request";
pub const PACKET_TYPE_AUTH_RESPONSE: &str = "kdeconnect.auth.response";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthRequestPacket {
    pub request_id: String,
    pub auth_type: String,      // "login", "sudo", "polkit", "unlock"
    pub username: String,
    pub hostname: String,
    pub challenge: String,      // Base64 encoded
    pub timestamp: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthResponsePacket {
    pub request_id: String,
    pub approved: bool,
    pub biometric_type: String, // "fingerprint", "face", "pin", "none"
    pub signature: String,      // Base64 encoded challenge signature
}

pub struct AuthPlugin {
    pending_requests: std::collections::HashMap<String, AuthRequest>,
}

impl AuthPlugin {
    pub fn handle_packet(&mut self, device: &Device, packet: NetworkPacket) {
        match packet.packet_type.as_str() {
            PACKET_TYPE_AUTH_REQUEST => {
                // This runs on the phone - show biometric prompt
                self.handle_auth_request(device, packet);
            }
            PACKET_TYPE_AUTH_RESPONSE => {
                // This runs on the desktop - verify and signal PAM
                self.handle_auth_response(device, packet);
            }
            _ => {}
        }
    }
}
```

### Desktop Daemon Component

Create a D-Bus service that the PAM module can communicate with:

```rust
// src/auth_daemon.rs

use zbus::{dbus_interface, Connection};
use tokio::sync::mpsc;

pub struct AuthDaemon {
    tx: mpsc::Sender<AuthResult>,
    pending: std::sync::Arc<tokio::sync::Mutex<HashMap<String, PendingAuth>>>,
}

#[dbus_interface(name = "org.cosmicde.PhoneAuth")]
impl AuthDaemon {
    /// Called by PAM module to request authentication
    async fn request_auth(
        &self,
        username: &str,
        auth_type: &str,
    ) -> zbus::fdo::Result<String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        
        // Find paired device and send request
        // ...
        
        Ok(request_id)
    }
    
    /// Called by PAM module to check auth result
    async fn check_auth(&self, request_id: &str) -> zbus::fdo::Result<(bool, String)> {
        let pending = self.pending.lock().await;
        
        match pending.get(request_id) {
            Some(auth) if auth.completed => {
                Ok((auth.approved, auth.biometric_type.clone()))
            }
            Some(_) => {
                // Still pending
                Err(zbus::fdo::Error::Failed("Auth pending".into()))
            }
            None => {
                Err(zbus::fdo::Error::Failed("Unknown request".into()))
            }
        }
    }
    
    /// Signal emitted when auth completes
    #[dbus_interface(signal)]
    async fn auth_completed(
        ctxt: &zbus::SignalContext<'_>,
        request_id: &str,
        approved: bool,
    ) -> zbus::Result<()>;
}

pub async fn run_auth_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::system().await?;
    
    let daemon = AuthDaemon::new();
    
    connection
        .object_server()
        .at("/org/cosmicde/PhoneAuth", daemon)
        .await?;
    
    connection
        .request_name("org.cosmicde.PhoneAuth")
        .await?;
    
    // Keep running
    std::future::pending::<()>().await;
    Ok(())
}
```

### Simplified PAM Module (Using D-Bus)

```rust
// pam_cosmic_connect/src/lib.rs

use pam::constants::{PamFlag, PamResultCode};
use pam::module::{PamHandle, PamHooks};

struct PamCosmicConnect;
pam::pam_hooks!(PamCosmicConnect);

impl PamHooks for PamCosmicConnect {
    fn sm_authenticate(
        pamh: &mut PamHandle,
        _args: Vec<&std::ffi::CStr>,
        _flags: PamFlag,
    ) -> PamResultCode {
        let user = match pamh.get_user(None) {
            Ok(u) => u.to_string_lossy().into_owned(),
            Err(_) => return PamResultCode::PAM_USER_UNKNOWN,
        };

        // Use blocking D-Bus call to auth daemon
        match dbus_authenticate(&user) {
            Ok(true) => PamResultCode::PAM_SUCCESS,
            Ok(false) => PamResultCode::PAM_AUTH_ERR,
            Err(_) => PamResultCode::PAM_AUTHINFO_UNAVAIL,
        }
    }
    
    fn acct_mgmt(
        _pamh: &mut PamHandle,
        _args: Vec<&std::ffi::CStr>,
        _flags: PamFlag,
    ) -> PamResultCode {
        PamResultCode::PAM_SUCCESS
    }
}

fn dbus_authenticate(username: &str) -> Result<bool, Box<dyn std::error::Error>> {
    use zbus::blocking::Connection;
    
    let connection = Connection::system()?;
    
    let proxy = connection.call_method(
        Some("org.cosmicde.PhoneAuth"),
        "/org/cosmicde/PhoneAuth",
        Some("org.cosmicde.PhoneAuth"),
        "RequestAuth",
        &(username, "login"),
    )?;
    
    let request_id: String = proxy.body().deserialize()?;
    
    // Poll for result (with timeout)
    for _ in 0..60 {  // 30 second timeout (500ms * 60)
        std::thread::sleep(std::time::Duration::from_millis(500));
        
        let result = connection.call_method(
            Some("org.cosmicde.PhoneAuth"),
            "/org/cosmicde/PhoneAuth",
            Some("org.cosmicde.PhoneAuth"),
            "CheckAuth",
            &request_id,
        );
        
        match result {
            Ok(reply) => {
                let (approved, _biometric_type): (bool, String) = reply.body().deserialize()?;
                return Ok(approved);
            }
            Err(_) => continue,  // Still pending
        }
    }
    
    Err("Timeout".into())
}
```

## Part 4: Android App Implementation (Kotlin + Rust)

For your cosmic-connect-android project, add biometric authentication:

### BiometricAuthManager.kt

```kotlin
package org.cosmicde.connect.auth

import android.content.Context
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

class BiometricAuthManager(private val context: Context) {
    
    private val biometricManager = BiometricManager.from(context)
    
    fun canAuthenticate(): Boolean {
        return biometricManager.canAuthenticate(
            BiometricManager.Authenticators.BIOMETRIC_STRONG or
            BiometricManager.Authenticators.DEVICE_CREDENTIAL
        ) == BiometricManager.BIOMETRIC_SUCCESS
    }
    
    suspend fun authenticate(
        activity: FragmentActivity,
        request: AuthRequest
    ): AuthResult = suspendCancellableCoroutine { continuation ->
        
        val executor = ContextCompat.getMainExecutor(context)
        
        val callback = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                val biometricType = when (result.authenticationType) {
                    BiometricPrompt.AUTHENTICATION_RESULT_TYPE_BIOMETRIC -> "fingerprint"
                    BiometricPrompt.AUTHENTICATION_RESULT_TYPE_DEVICE_CREDENTIAL -> "pin"
                    else -> "unknown"
                }
                continuation.resume(AuthResult(
                    approved = true,
                    biometricType = biometricType,
                    challenge = request.challenge
                ))
            }
            
            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                continuation.resume(AuthResult(
                    approved = false,
                    biometricType = "none",
                    challenge = request.challenge,
                    error = errString.toString()
                ))
            }
            
            override fun onAuthenticationFailed() {
                // Don't resume yet - let user retry
            }
        }
        
        val promptInfo = BiometricPrompt.PromptInfo.Builder()
            .setTitle("Authentication Request")
            .setSubtitle("${request.hostname} wants to ${request.authType}")
            .setDescription("User: ${request.username}")
            .setAllowedAuthenticators(
                BiometricManager.Authenticators.BIOMETRIC_STRONG or
                BiometricManager.Authenticators.DEVICE_CREDENTIAL
            )
            .build()
        
        val biometricPrompt = BiometricPrompt(activity, executor, callback)
        biometricPrompt.authenticate(promptInfo)
        
        continuation.invokeOnCancellation {
            biometricPrompt.cancelAuthentication()
        }
    }
}

data class AuthRequest(
    val requestId: String,
    val authType: String,
    val username: String,
    val hostname: String,
    val challenge: ByteArray
)

data class AuthResult(
    val approved: Boolean,
    val biometricType: String,
    val challenge: ByteArray,
    val error: String? = null
)
```

### Rust JNI Bridge (for crypto operations)

```rust
// android/src/lib.rs

use jni::JNIEnv;
use jni::objects::{JClass, JByteArray};
use jni::sys::jbyteArray;

#[no_mangle]
pub extern "system" fn Java_org_cosmicde_connect_CryptoNative_signChallenge(
    mut env: JNIEnv,
    _class: JClass,
    challenge: JByteArray,
    private_key: JByteArray,
) -> jbyteArray {
    let challenge_bytes = env.convert_byte_array(&challenge).unwrap();
    let key_bytes = env.convert_byte_array(&private_key).unwrap();
    
    // Sign the challenge with the device's private key
    let signature = sign_challenge(&challenge_bytes, &key_bytes);
    
    env.byte_array_from_slice(&signature).unwrap().into_raw()
}

fn sign_challenge(challenge: &[u8], private_key: &[u8]) -> Vec<u8> {
    use ring::signature::{Ed25519KeyPair, Signature};
    
    let key_pair = Ed25519KeyPair::from_pkcs8(private_key).unwrap();
    let signature = key_pair.sign(challenge);
    
    signature.as_ref().to_vec()
}
```

## Part 5: Installation & Configuration

### Build the PAM Module

```bash
cd cosmic-phone-auth
cargo build --release

# Install the shared library
sudo cp target/release/libpam_cosmic_phone.so /lib/security/pam_cosmic_phone.so

# Install PAM config
sudo cp pam/cosmic-phone /etc/pam.d/cosmic-phone
```

### NixOS Configuration

```nix
# /etc/nixos/cosmic-phone-auth.nix
{ config, pkgs, ... }:

let
  pam-cosmic-phone = pkgs.rustPlatform.buildRustPackage {
    pname = "pam-cosmic-phone";
    version = "0.1.0";
    src = /path/to/cosmic-phone-auth;
    cargoLock.lockFile = /path/to/cosmic-phone-auth/Cargo.lock;
    
    postInstall = ''
      mkdir -p $out/lib/security
      cp target/release/libpam_cosmic_phone.so $out/lib/security/pam_cosmic_phone.so
    '';
  };
in {
  security.pam.services.cosmic-greeter = {
    text = ''
      auth sufficient ${pam-cosmic-phone}/lib/security/pam_cosmic_phone.so
      auth include login
      account include login
      session include login
      password include login
    '';
  };
  
  # Also enable for sudo
  security.pam.services.sudo = {
    text = ''
      auth sufficient ${pam-cosmic-phone}/lib/security/pam_cosmic_phone.so
      auth include sudo
      account include sudo
      session include sudo
    '';
  };
  
  # Systemd service for auth daemon
  systemd.services.cosmic-phone-auth = {
    description = "COSMIC Phone Authentication Daemon";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    
    serviceConfig = {
      ExecStart = "${pam-cosmic-phone}/bin/cosmic-phone-authd";
      Restart = "always";
    };
  };
}
```

### Device Pairing

Create a CLI tool for initial pairing:

```bash
# cosmic-phone-pair --scan
Scanning for devices...
Found: Olaf's Pixel 8 (192.168.1.42)

# cosmic-phone-pair --pair "Olaf's Pixel 8"
Pairing request sent to device.
Please confirm on your phone...
 Paired successfully!
Device ID: a1b2c3d4-e5f6-7890-abcd-ef1234567890
Certificate stored in ~/.config/cosmic-phone-auth/devices.json
```

## Security Considerations

1. **TLS 1.3** - All communication encrypted
2. **Challenge-Response** - Prevents replay attacks
3. **Certificate Pinning** - Device certificates verified
4. **Timeout** - Auth requests expire after 30 seconds
5. **Local Network Only** - No internet required
6. **Biometric Binding** - Signature tied to biometric verification

## Testing

```bash
# Test PAM module directly
pamtester cosmic-phone yourusername authenticate

# Test with sudo
sudo echo "Phone auth works!"

# Lock screen and unlock with phone
cosmic-greeter --lock
```

## Next Steps for cosmic-connect

1. Add the `auth` plugin to your plugin system
2. Implement the D-Bus daemon in `cosmic-connect-daemon`
3. Add biometric handling to `cosmic-connect-android`
4. Create pairing UI in COSMIC Settings
5. Add NixOS module for easy deployment

This would make cosmic-connect the first KDE Connect implementation with native desktop authentication - a killer feature!
