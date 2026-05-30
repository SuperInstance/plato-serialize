#[cfg(test)]
mod lib_tests;

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Wire format variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum WireFormat {
    Json = 0x01,
    Compact = 0x02,
    Binary = 0x03,
}

impl WireFormat {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Json),
            0x02 => Some(Self::Compact),
            0x03 => Some(Self::Binary),
            _ => None,
        }
    }
}

impl fmt::Display for WireFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json => write!(f, "Json"),
            Self::Compact => write!(f, "Compact"),
            Self::Binary => write!(f, "Binary"),
        }
    }
}

/// Wire header prepended to every message.
///
/// Wire layout (6 bytes):
/// - 1 byte: version
/// - 1 byte: format flag
/// - 2 bytes BE: payload length
/// - 2 bytes BE: CRC16 checksum (over payload only)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireHeader {
    pub version: u8,
    pub format: WireFormat,
    pub payload_len: u16,
    pub checksum: u16,
}

impl WireHeader {
    pub const SIZE: usize = 6;

    pub fn to_bytes(&self) -> [u8; 6] {
        let mut buf = [0u8; 6];
        buf[0] = self.version;
        buf[1] = self.format as u8;
        buf[2..4].copy_from_slice(&self.payload_len.to_be_bytes());
        buf[4..6].copy_from_slice(&self.checksum.to_be_bytes());
        buf
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, SerializeError> {
        if data.len() < Self::SIZE {
            return Err(SerializeError::TruncatedHeader {
                expected: Self::SIZE,
                got: data.len(),
            });
        }
        let version = data[0];
        let format = WireFormat::from_byte(data[1]).ok_or(SerializeError::UnknownFormat(data[1]))?;
        let payload_len = u16::from_be_bytes([data[2], data[3]]);
        let checksum = u16::from_be_bytes([data[4], data[5]]);
        Ok(Self {
            version,
            format,
            payload_len,
            checksum,
        })
    }
}

// ---------------------------------------------------------------------------
// Tile type (minimal, self-contained)
// ---------------------------------------------------------------------------

/// Tile type enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum TileType {
    Sensor = 0x01,
    Actuator = 0x02,
    Virtual = 0x03,
    Composite = 0x04,
}

impl TileType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Sensor),
            0x02 => Some(Self::Actuator),
            0x03 => Some(Self::Virtual),
            0x04 => Some(Self::Composite),
            _ => None,
        }
    }
}

/// Value type for a tile reading.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ValueType {
    Float(f64),
    Int(i32),
}

/// A PLATO tile — the domain object we serialize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    pub id: Uuid,
    pub tile_type: TileType,
    pub value: ValueType,
    pub confidence: f64,
    pub timestamp: u64,
    pub layer: u8,
}

// ---------------------------------------------------------------------------
// Compact tile (fixed-size binary layout, 43 bytes)
//
//  16 bytes  UUID (big-endian)
//   1 byte   tile type
//   1 byte   value discriminator (0 = float, 1 = int)
//   8 bytes  f64 value  OR  4-byte i32 + 4 bytes zero pad
//   8 bytes  confidence (f64)
//   8 bytes  timestamp (u64)
//   1 byte   layer
// ---------------------------------------------------------------------------

pub const COMPACT_TILE_SIZE: usize = 43;

/// Compact on-wire representation of a Tile.
#[derive(Debug, Clone, PartialEq)]
pub struct CompactTile {
    pub id: Uuid,
    pub tile_type: TileType,
    pub value: ValueType,
    pub confidence: f64,
    pub timestamp: u64,
    pub layer: u8,
}

impl CompactTile {
    pub fn from_tile(tile: &Tile) -> Self {
        Self {
            id: tile.id,
            tile_type: tile.tile_type,
            value: tile.value,
            confidence: tile.confidence,
            timestamp: tile.timestamp,
            layer: tile.layer,
        }
    }

    pub fn to_tile(&self) -> Tile {
        Tile {
            id: self.id,
            tile_type: self.tile_type,
            value: self.value,
            confidence: self.confidence,
            timestamp: self.timestamp,
            layer: self.layer,
        }
    }

    /// Encode to fixed-size byte array.
    pub fn to_bytes(&self) -> [u8; COMPACT_TILE_SIZE] {
        let mut buf = [0u8; COMPACT_TILE_SIZE];
        // UUID (16 bytes)
        buf[0..16].copy_from_slice(self.id.as_bytes());
        // Tile type (1 byte)
        buf[16] = self.tile_type as u8;
        // Value discriminator + payload (9 bytes)
        match self.value {
            ValueType::Float(f) => {
                buf[17] = 0;
                buf[18..26].copy_from_slice(&f.to_be_bytes());
            }
            ValueType::Int(i) => {
                buf[17] = 1;
                buf[18..22].copy_from_slice(&i.to_be_bytes());
                // bytes 22..26 stay zero-padded
            }
        }
        // Confidence (8 bytes)
        buf[26..34].copy_from_slice(&self.confidence.to_be_bytes());
        // Timestamp (8 bytes)
        buf[34..42].copy_from_slice(&self.timestamp.to_be_bytes());
        // Layer (1 byte)
        buf[42] = self.layer;
        buf
    }

    /// Decode from byte slice. Accepts exactly COMPACT_TILE_SIZE bytes or more
    /// (reads only the first COMPACT_TILE_SIZE).
    pub fn from_bytes(data: &[u8]) -> Result<Self, SerializeError> {
        if data.len() < COMPACT_TILE_SIZE {
            return Err(SerializeError::TruncatedTile {
                expected: COMPACT_TILE_SIZE,
                got: data.len(),
            });
        }
        let id = Uuid::from_bytes_ref(data[0..16].try_into().unwrap());
        let tile_type = TileType::from_byte(data[16])
            .ok_or(SerializeError::UnknownTileType(data[16]))?;
        let value = match data[17] {
            0 => ValueType::Float(f64::from_be_bytes(data[18..26].try_into().unwrap())),
            1 => ValueType::Int(i32::from_be_bytes(data[18..22].try_into().unwrap())),
            d => return Err(SerializeError::UnknownValueType(d)),
        };
        let confidence = f64::from_be_bytes(data[26..34].try_into().unwrap());
        let timestamp = u64::from_be_bytes(data[34..42].try_into().unwrap());
        let layer = data[42];
        Ok(Self {
            id: *id,
            tile_type,
            value,
            confidence,
            timestamp,
            layer,
        })
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SerializeError {
    #[error("truncated header: expected {expected} bytes, got {got}")]
    TruncatedHeader { expected: usize, got: usize },
    #[error("truncated tile: expected {expected} bytes, got {got}")]
    TruncatedTile { expected: usize, got: usize },
    #[error("truncated payload: expected {expected} bytes, got {got}")]
    TruncatedPayload { expected: usize, got: usize },
    #[error("unknown wire format byte: {0:#04x}")]
    UnknownFormat(u8),
    #[error("unknown tile type byte: {0:#04x}")]
    UnknownTileType(u8),
    #[error("unknown value type discriminator: {0}")]
    UnknownValueType(u8),
    #[error("CRC mismatch: expected {expected:#06x}, computed {computed:#06x}")]
    CrcMismatch { expected: u16, computed: u16 },
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u8),
}

// ---------------------------------------------------------------------------
// CRC-16 (CCITT / CRC-16-XMODEM)
// ---------------------------------------------------------------------------

/// Compute CRC-16/XMODEM over `data`.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

// ---------------------------------------------------------------------------
// BinaryEncoder / BinaryDecoder
// ---------------------------------------------------------------------------

pub const PROTOCOL_VERSION: u8 = 1;

pub struct BinaryEncoder;

impl BinaryEncoder {
    /// Encode a single tile to binary wire format (header + compact payload).
    pub fn encode_tile(tile: &Tile) -> Vec<u8> {
        let compact = CompactTile::from_tile(tile);
        let payload = compact.to_bytes();
        let checksum = crc16(&payload);
        let header = WireHeader {
            version: PROTOCOL_VERSION,
            format: WireFormat::Binary,
            payload_len: payload.len() as u16,
            checksum,
        };
        let mut buf = Vec::with_capacity(WireHeader::SIZE + payload.len());
        buf.extend_from_slice(&header.to_bytes());
        buf.extend_from_slice(&payload);
        buf
    }

    /// Encode a batch of tiles. Payload = u16 BE count + N compact tiles.
    pub fn encode_batch(tiles: &[Tile]) -> Vec<u8> {
        let count = tiles.len() as u16;
        let mut payload = Vec::with_capacity(2 + tiles.len() * COMPACT_TILE_SIZE);
        payload.extend_from_slice(&count.to_be_bytes());
        for tile in tiles {
            let compact = CompactTile::from_tile(tile);
            payload.extend_from_slice(&compact.to_bytes());
        }
        let checksum = crc16(&payload);
        let header = WireHeader {
            version: PROTOCOL_VERSION,
            format: WireFormat::Binary,
            payload_len: payload.len() as u16,
            checksum,
        };
        let mut buf = Vec::with_capacity(WireHeader::SIZE + payload.len());
        buf.extend_from_slice(&header.to_bytes());
        buf.extend_from_slice(&payload);
        buf
    }

    /// JSON size of a single tile for comparison.
    pub fn json_size(tile: &Tile) -> usize {
        serde_json::to_vec(tile).unwrap().len()
    }
}

pub struct BinaryDecoder;

impl BinaryDecoder {
    /// Decode a single tile from binary wire format.
    pub fn decode_tile(data: &[u8]) -> Result<Tile, SerializeError> {
        if data.len() < WireHeader::SIZE {
            return Err(SerializeError::TruncatedHeader {
                expected: WireHeader::SIZE,
                got: data.len(),
            });
        }
        let header = WireHeader::from_bytes(data)?;
        if header.version != PROTOCOL_VERSION {
            return Err(SerializeError::UnsupportedVersion(header.version));
        }
        let payload_start = WireHeader::SIZE;
        let payload_end = payload_start + header.payload_len as usize;
        if data.len() < payload_end {
            return Err(SerializeError::TruncatedPayload {
                expected: payload_end,
                got: data.len(),
            });
        }
        let payload = &data[payload_start..payload_end];
        let computed = crc16(payload);
        if computed != header.checksum {
            return Err(SerializeError::CrcMismatch {
                expected: header.checksum,
                computed,
            });
        }
        let compact = CompactTile::from_bytes(payload)?;
        Ok(compact.to_tile())
    }

    /// Decode a batch of tiles from binary wire format.
    pub fn decode_batch(data: &[u8]) -> Result<Vec<Tile>, SerializeError> {
        if data.len() < WireHeader::SIZE {
            return Err(SerializeError::TruncatedHeader {
                expected: WireHeader::SIZE,
                got: data.len(),
            });
        }
        let header = WireHeader::from_bytes(data)?;
        if header.version != PROTOCOL_VERSION {
            return Err(SerializeError::UnsupportedVersion(header.version));
        }
        let payload_start = WireHeader::SIZE;
        let payload_end = payload_start + header.payload_len as usize;
        if data.len() < payload_end {
            return Err(SerializeError::TruncatedPayload {
                expected: payload_end,
                got: data.len(),
            });
        }
        let payload = &data[payload_start..payload_end];
        let computed = crc16(payload);
        if computed != header.checksum {
            return Err(SerializeError::CrcMismatch {
                expected: header.checksum,
                computed,
            });
        }
        if payload.len() < 2 {
            return Err(SerializeError::TruncatedPayload {
                expected: 2,
                got: payload.len(),
            });
        }
        let count = u16::from_be_bytes([payload[0], payload[1]]) as usize;
        let expected_len = 2 + count * COMPACT_TILE_SIZE;
        if payload.len() < expected_len {
            return Err(SerializeError::TruncatedPayload {
                expected: expected_len,
                got: payload.len(),
            });
        }
        let mut tiles = Vec::with_capacity(count);
        for i in 0..count {
            let offset = 2 + i * COMPACT_TILE_SIZE;
            let compact = CompactTile::from_bytes(&payload[offset..offset + COMPACT_TILE_SIZE])?;
            tiles.push(compact.to_tile());
        }
        Ok(tiles)
    }
}
