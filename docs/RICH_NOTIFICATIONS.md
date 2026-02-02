# Rich Notification Support Implementation

## Overview

This document describes the rich notification support implementation for COSMIC Connect, enabling notifications with HTML formatting, embedded images, clickable links, and video thumbnails.

## Architecture

### Components

1. **Notification Protocol Layer** (`cosmic-connect-protocol/src/plugins/notification.rs`)
   - Extended `Notification` struct with rich content fields
   - Added `NotificationLink` struct for clickable links
   - Helper methods for decoding images and detecting rich content

2. **COSMIC Desktop Integration** (`cosmic-connect-daemon/src/cosmic_notifications.rs`)
   - HTML sanitization for freedesktop compatibility
   - Image data conversion to freedesktop spec format
   - Link action handling and browser integration
   - Notification metadata tracking

## Data Structures

### NotificationLink

```rust
pub struct NotificationLink {
    pub url: String,                    // URL to open
    pub title: Option<String>,          // Display title
    pub start: usize,                   // Character offset in text
    pub length: usize,                  // Link text length
}
```

### Extended Notification Fields

```rust
pub struct Notification {
    // ... existing fields ...

    /// Rich HTML formatted body text
    pub rich_body: Option<String>,

    /// Base64 encoded notification image
    pub image_data: Option<String>,

    /// Clickable links in the notification
    pub links: Option<Vec<NotificationLink>>,

    /// Base64 encoded video thumbnail
    pub video_thumbnail: Option<String>,
}
```

## Features

### 1. Rich HTML Content

**Supported Tags:**
- `<b>` - Bold text
- `<i>` - Italic text
- `<u>` - Underline text
- `<a href="...">` - Hyperlinks

**Security:**
- All HTML is sanitized before display
- Dangerous tags (script, iframe, img) are stripped
- Only `href` attribute allowed in `<a>` tags
- XSS protection through regex-based filtering

**Usage:**
```rust
let mut notif = Notification::new("id", "App", "Title", "Text", true);
notif.rich_body = Some("<b>Important:</b> Check <a href=\"https://example.com\">this link</a>");
```

### 2. Notification Images

**Image Format:**
- Base64 encoded ARGB32 format
- Automatic conversion to freedesktop `image-data` hint
- Falls back to `sender_avatar` if `image_data` not present

**Freedesktop Spec:**
```rust
image-data: (width: i32, height: i32, rowstride: i32, has_alpha: bool,
             bits_per_sample: i32, channels: i32, data: Vec<u8>)
```

**Usage:**
```rust
notif.image_data = Some(base64::encode(image_bytes));
let decoded = notif.get_image_bytes().unwrap();
```

### 3. Clickable Links

**Link Actions:**
- Each link creates a notification action button
- Action format: `open_link_{index}:{url}`
- Opens URL in default browser via `open::that()`

**Metadata Tracking:**
- Notifications are tracked by ID
- Link URLs stored for action callbacks
- Automatic cleanup on notification close

**Usage:**
```rust
notif.links = Some(vec![
    NotificationLink::new("https://example.com", Some("Read More"), 0, 9),
]);
```

### 4. Video Thumbnails

**Format:**
- Base64 encoded image data
- Displayed as notification image
- Separate from main image_data field

**Usage:**
```rust
notif.video_thumbnail = Some(base64::encode(thumbnail_bytes));
let thumbnail = notif.get_video_thumbnail_bytes().unwrap();
```

## API Reference

### NotificationBuilder Methods

```rust
/// Set rich HTML body (auto-sanitized)
pub fn rich_body(mut self, html: impl Into<String>) -> Self

/// Set notification image data
pub fn image_data(mut self, image_bytes: Vec<u8>, width: i32, height: i32) -> Self
```

### CosmicNotifier Methods

```rust
/// Send rich notification from device
pub async fn notify_rich_from_device(
    &self,
    notification_id: &str,
    device_name: &str,
    app_name: &str,
    title: &str,
    text: &str,
    rich_body: Option<&str>,
    image_bytes: Option<(Vec<u8>, i32, i32)>,
    links: Vec<String>,
) -> Result<u32>

/// Open a notification link
pub async fn open_notification_link(
    &self,
    notification_id: u32,
    link_url: &str,
) -> Result<()>
```

### Notification Helper Methods

```rust
/// Check if notification has rich HTML content
pub fn has_rich_content(&self) -> bool

/// Check if notification has an image
pub fn has_image(&self) -> bool

/// Check if notification has clickable links
pub fn has_links(&self) -> bool

/// Get decoded image bytes
pub fn get_image_bytes(&self) -> Option<Vec<u8>>

/// Get decoded video thumbnail bytes
pub fn get_video_thumbnail_bytes(&self) -> Option<Vec<u8>>
```

## Protocol Specification

### Packet Format

```json
{
    "id": "notification-id-123",
    "type": "cconnect.notification",
    "body": {
        "id": "notification-id-123",
        "appName": "WhatsApp",
        "title": "New Message",
        "text": "Check this out!",
        "richBody": "<b>Important:</b> Check <a href=\"https://example.com\">this link</a>",
        "imageData": "base64encodedimagedata...",
        "links": [
            {
                "url": "https://example.com",
                "title": "Link",
                "start": 20,
                "length": 4
            }
        ],
        "videoThumbnail": "base64encodedthumbnail..."
    }
}
```

## Implementation Details

### HTML Sanitization

The sanitizer uses regex-based filtering to ensure only safe HTML reaches the desktop:

1. **Remove dangerous tags:**
   - script, style, iframe, object, embed
   - link, meta, html, head, body
   - img, video, audio

2. **Clean link attributes:**
   - Only `href` attribute preserved
   - All other attributes stripped (onclick, onerror, etc.)

3. **Preserve safe formatting:**
   - `<b>`, `<i>`, `<u>` tags preserved
   - Text content unchanged

### Image Data Conversion

Images are converted from base64 to freedesktop spec format:

1. Decode base64 string to bytes
2. Create freedesktop image-data structure:
   - Width and height from parameters
   - Rowstride = width * 4 (ARGB32)
   - has_alpha = true
   - bits_per_sample = 8
   - channels = 4 (ARGB)
   - Data as Vec<u8>

### Link Action Handling

Link actions are processed through DBus action callbacks:

1. Notification sent with action buttons
2. User clicks action → DBus signal emitted
3. Action format parsed: `open_link_{index}:{url}`
4. URL opened via `open::that(url)`
5. Metadata cleaned up on notification close

## Testing

### Unit Tests

**Notification Tests:**
- `test_notification_link_creation` - Link construction
- `test_notification_link_serialization` - JSON round-trip
- `test_rich_notification_creation` - Rich content fields
- `test_rich_notification_serialization` - Full serialization
- `test_notification_image_decoding` - Base64 decoding
- `test_notification_video_thumbnail_decoding` - Thumbnail decoding
- `test_notification_has_rich_content` - Content detection
- `test_notification_has_image` - Image detection
- `test_notification_has_links` - Link detection

**Builder Tests:**
- `test_html_sanitization` - HTML cleaning
- `test_rich_body_builder` - Rich body setting
- `test_image_data_hint` - Freedesktop format
- `test_sanitize_script_tags` - XSS prevention
- `test_sanitize_multiple_dangerous_tags` - Multiple tag removal
- `test_sanitize_preserves_safe_content` - Safe content preservation

### Integration Tests

**End-to-End:**
- `test_handle_rich_notification` - Full workflow test

## Security Considerations

### HTML Sanitization

**Threats Mitigated:**
- XSS attacks via `<script>` tags
- Event handler injection (onclick, onerror)
- Content injection via `<iframe>`, `<embed>`
- Style-based attacks via `<style>` tags

**Limitations:**
- Regex-based sanitization is basic
- Production should use proper HTML parser
- URL validation not implemented
- Consider using ammonia or bleach crate

### Link Validation

**Current Implementation:**
- No URL validation
- All URLs opened in default browser
- User sees URL before clicking

**Recommendations:**
- Validate URL schemes (http, https only)
- Block javascript: URLs
- Implement URL preview on hover
- Add user confirmation for external links

### Image Data

**Considerations:**
- Large images consume memory
- No size limits enforced
- Base64 encoding increases size by ~33%

**Recommendations:**
- Enforce maximum image size
- Compress images before encoding
- Consider payload transfer for large images

## Dependencies

### Added Dependencies

```toml
[workspace.dependencies]
regex = "1.10"
open = "5.0"
```

### Crate Usage

**cosmic-connect-protocol:**
- `base64` - Image encoding/decoding

**cosmic-connect-daemon:**
- `regex` - HTML sanitization
- `open` - Browser integration

## Future Enhancements

### Planned Features

1. **Inline Reply:**
   - Text input in notification
   - Send reply directly from desktop
   - Integration with messaging apps

2. **Action Buttons:**
   - Custom notification actions
   - Callback to Android app
   - Action result handling

3. **Progress Indicators:**
   - File transfer progress
   - Download/upload status
   - Animated progress bars

4. **Media Attachments:**
   - Audio message previews
   - Video playback controls
   - Document previews

### Improvements

1. **HTML Parsing:**
   - Use proper HTML parser (ammonia)
   - Better XSS protection
   - Support more formatting tags

2. **Image Optimization:**
   - Image resizing
   - Format conversion (JPEG, PNG)
   - Lazy loading for large images

3. **Link Preview:**
   - Fetch link metadata
   - Display preview cards
   - Cache previewed links

4. **Accessibility:**
   - Screen reader support
   - High contrast mode
   - Keyboard navigation

## Example Usage

### Android Side

```kotlin
// Create rich notification
val notification = NetworkPacket("kdeconnect.notification").apply {
    set("id", "msg-123")
    set("appName", "WhatsApp")
    set("title", "New Message")
    set("text", "Check this out!")
    set("richBody", "<b>Important:</b> See <a href=\"https://example.com\">link</a>")
    set("imageData", Base64.encodeToString(imageBytes, Base64.DEFAULT))
    set("links", JSONArray().apply {
        put(JSONObject().apply {
            put("url", "https://example.com")
            put("title", "Link")
            put("start", 20)
            put("length", 4)
        })
    })
}
```

### Desktop Side

```rust
// Receive and display rich notification
let notifier = CosmicNotifier::new().await?;

notifier.notify_rich_from_device(
    "msg-123",
    "My Phone",
    "WhatsApp",
    "New Message",
    "Check this out!",
    Some("<b>Important:</b> See <a href=\"https://example.com\">link</a>"),
    Some((image_bytes, 200, 200)),
    vec!["https://example.com".to_string()],
).await?;
```

## Troubleshooting

### Common Issues

**Issue:** HTML not displayed correctly
**Solution:** Check freedesktop notification daemon supports HTML

**Issue:** Images not showing
**Solution:** Verify ARGB32 format and size limits

**Issue:** Links not clickable
**Solution:** Ensure DBus action signals are connected

**Issue:** XSS attempts logged
**Solution:** HTML sanitizer working correctly

## References

- [Freedesktop Notification Spec](https://specifications.freedesktop.org/notification-spec/latest/)
- [KDE Connect Protocol](https://invent.kde.org/network/kdeconnect-kde)
- [COSMIC Desktop](https://github.com/pop-os/cosmic-epoch)
- [zbus Documentation](https://docs.rs/zbus/)

## Changelog

### Version 0.1.0 (2026-01-31)

- Initial rich notification support
- HTML sanitization
- Image data conversion
- Link action handling
- Comprehensive test suite
