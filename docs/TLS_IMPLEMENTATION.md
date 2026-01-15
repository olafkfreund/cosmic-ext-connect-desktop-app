# TLS Implementation Guide for KDE Connect

> Version: Protocol v8
> Last Updated: 2026-01-15
> Target: Android Client Rewrite

## Overview

This guide provides detailed TLS implementation guidance for the KDE Connect Android client rewrite. KDE Connect uses TLS with custom certificate verification (certificate pinning) for secure device-to-device communication.

## Table of Contents

1. [Certificate Generation](#certificate-generation)
2. [TLS Connection Setup](#tls-connection-setup)
3. [Certificate Verification](#certificate-verification)
4. [Android Implementation](#android-implementation)
5. [Connection Management](#connection-management)
6. [Error Handling](#error-handling)
7. [Testing](#testing)

---

## Certificate Generation

Each KDE Connect device generates a self-signed X.509 certificate on first run.

### Certificate Requirements

- **Algorithm**: RSA 2048-bit
- **Hash**: SHA-256
- **Validity**: 10 years from generation
- **Subject**: `CN=<device_id>` (common name is the device UUID)
- **Usage**: Digital Signature, Key Encipherment

### Android Implementation (Java)

```java
import java.security.*;
import java.security.cert.Certificate;
import java.security.cert.X509Certificate;
import java.security.spec.RSAKeyGenParameterSpec;
import java.math.BigInteger;
import java.util.Date;
import org.bouncycastle.asn1.x500.X500Name;
import org.bouncycastle.cert.X509v3CertificateBuilder;
import org.bouncycastle.cert.jcajce.JcaX509CertificateConverter;
import org.bouncycastle.cert.jcajce.JcaX509v3CertificateBuilder;
import org.bouncycastle.operator.ContentSigner;
import org.bouncycastle.operator.jcajce.JcaContentSignerBuilder;

public class CertificateGenerator {

    private static final int KEY_SIZE = 2048;
    private static final String SIGNATURE_ALGORITHM = "SHA256withRSA";
    private static final long VALIDITY_YEARS = 10;

    /**
     * Generate a self-signed certificate for this device
     *
     * @param deviceId The unique device ID (UUID)
     * @return KeyPair and X509Certificate
     */
    public static CertificateKeyPair generateCertificate(String deviceId)
            throws Exception {

        // Generate RSA key pair
        KeyPairGenerator keyPairGenerator = KeyPairGenerator.getInstance("RSA");
        keyPairGenerator.initialize(
            new RSAKeyGenParameterSpec(KEY_SIZE, RSAKeyGenParameterSpec.F4)
        );
        KeyPair keyPair = keyPairGenerator.generateKeyPair();

        // Calculate validity period (10 years)
        Date notBefore = new Date();
        Date notAfter = new Date(
            notBefore.getTime() + (VALIDITY_YEARS * 365L * 24 * 60 * 60 * 1000)
        );

        // Build certificate
        X500Name subject = new X500Name("CN=" + deviceId);
        X509v3CertificateBuilder certBuilder = new JcaX509v3CertificateBuilder(
            subject,                              // issuer
            BigInteger.valueOf(System.currentTimeMillis()), // serial
            notBefore,                           // not before
            notAfter,                            // not after
            subject,                             // subject (same as issuer - self-signed)
            keyPair.getPublic()                  // public key
        );

        // Sign the certificate
        ContentSigner signer = new JcaContentSignerBuilder(SIGNATURE_ALGORITHM)
            .build(keyPair.getPrivate());
        X509Certificate certificate = new JcaX509CertificateConverter()
            .getCertificate(certBuilder.build(signer));

        return new CertificateKeyPair(certificate, keyPair);
    }

    /**
     * Store certificate and private key securely
     */
    public static void storeCertificate(
        Context context,
        X509Certificate cert,
        KeyPair keyPair
    ) throws Exception {

        // Use Android KeyStore for secure storage
        KeyStore keyStore = KeyStore.getInstance("AndroidKeyStore");
        keyStore.load(null);

        // Store private key
        keyStore.setKeyEntry(
            "kdeconnect_private_key",
            keyPair.getPrivate(),
            null, // No password needed for AndroidKeyStore
            new Certificate[]{cert}
        );

        // Store certificate in shared preferences
        String certPem = convertToPem(cert);
        context.getSharedPreferences("kdeconnect", Context.MODE_PRIVATE)
            .edit()
            .putString("certificate", certPem)
            .apply();
    }

    private static String convertToPem(X509Certificate cert) throws Exception {
        Base64.Encoder encoder = Base64.getEncoder();
        String certPem = "-----BEGIN CERTIFICATE-----\n";
        certPem += encoder.encodeToString(cert.getEncoded());
        certPem += "\n-----END CERTIFICATE-----";
        return certPem;
    }
}

class CertificateKeyPair {
    public final X509Certificate certificate;
    public final KeyPair keyPair;

    public CertificateKeyPair(X509Certificate certificate, KeyPair keyPair) {
        this.certificate = certificate;
        this.keyPair = keyPair;
    }
}
```

### Android Implementation (Kotlin)

```kotlin
import java.security.*
import java.security.cert.X509Certificate
import java.security.spec.RSAKeyGenParameterSpec
import java.math.BigInteger
import java.util.Date
import org.bouncycastle.asn1.x500.X500Name
import org.bouncycastle.cert.jcajce.JcaX509CertificateConverter
import org.bouncycastle.cert.jcajce.JcaX509v3CertificateBuilder
import org.bouncycastle.operator.jcajce.JcaContentSignerBuilder

object CertificateGenerator {

    private const val KEY_SIZE = 2048
    private const val SIGNATURE_ALGORITHM = "SHA256withRSA"
    private const val VALIDITY_YEARS = 10L

    /**
     * Generate a self-signed certificate for this device
     */
    fun generateCertificate(deviceId: String): CertificateKeyPair {
        // Generate RSA key pair
        val keyPairGenerator = KeyPairGenerator.getInstance("RSA").apply {
            initialize(RSAKeyGenParameterSpec(KEY_SIZE, RSAKeyGenParameterSpec.F4))
        }
        val keyPair = keyPairGenerator.generateKeyPair()

        // Calculate validity period (10 years)
        val notBefore = Date()
        val notAfter = Date(
            notBefore.time + (VALIDITY_YEARS * 365 * 24 * 60 * 60 * 1000)
        )

        // Build certificate
        val subject = X500Name("CN=$deviceId")
        val certBuilder = JcaX509v3CertificateBuilder(
            subject,                                    // issuer
            BigInteger.valueOf(System.currentTimeMillis()), // serial
            notBefore,                                 // not before
            notAfter,                                  // not after
            subject,                                   // subject (self-signed)
            keyPair.public                             // public key
        )

        // Sign the certificate
        val signer = JcaContentSignerBuilder(SIGNATURE_ALGORITHM)
            .build(keyPair.private)
        val certificate = JcaX509CertificateConverter()
            .getCertificate(certBuilder.build(signer))

        return CertificateKeyPair(certificate, keyPair)
    }

    /**
     * Store certificate and private key securely
     */
    fun storeCertificate(
        context: Context,
        cert: X509Certificate,
        keyPair: KeyPair
    ) {
        // Use Android KeyStore for secure storage
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply {
            load(null)
        }

        // Store private key
        keyStore.setKeyEntry(
            "kdeconnect_private_key",
            keyPair.private,
            null, // No password for AndroidKeyStore
            arrayOf(cert)
        )

        // Store certificate in shared preferences
        val certPem = cert.toPem()
        context.getSharedPreferences("kdeconnect", Context.MODE_PRIVATE)
            .edit()
            .putString("certificate", certPem)
            .apply()
    }

    private fun X509Certificate.toPem(): String {
        val encoder = java.util.Base64.getEncoder()
        return """
            -----BEGIN CERTIFICATE-----
            ${encoder.encodeToString(encoded)}
            -----END CERTIFICATE-----
        """.trimIndent()
    }
}

data class CertificateKeyPair(
    val certificate: X509Certificate,
    val keyPair: KeyPair
)
```

---

## TLS Connection Setup

### Protocol v8 Requirements

1. **TLS-First**: Establish TLS connection BEFORE exchanging identity packets
2. **Accept Any Certificate**: During handshake, accept any certificate (verification happens later)
3. **Post-TLS Identity Exchange**: Exchange identity packets over TLS connection
4. **Manual Verification**: Verify certificate fingerprint against stored pairing data

### Client-Side Connection (Kotlin)

```kotlin
import javax.net.ssl.*
import java.security.cert.X509Certificate
import java.net.Socket

class TlsClient(
    private val ownCertificate: X509Certificate,
    private val privateKey: PrivateKey
) {

    /**
     * Connect to a KDE Connect device
     *
     * @param host Device IP address
     * @param port Device port (usually 1716)
     * @return Established TLS connection
     */
    suspend fun connect(host: String, port: Int): TlsConnection =
        withContext(Dispatchers.IO) {

            // Step 1: Create TCP socket
            val socket = Socket(host, port).apply {
                tcpNoDelay = true
                keepAlive = true
                soTimeout = 30000 // 30 second timeout
            }

            // Step 2: Setup TLS with custom trust manager
            val trustManager = AcceptAnyCertificateTrustManager()
            val sslContext = SSLContext.getInstance("TLS").apply {
                init(
                    arrayOf(createKeyManager()),
                    arrayOf(trustManager),
                    SecureRandom()
                )
            }

            // Step 3: Wrap socket with TLS
            val sslSocket = sslContext.socketFactory.createSocket(
                socket,
                host,
                port,
                true // autoClose
            ) as SSLSocket

            // Step 4: Start TLS handshake
            sslSocket.startHandshake()

            // Step 5: Get peer certificate for later verification
            val peerCert = sslSocket.session.peerCertificates[0] as X509Certificate

            TlsConnection(sslSocket, peerCert)
        }

    /**
     * Create key manager with our certificate
     */
    private fun createKeyManager(): KeyManager {
        val keyStore = KeyStore.getInstance(KeyStore.getDefaultType()).apply {
            load(null)
            setKeyEntry(
                "kdeconnect",
                privateKey,
                charArrayOf(),
                arrayOf(ownCertificate)
            )
        }

        val kmf = KeyManagerFactory.getInstance(
            KeyManagerFactory.getDefaultAlgorithm()
        ).apply {
            init(keyStore, charArrayOf())
        }

        return kmf.keyManagers[0]
    }
}

/**
 * Trust manager that accepts any certificate during TLS handshake
 * Actual verification happens after identity exchange
 */
class AcceptAnyCertificateTrustManager : X509TrustManager {
    override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {
        // Accept any certificate - we verify manually later
    }

    override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {
        // Accept any certificate - we verify manually later
    }

    override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
}

/**
 * Represents an established TLS connection
 */
class TlsConnection(
    private val socket: SSLSocket,
    val peerCertificate: X509Certificate
) {
    private val input = socket.inputStream.bufferedReader()
    private val output = socket.outputStream.bufferedWriter()

    /**
     * Send a packet over TLS
     */
    suspend fun sendPacket(packet: Packet) = withContext(Dispatchers.IO) {
        val json = packet.toJson()
        output.write(json)
        output.newLine()
        output.flush()
    }

    /**
     * Receive a packet over TLS
     */
    suspend fun receivePacket(): Packet = withContext(Dispatchers.IO) {
        val line = input.readLine() ?: throw IOException("Connection closed")
        Packet.fromJson(line)
    }

    /**
     * Close the TLS connection
     */
    fun close() {
        socket.close()
    }
}
```

### Server-Side Connection (Kotlin)

```kotlin
import javax.net.ssl.*
import java.security.cert.X509Certificate
import java.net.ServerSocket

class TlsServer(
    private val ownCertificate: X509Certificate,
    private val privateKey: PrivateKey,
    private val port: Int = 1716
) {

    private var serverSocket: ServerSocket? = null

    /**
     * Start listening for incoming connections
     */
    suspend fun start() = withContext(Dispatchers.IO) {
        val sslContext = SSLContext.getInstance("TLS").apply {
            init(
                arrayOf(createKeyManager()),
                arrayOf(AcceptAnyCertificateTrustManager()),
                SecureRandom()
            )
        }

        serverSocket = sslContext.serverSocketFactory.createServerSocket(port)

        Log.i("TlsServer", "Listening on port $port")

        // Accept connections in loop
        while (true) {
            val socket = serverSocket?.accept() as? SSLSocket ?: break

            // Handle each connection in separate coroutine
            launch {
                handleConnection(socket)
            }
        }
    }

    /**
     * Handle incoming TLS connection
     */
    private suspend fun handleConnection(socket: SSLSocket) {
        try {
            // TLS handshake already completed by SSLServerSocket
            val peerCert = socket.session.peerCertificates[0] as X509Certificate
            val connection = TlsConnection(socket, peerCert)

            // Process the connection (identity exchange, pairing, etc.)
            // This is handled by ConnectionManager

        } catch (e: Exception) {
            Log.e("TlsServer", "Connection error", e)
            socket.close()
        }
    }

    /**
     * Create key manager with our certificate
     */
    private fun createKeyManager(): KeyManager {
        val keyStore = KeyStore.getInstance(KeyStore.getDefaultType()).apply {
            load(null)
            setKeyEntry(
                "kdeconnect",
                privateKey,
                charArrayOf(),
                arrayOf(ownCertificate)
            )
        }

        val kmf = KeyManagerFactory.getInstance(
            KeyManagerFactory.getDefaultAlgorithm()
        ).apply {
            init(keyStore, charArrayOf())
        }

        return kmf.keyManagers[0]
    }

    /**
     * Stop the server
     */
    fun stop() {
        serverSocket?.close()
        serverSocket = null
    }
}
```

---

## Certificate Verification

After TLS handshake and identity exchange, verify the peer's certificate against stored pairing data.

### SHA-256 Fingerprint Computation

```kotlin
import java.security.MessageDigest
import java.security.cert.X509Certificate

object CertificateVerifier {

    /**
     * Compute SHA-256 fingerprint of a certificate
     * Returns colon-separated hex string (e.g., "AB:CD:EF:...")
     */
    fun computeSha256Fingerprint(cert: X509Certificate): String {
        val digest = MessageDigest.getInstance("SHA-256")
        val hash = digest.digest(cert.encoded)

        return hash.joinToString(":") { byte ->
            "%02X".format(byte)
        }
    }

    /**
     * Verify certificate matches stored fingerprint for paired device
     *
     * @param deviceId Remote device ID
     * @param cert Certificate received from remote device
     * @return true if certificate matches stored fingerprint
     */
    fun verifyPairedDevice(
        deviceId: String,
        cert: X509Certificate,
        storage: PairingStorage
    ): Boolean {
        val storedFingerprint = storage.getCertificateFingerprint(deviceId)
            ?: return false // Device not paired

        val currentFingerprint = computeSha256Fingerprint(cert)

        if (currentFingerprint != storedFingerprint) {
            Log.e("CertVerify",
                "Certificate mismatch for device $deviceId!\n" +
                "Expected: $storedFingerprint\n" +
                "Received: $currentFingerprint"
            )
            return false
        }

        return true
    }

    /**
     * Store certificate fingerprint for newly paired device
     */
    fun storeCertificateFingerprint(
        deviceId: String,
        cert: X509Certificate,
        storage: PairingStorage
    ) {
        val fingerprint = computeSha256Fingerprint(cert)
        storage.storeCertificateFingerprint(deviceId, fingerprint)

        Log.i("CertVerify", "Stored certificate for device $deviceId: $fingerprint")
    }
}
```

### Pairing Storage Interface

```kotlin
interface PairingStorage {
    /**
     * Get stored certificate fingerprint for a device
     */
    fun getCertificateFingerprint(deviceId: String): String?

    /**
     * Store certificate fingerprint for a device
     */
    fun storeCertificateFingerprint(deviceId: String, fingerprint: String)

    /**
     * Check if device is paired
     */
    fun isPaired(deviceId: String): Boolean

    /**
     * Remove pairing data for a device
     */
    fun unpair(deviceId: String)
}

/**
 * Simple implementation using SharedPreferences
 */
class SharedPreferencesPairingStorage(
    private val context: Context
) : PairingStorage {

    private val prefs = context.getSharedPreferences(
        "kdeconnect_pairing",
        Context.MODE_PRIVATE
    )

    override fun getCertificateFingerprint(deviceId: String): String? {
        return prefs.getString("cert_$deviceId", null)
    }

    override fun storeCertificateFingerprint(deviceId: String, fingerprint: String) {
        prefs.edit()
            .putString("cert_$deviceId", fingerprint)
            .putBoolean("paired_$deviceId", true)
            .apply()
    }

    override fun isPaired(deviceId: String): Boolean {
        return prefs.getBoolean("paired_$deviceId", false)
    }

    override fun unpair(deviceId: String) {
        prefs.edit()
            .remove("cert_$deviceId")
            .remove("paired_$deviceId")
            .apply()
    }
}
```

---

## Android Implementation

### Complete Connection Flow

```kotlin
class ConnectionManager(
    private val ownDeviceId: String,
    private val ownCertificate: X509Certificate,
    private val privateKey: PrivateKey,
    private val pairingStorage: PairingStorage
) {

    private val tlsClient = TlsClient(ownCertificate, privateKey)
    private val tlsServer = TlsServer(ownCertificate, privateKey)

    /**
     * Connect to a discovered device
     */
    suspend fun connectToDevice(
        deviceId: String,
        host: String,
        port: Int
    ): Result<Connection> = withContext(Dispatchers.IO) {
        try {
            // Step 1: Establish TLS connection
            val tlsConn = tlsClient.connect(host, port)

            // Step 2: Exchange identity packets (Protocol v8 - AFTER TLS)
            tlsConn.sendPacket(createIdentityPacket())
            val remoteIdentity = tlsConn.receivePacket()

            if (remoteIdentity.type != "kdeconnect.identity") {
                throw ProtocolException("Expected identity packet")
            }

            val remoteDeviceId = remoteIdentity.body["deviceId"] as String

            // Step 3: Verify certificate if device is paired
            if (pairingStorage.isPaired(remoteDeviceId)) {
                val isValid = CertificateVerifier.verifyPairedDevice(
                    remoteDeviceId,
                    tlsConn.peerCertificate,
                    pairingStorage
                )

                if (!isValid) {
                    tlsConn.close()
                    return@withContext Result.failure(
                        SecurityException("Certificate verification failed")
                    )
                }
            }

            // Step 4: Create connection object
            val connection = Connection(
                deviceId = remoteDeviceId,
                tlsConnection = tlsConn,
                isPaired = pairingStorage.isPaired(remoteDeviceId)
            )

            Result.success(connection)

        } catch (e: Exception) {
            Log.e("ConnMgr", "Connection failed", e)
            Result.failure(e)
        }
    }

    /**
     * Handle pairing request
     */
    suspend fun handlePairingRequest(
        connection: Connection,
        request: Packet
    ): Boolean {
        // User approves pairing (show dialog, etc.)
        val userApproved = showPairingDialog(connection.deviceId)

        if (userApproved) {
            // Store certificate fingerprint
            CertificateVerifier.storeCertificateFingerprint(
                connection.deviceId,
                connection.tlsConnection.peerCertificate,
                pairingStorage
            )

            // Send pairing response
            connection.tlsConnection.sendPacket(
                Packet(
                    type = "kdeconnect.pair",
                    body = mapOf("pair" to true)
                )
            )

            connection.isPaired = true
            return true
        } else {
            // User rejected pairing
            connection.tlsConnection.sendPacket(
                Packet(
                    type = "kdeconnect.pair",
                    body = mapOf("pair" to false)
                )
            )
            return false
        }
    }

    private fun createIdentityPacket(): Packet {
        return Packet(
            id = System.currentTimeMillis(),
            type = "kdeconnect.identity",
            body = mapOf(
                "deviceId" to ownDeviceId,
                "deviceName" to getDeviceName(),
                "deviceType" to "phone",
                "protocolVersion" to 8,
                "incomingCapabilities" to listOf(/* plugin IDs */),
                "outgoingCapabilities" to listOf(/* plugin IDs */)
            )
        )
    }
}

/**
 * Represents an active connection to a device
 */
data class Connection(
    val deviceId: String,
    val tlsConnection: TlsConnection,
    var isPaired: Boolean
)
```

---

## Connection Management

### Rate Limiting (IMPORTANT)

The desktop implementation has rate limiting to prevent connection storms. Android client should respect this:

```kotlin
class ConnectionRateLimiter {
    private val lastConnectionTime = mutableMapOf<String, Long>()
    private val minDelayMs = 1000L // 1 second minimum between attempts

    /**
     * Check if we can attempt connection to device
     * Returns true if enough time has passed since last attempt
     */
    fun canConnect(deviceId: String): Boolean {
        val now = System.currentTimeMillis()
        val lastTime = lastConnectionTime[deviceId] ?: 0L
        val elapsed = now - lastTime

        if (elapsed < minDelayMs) {
            Log.w("RateLimit",
                "Too soon to reconnect to $deviceId (${elapsed}ms < ${minDelayMs}ms)")
            return false
        }

        lastConnectionTime[deviceId] = now
        return true
    }

    /**
     * Clear rate limit for device (when user manually triggers connection)
     */
    fun reset(deviceId: String) {
        lastConnectionTime.remove(deviceId)
    }
}
```

### Socket Reuse Strategy

**IMPORTANT:** Do NOT close and reopen connections unnecessarily. Reuse the same socket:

```kotlin
class DeviceConnection(
    val deviceId: String,
    var tlsConnection: TlsConnection
) {
    private var isAlive = true

    /**
     * Check if connection is still alive
     */
    fun isConnected(): Boolean {
        if (!isAlive) return false

        try {
            // Try to read with timeout to check if socket is alive
            // Implementation depends on your socket setup
            return tlsConnection.socket.isConnected &&
                   !tlsConnection.socket.isClosed
        } catch (e: Exception) {
            isAlive = false
            return false
        }
    }

    /**
     * Replace socket in existing connection (NOT recommended - causes issues)
     * Better approach: Keep existing connection stable
     */
    @Deprecated("Causes connection cycling - avoid replacing sockets")
    fun replaceSocket(newTlsConnection: TlsConnection) {
        tlsConnection.close()
        tlsConnection = newTlsConnection
    }
}
```

### Connection Stability Best Practices

Based on debugging the desktop implementation:

1. **DO NOT reconnect aggressively** - The phone was reconnecting every 5 seconds, causing connection cycling
2. **Reuse existing connections** - Don't close and reopen connections unnecessarily
3. **Use TCP keepalive** - Let OS handle connection liveness
4. **Respect rate limiting** - Wait at least 1 second between connection attempts
5. **Handle disconnects gracefully** - Only reconnect when connection genuinely fails

```kotlin
class ConnectionStabilityManager {

    /**
     * Configure socket for stable long-lived connection
     */
    fun configureSocket(socket: Socket) {
        socket.apply {
            // Use TCP keepalive instead of application-level pings
            keepAlive = true

            // Disable Nagle's algorithm for low-latency
            tcpNoDelay = true

            // Set reasonable timeout (5 minutes like desktop)
            soTimeout = 300_000

            // Set send/receive buffer sizes
            sendBufferSize = 8192
            receiveBufferSize = 8192
        }
    }

    /**
     * Monitor connection health without aggressive reconnections
     */
    suspend fun monitorConnection(connection: DeviceConnection) {
        while (connection.isConnected()) {
            delay(60_000) // Check every 60 seconds (not 5 seconds!)

            // Optionally send ping if truly needed
            // But prefer TCP keepalive over application pings
        }

        // Connection lost - wait before reconnecting
        delay(1000)
        reconnectToDevice(connection.deviceId)
    }
}
```

---

## Error Handling

### Common TLS Errors

```kotlin
sealed class TlsError : Exception() {
    data class HandshakeFailed(val cause: Throwable) : TlsError()
    data class CertificateVerificationFailed(val deviceId: String) : TlsError()
    data class ConnectionTimeout(val host: String) : TlsError()
    data class ProtocolVersionMismatch(val expected: Int, val received: Int) : TlsError()
}

fun handleTlsError(error: TlsError) {
    when (error) {
        is TlsError.HandshakeFailed -> {
            Log.e("TLS", "TLS handshake failed", error.cause)
            // Show user-friendly error
            showError("Failed to establish secure connection")
        }

        is TlsError.CertificateVerificationFailed -> {
            Log.e("TLS", "Certificate verification failed for ${error.deviceId}")
            // This is CRITICAL - potential MITM attack
            showSecurityAlert(
                "Device certificate changed! " +
                "This could be a security risk. " +
                "If you trust this device, unpair and re-pair."
            )
        }

        is TlsError.ConnectionTimeout -> {
            Log.w("TLS", "Connection timeout to ${error.host}")
            showError("Device not reachable")
        }

        is TlsError.ProtocolVersionMismatch -> {
            Log.e("TLS", "Protocol mismatch: expected ${error.expected}, got ${error.received}")
            showError("Incompatible KDE Connect version")
        }
    }
}
```

### Retry Strategy

```kotlin
class ConnectionRetryManager {

    private val maxRetries = 3
    private val baseDelayMs = 1000L

    /**
     * Retry connection with exponential backoff
     */
    suspend fun <T> retryWithBackoff(
        operation: suspend () -> T
    ): Result<T> {
        var lastException: Exception? = null

        repeat(maxRetries) { attempt ->
            try {
                return Result.success(operation())
            } catch (e: Exception) {
                lastException = e
                val delayMs = baseDelayMs * (1 shl attempt) // Exponential backoff
                Log.w("Retry", "Attempt ${attempt + 1} failed, retrying in ${delayMs}ms", e)
                delay(delayMs)
            }
        }

        return Result.failure(
            lastException ?: Exception("All retries failed")
        )
    }
}

// Usage
val result = retryManager.retryWithBackoff {
    connectionManager.connectToDevice(deviceId, host, port)
}
```

---

## Testing

### Testing TLS with OpenSSL

```bash
# Test TLS server
openssl s_client -connect <device_ip>:1716 -showcerts

# Verify certificate details
openssl x509 -in cert.pem -text -noout

# Compute SHA-256 fingerprint
openssl x509 -in cert.pem -noout -fingerprint -sha256
```

### Unit Tests

```kotlin
class CertificateVerifierTest {

    @Test
    fun `computeSha256Fingerprint returns correct format`() {
        val cert = generateTestCertificate()
        val fingerprint = CertificateVerifier.computeSha256Fingerprint(cert)

        // Should be colon-separated hex
        assertTrue(fingerprint.matches(Regex("([0-9A-F]{2}:){31}[0-9A-F]{2}")))
    }

    @Test
    fun `verifyPairedDevice returns true for matching certificate`() {
        val storage = MockPairingStorage()
        val cert = generateTestCertificate()
        val deviceId = "test_device"

        // Store fingerprint
        CertificateVerifier.storeCertificateFingerprint(deviceId, cert, storage)

        // Verify same certificate
        val isValid = CertificateVerifier.verifyPairedDevice(deviceId, cert, storage)
        assertTrue(isValid)
    }

    @Test
    fun `verifyPairedDevice returns false for different certificate`() {
        val storage = MockPairingStorage()
        val cert1 = generateTestCertificate()
        val cert2 = generateTestCertificate()
        val deviceId = "test_device"

        // Store first certificate
        CertificateVerifier.storeCertificateFingerprint(deviceId, cert1, storage)

        // Try to verify with different certificate
        val isValid = CertificateVerifier.verifyPairedDevice(deviceId, cert2, storage)
        assertFalse(isValid)
    }
}

class TlsConnectionTest {

    @Test
    fun `connection establishes successfully`() = runTest {
        val server = startTestServer()
        val client = TlsClient(testCertificate, testPrivateKey)

        val connection = client.connect("localhost", server.port)

        assertNotNull(connection)
        connection.close()
        server.stop()
    }

    @Test
    fun `identity packet exchange works`() = runTest {
        val (serverConn, clientConn) = establishTestConnection()

        // Client sends identity
        val clientIdentity = createTestIdentityPacket("client_device")
        clientConn.sendPacket(clientIdentity)

        // Server receives identity
        val received = serverConn.receivePacket()
        assertEquals("kdeconnect.identity", received.type)
        assertEquals("client_device", received.body["deviceId"])
    }
}
```

### Integration Tests

```kotlin
@RunWith(AndroidJUnit4::class)
class PairingIntegrationTest {

    @Test
    fun `complete pairing flow succeeds`() = runTest {
        // Setup
        val device1 = createTestDevice("device1")
        val device2 = createTestDevice("device2")

        // Device 1 connects to Device 2
        val connection = device1.connectToDevice(
            device2.deviceId,
            "localhost",
            device2.port
        ).getOrThrow()

        // Device 1 requests pairing
        device1.requestPairing(connection)

        // Device 2 accepts pairing
        device2.acceptPairing(connection.deviceId)

        // Wait for pairing to complete
        delay(1000)

        // Verify both devices show as paired
        assertTrue(device1.isPaired(device2.deviceId))
        assertTrue(device2.isPaired(device1.deviceId))

        // Verify certificates are stored
        assertNotNull(device1.getCertificateFingerprint(device2.deviceId))
        assertNotNull(device2.getCertificateFingerprint(device1.deviceId))
    }

    @Test
    fun `reconnection uses same certificate`() = runTest {
        val device1 = createTestDevice("device1")
        val device2 = createTestDevice("device2")

        // Pair devices
        pairDevices(device1, device2)

        // Get certificate fingerprint from first connection
        val firstFingerprint = device1.getCertificateFingerprint(device2.deviceId)

        // Disconnect and reconnect
        device1.disconnect(device2.deviceId)
        delay(1000)
        device1.connectToDevice(device2.deviceId, "localhost", device2.port)

        // Verify same certificate is used
        val secondFingerprint = device1.getCertificateFingerprint(device2.deviceId)
        assertEquals(firstFingerprint, secondFingerprint)
    }
}
```

---

## Security Considerations

### Certificate Pinning

- **ALWAYS** verify certificate fingerprint for paired devices
- **NEVER** accept certificate changes without user confirmation
- Store fingerprints securely (Android KeyStore or encrypted SharedPreferences)

### TLS Configuration

```kotlin
// Recommended TLS configuration
fun createSecureTlsContext(): SSLContext {
    val context = SSLContext.getInstance("TLSv1.3") // Use TLS 1.3 if available

    // Fallback to TLS 1.2
    val availableProtocols = SSLSocket.getSupportedProtocols()
    val protocol = when {
        "TLSv1.3" in availableProtocols -> "TLSv1.3"
        "TLSv1.2" in availableProtocols -> "TLSv1.2"
        else -> throw IllegalStateException("No secure TLS version available")
    }

    return SSLContext.getInstance(protocol)
}
```

### Certificate Storage

```kotlin
// Use Android KeyStore for private key
fun storePrivateKeySecurely(privateKey: PrivateKey, alias: String) {
    val keyStore = KeyStore.getInstance("AndroidKeyStore").apply {
        load(null)
    }

    // Store with encryption
    keyStore.setEntry(
        alias,
        KeyStore.PrivateKeyEntry(privateKey, arrayOf()),
        KeyProtection.Builder(KeyProperties.PURPOSE_SIGN)
            .setUserAuthenticationRequired(false)
            .build()
    )
}
```

---

## References

- [Original KDE Connect Android Source](https://invent.kde.org/network/kdeconnect-android)
- [KDE Connect Protocol Documentation](https://community.kde.org/KDEConnect)
- [RFC 5280 - X.509 Certificate Standard](https://tools.ietf.org/html/rfc5280)
- [Android Network Security Config](https://developer.android.com/training/articles/security-config)
- [Pairing Process Documentation](./PAIRING_PROCESS.md)
- [GitHub Issue #52 - Connection Cycling Bug](https://github.com/olafkfreund/cosmic-applet-kdeconnect/issues/52)

---

**Next Steps:**
1. Review this documentation alongside PAIRING_PROCESS.md
2. Implement certificate generation on first run
3. Implement TLS connection setup following Protocol v8
4. Implement certificate verification with fingerprint checking
5. Add rate limiting to prevent connection storms
6. Test with desktop implementation
7. Fix connection cycling issue in Android client
