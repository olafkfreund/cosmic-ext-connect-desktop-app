//! RFB Protocol Definitions
//!
//! Implements RFB (Remote Framebuffer) Protocol 3.8 constants and message types.
//!
//! ## Protocol Flow
//!
//! ```text
//! Client                    Server
//!   |                         |
//!   |  ProtocolVersion        |
//!   |------------------------>|
//!   |                         |
//!   |  ProtocolVersion        |
//!   |<------------------------|
//!   |                         |
//!   |  Security Types         |
//!   |<------------------------|
//!   |                         |
//!   |  Security Type          |
//!   |------------------------>|
//!   |                         |
//!   |  SecurityResult         |
//!   |<------------------------|
//!   |                         |
//!   |  ClientInit             |
//!   |------------------------>|
//!   |                         |
//!   |  ServerInit             |
//!   |<------------------------|
//!   |                         |
//!   |  Client Messages        |
//!   |<----------------------->|
//!   |  Server Messages        |
//!   |                         |
//! ```
//!
//! ## References
//!
//! - [RFB Protocol Specification](https://github.com/rfbproto/rfbproto/blob/master/rfbproto.rst)

use crate::Result;
use std::io::{Read, Write};

/// RFB Protocol version 3.8
pub const RFB_VERSION_3_8: &[u8; 12] = b"RFB 003.008\n";

/// RFB Protocol version 3.3 (for compatibility)
pub const RFB_VERSION_3_3: &[u8; 12] = b"RFB 003.003\n";

/// Security type: Invalid
pub const SECURITY_INVALID: u8 = 0;

/// Security type: None (no authentication)
pub const SECURITY_NONE: u8 = 1;

/// Security type: VNC authentication
pub const SECURITY_VNC_AUTH: u8 = 2;

/// Security result: OK
pub const SECURITY_RESULT_OK: u32 = 0;

/// Security result: Failed
pub const SECURITY_RESULT_FAILED: u32 = 1;

/// Client to server message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClientMessage {
    /// Set pixel format
    SetPixelFormat = 0,

    /// Set encodings
    SetEncodings = 2,

    /// Framebuffer update request
    FramebufferUpdateRequest = 3,

    /// Key event
    KeyEvent = 4,

    /// Pointer event
    PointerEvent = 5,

    /// Client cut text
    ClientCutText = 6,
}

impl ClientMessage {
    /// Parse message type from byte
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::SetPixelFormat),
            2 => Some(Self::SetEncodings),
            3 => Some(Self::FramebufferUpdateRequest),
            4 => Some(Self::KeyEvent),
            5 => Some(Self::PointerEvent),
            6 => Some(Self::ClientCutText),
            _ => None,
        }
    }
}

/// Server to client message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ServerMessage {
    /// Framebuffer update
    FramebufferUpdate = 0,

    /// Set colour map entries
    SetColourMapEntries = 1,

    /// Bell
    Bell = 2,

    /// Server cut text
    ServerCutText = 3,
}

/// RFB encoding types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum RfbEncoding {
    /// Raw encoding
    Raw = 0,

    /// CopyRect encoding
    CopyRect = 1,

    /// RRE encoding
    RRE = 2,

    /// Hextile encoding
    Hextile = 5,

    /// ZRLE encoding
    ZRLE = 16,

    /// Cursor pseudo-encoding
    Cursor = -239,

    /// DesktopSize pseudo-encoding
    DesktopSize = -223,
}

impl RfbEncoding {
    /// Parse encoding from i32
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Raw),
            1 => Some(Self::CopyRect),
            2 => Some(Self::RRE),
            5 => Some(Self::Hextile),
            16 => Some(Self::ZRLE),
            -239 => Some(Self::Cursor),
            -223 => Some(Self::DesktopSize),
            _ => None,
        }
    }
}

/// Pixel format descriptor
#[derive(Debug, Clone, Copy)]
pub struct PixelFormat {
    /// Bits per pixel (8, 16, or 32)
    pub bits_per_pixel: u8,

    /// Depth (number of useful bits in the pixel value)
    pub depth: u8,

    /// Big-endian flag
    pub big_endian_flag: u8,

    /// True colour flag
    pub true_colour_flag: u8,

    /// Red maximum value
    pub red_max: u16,

    /// Green maximum value
    pub green_max: u16,

    /// Blue maximum value
    pub blue_max: u16,

    /// Red shift
    pub red_shift: u8,

    /// Green shift
    pub green_shift: u8,

    /// Blue shift
    pub blue_shift: u8,
}

impl PixelFormat {
    /// Create standard RGBA 32-bit pixel format
    pub fn rgba32() -> Self {
        Self {
            bits_per_pixel: 32,
            depth: 24,
            big_endian_flag: 0,
            true_colour_flag: 1,
            red_max: 255,
            green_max: 255,
            blue_max: 255,
            red_shift: 0,
            green_shift: 8,
            blue_shift: 16,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0] = self.bits_per_pixel;
        bytes[1] = self.depth;
        bytes[2] = self.big_endian_flag;
        bytes[3] = self.true_colour_flag;
        bytes[4..6].copy_from_slice(&self.red_max.to_be_bytes());
        bytes[6..8].copy_from_slice(&self.green_max.to_be_bytes());
        bytes[8..10].copy_from_slice(&self.blue_max.to_be_bytes());
        bytes[10] = self.red_shift;
        bytes[11] = self.green_shift;
        bytes[12] = self.blue_shift;
        // bytes[13..16] are padding
        bytes
    }

    /// Parse from bytes
    pub fn from_bytes(bytes: &[u8; 16]) -> Self {
        Self {
            bits_per_pixel: bytes[0],
            depth: bytes[1],
            big_endian_flag: bytes[2],
            true_colour_flag: bytes[3],
            red_max: u16::from_be_bytes([bytes[4], bytes[5]]),
            green_max: u16::from_be_bytes([bytes[6], bytes[7]]),
            blue_max: u16::from_be_bytes([bytes[8], bytes[9]]),
            red_shift: bytes[10],
            green_shift: bytes[11],
            blue_shift: bytes[12],
        }
    }
}

/// Server initialization message
#[derive(Debug, Clone)]
pub struct ServerInit {
    /// Framebuffer width
    pub width: u16,

    /// Framebuffer height
    pub height: u16,

    /// Pixel format
    pub pixel_format: PixelFormat,

    /// Desktop name
    pub name: String,
}

impl ServerInit {
    /// Create new server init message
    pub fn new(width: u16, height: u16, name: String) -> Self {
        Self {
            width,
            height,
            pixel_format: PixelFormat::rgba32(),
            name,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Width and height (2 bytes each)
        bytes.extend_from_slice(&self.width.to_be_bytes());
        bytes.extend_from_slice(&self.height.to_be_bytes());

        // Pixel format (16 bytes)
        bytes.extend_from_slice(&self.pixel_format.to_bytes());

        // Name length (4 bytes)
        bytes.extend_from_slice(&(self.name.len() as u32).to_be_bytes());

        // Name string
        bytes.extend_from_slice(self.name.as_bytes());

        bytes
    }
}

/// Framebuffer update request
#[derive(Debug, Clone, Copy)]
pub struct FramebufferUpdateRequest {
    /// Incremental update flag
    pub incremental: bool,

    /// X position
    pub x: u16,

    /// Y position
    pub y: u16,

    /// Width
    pub width: u16,

    /// Height
    pub height: u16,
}

impl FramebufferUpdateRequest {
    /// Parse from reader
    pub fn from_reader<R: Read>(reader: &mut R) -> Result<Self> {
        let mut buf = [0u8; 9];
        reader.read_exact(&mut buf)?;

        Ok(Self {
            incremental: buf[0] != 0,
            x: u16::from_be_bytes([buf[1], buf[2]]),
            y: u16::from_be_bytes([buf[3], buf[4]]),
            width: u16::from_be_bytes([buf[5], buf[6]]),
            height: u16::from_be_bytes([buf[7], buf[8]]),
        })
    }
}

/// Key event message
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    /// Down flag (true = key press, false = key release)
    pub down: bool,

    /// Key symbol (X11 keysym)
    pub key: u32,
}

impl KeyEvent {
    /// Parse from reader
    pub fn from_reader<R: Read>(reader: &mut R) -> Result<Self> {
        let mut buf = [0u8; 7];
        reader.read_exact(&mut buf)?;

        Ok(Self {
            down: buf[0] != 0,
            key: u32::from_be_bytes([buf[3], buf[4], buf[5], buf[6]]),
        })
    }
}

/// Pointer event message
#[derive(Debug, Clone, Copy)]
pub struct PointerEvent {
    /// Button mask (bit 0 = left, bit 1 = middle, bit 2 = right)
    pub button_mask: u8,

    /// X position
    pub x: u16,

    /// Y position
    pub y: u16,
}

impl PointerEvent {
    /// Parse from reader
    pub fn from_reader<R: Read>(reader: &mut R) -> Result<Self> {
        let mut buf = [0u8; 5];
        reader.read_exact(&mut buf)?;

        Ok(Self {
            button_mask: buf[0],
            x: u16::from_be_bytes([buf[1], buf[2]]),
            y: u16::from_be_bytes([buf[3], buf[4]]),
        })
    }
}

/// Framebuffer update header
#[derive(Debug, Clone)]
pub struct FramebufferUpdate {
    /// Number of rectangles
    pub rectangles: Vec<Rectangle>,
}

impl FramebufferUpdate {
    /// Create new framebuffer update
    pub fn new(rectangles: Vec<Rectangle>) -> Self {
        Self { rectangles }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Message type
        bytes.push(ServerMessage::FramebufferUpdate as u8);

        // Padding
        bytes.push(0);

        // Number of rectangles
        bytes.extend_from_slice(&(self.rectangles.len() as u16).to_be_bytes());

        // Rectangles
        for rect in &self.rectangles {
            bytes.extend_from_slice(&rect.to_bytes());
        }

        bytes
    }
}

/// Rectangle in framebuffer update
#[derive(Debug, Clone)]
pub struct Rectangle {
    /// X position
    pub x: u16,

    /// Y position
    pub y: u16,

    /// Width
    pub width: u16,

    /// Height
    pub height: u16,

    /// Encoding type
    pub encoding: i32,

    /// Pixel data
    pub data: Vec<u8>,
}

impl Rectangle {
    /// Create new rectangle
    pub fn new(x: u16, y: u16, width: u16, height: u16, encoding: i32, data: Vec<u8>) -> Self {
        Self {
            x,
            y,
            width,
            height,
            encoding,
            data,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Position and size
        bytes.extend_from_slice(&self.x.to_be_bytes());
        bytes.extend_from_slice(&self.y.to_be_bytes());
        bytes.extend_from_slice(&self.width.to_be_bytes());
        bytes.extend_from_slice(&self.height.to_be_bytes());

        // Encoding type
        bytes.extend_from_slice(&self.encoding.to_be_bytes());

        // Pixel data
        bytes.extend_from_slice(&self.data);

        bytes
    }
}

/// Write helper for protocol messages
pub trait ProtocolWrite {
    /// Write u8
    fn write_u8(&mut self, value: u8) -> Result<()>;

    /// Write u16 (big-endian)
    fn write_u16(&mut self, value: u16) -> Result<()>;

    /// Write u32 (big-endian)
    fn write_u32(&mut self, value: u32) -> Result<()>;

    /// Write i32 (big-endian)
    fn write_i32(&mut self, value: i32) -> Result<()>;

    /// Write bytes
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<()>;
}

impl<W: Write> ProtocolWrite for W {
    fn write_u8(&mut self, value: u8) -> Result<()> {
        self.write_all(&[value])?;
        Ok(())
    }

    fn write_u16(&mut self, value: u16) -> Result<()> {
        self.write_all(&value.to_be_bytes())?;
        Ok(())
    }

    fn write_u32(&mut self, value: u32) -> Result<()> {
        self.write_all(&value.to_be_bytes())?;
        Ok(())
    }

    fn write_i32(&mut self, value: i32) -> Result<()> {
        self.write_all(&value.to_be_bytes())?;
        Ok(())
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.write_all(bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_format_serialization() {
        let pf = PixelFormat::rgba32();
        let bytes = pf.to_bytes();
        let parsed = PixelFormat::from_bytes(&bytes);

        assert_eq!(pf.bits_per_pixel, parsed.bits_per_pixel);
        assert_eq!(pf.depth, parsed.depth);
        assert_eq!(pf.red_shift, parsed.red_shift);
        assert_eq!(pf.green_shift, parsed.green_shift);
        assert_eq!(pf.blue_shift, parsed.blue_shift);
    }

    #[test]
    fn test_server_init_serialization() {
        let init = ServerInit::new(1920, 1080, "COSMIC Desktop".to_string());
        let bytes = init.to_bytes();

        // Check width and height
        assert_eq!(u16::from_be_bytes([bytes[0], bytes[1]]), 1920);
        assert_eq!(u16::from_be_bytes([bytes[2], bytes[3]]), 1080);

        // Check name length and name
        let name_len_offset = 4 + 16; // width(2) + height(2) + pixel_format(16)
        let name_len = u32::from_be_bytes([
            bytes[name_len_offset],
            bytes[name_len_offset + 1],
            bytes[name_len_offset + 2],
            bytes[name_len_offset + 3],
        ]);
        assert_eq!(name_len, 14);
    }

    #[test]
    fn test_client_message_parsing() {
        assert_eq!(
            ClientMessage::from_u8(0),
            Some(ClientMessage::SetPixelFormat)
        );
        assert_eq!(
            ClientMessage::from_u8(3),
            Some(ClientMessage::FramebufferUpdateRequest)
        );
        assert_eq!(ClientMessage::from_u8(4), Some(ClientMessage::KeyEvent));
        assert_eq!(ClientMessage::from_u8(5), Some(ClientMessage::PointerEvent));
        assert_eq!(ClientMessage::from_u8(99), None);
    }

    #[test]
    fn test_rfb_encoding_parsing() {
        assert_eq!(RfbEncoding::from_i32(0), Some(RfbEncoding::Raw));
        assert_eq!(RfbEncoding::from_i32(5), Some(RfbEncoding::Hextile));
        assert_eq!(RfbEncoding::from_i32(-223), Some(RfbEncoding::DesktopSize));
        assert_eq!(RfbEncoding::from_i32(999), None);
    }
}
