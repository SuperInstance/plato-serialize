use super::*;
use uuid::Uuid;

fn sample_tile() -> Tile {
    Tile {
        id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
        tile_type: TileType::Sensor,
        value: ValueType::Float(23.5),
        confidence: 0.95,
        timestamp: 1_700_000_000,
        layer: 1,
    }
}

fn sample_tile_int() -> Tile {
    Tile {
        id: Uuid::parse_str("660e8400-e29b-41d4-a716-446655440001").unwrap(),
        tile_type: TileType::Actuator,
        value: ValueType::Int(42),
        confidence: 0.80,
        timestamp: 1_700_000_001,
        layer: 2,
    }
}

// 1. Single tile encode/decode roundtrip (float)
#[test]
fn roundtrip_float_tile() {
    let tile = sample_tile();
    let encoded = BinaryEncoder::encode_tile(&tile);
    let decoded = BinaryDecoder::decode_tile(&encoded).unwrap();
    assert_eq!(tile, decoded);
}

// 2. Single tile encode/decode roundtrip (int)
#[test]
fn roundtrip_int_tile() {
    let tile = sample_tile_int();
    let encoded = BinaryEncoder::encode_tile(&tile);
    let decoded = BinaryDecoder::decode_tile(&encoded).unwrap();
    assert_eq!(tile, decoded);
}

// 3. Batch encode/decode roundtrip
#[test]
fn roundtrip_batch() {
    let tiles = vec![sample_tile(), sample_tile_int(), sample_tile()];
    let encoded = BinaryEncoder::encode_batch(&tiles);
    let decoded = BinaryDecoder::decode_batch(&encoded).unwrap();
    assert_eq!(tiles, decoded);
}

// 4. CRC16 correctness with known vector
#[test]
fn crc16_known() {
    // CRC-16/XMODEM of "123456789" is 0x31C3
    let crc = crc16(b"123456789");
    assert_eq!(crc, 0x31C3);
}

// 5. CRC16 of empty data is 0
#[test]
fn crc16_empty() {
    assert_eq!(crc16(b""), 0);
}

// 6. Wire header serialization roundtrip
#[test]
fn wire_header_roundtrip() {
    let header = WireHeader {
        version: 1,
        format: WireFormat::Binary,
        payload_len: 43,
        checksum: 0xABCD,
    };
    let bytes = header.to_bytes();
    let restored = WireHeader::from_bytes(&bytes).unwrap();
    assert_eq!(header, restored);
}

// 7. Wire header is exactly 6 bytes
#[test]
fn wire_header_size() {
    assert_eq!(WireHeader::SIZE, 6);
    let header = WireHeader {
        version: 1,
        format: WireFormat::Compact,
        payload_len: 0,
        checksum: 0,
    };
    assert_eq!(header.to_bytes().len(), 6);
}

// 8. Size comparison: binary < JSON
#[test]
fn binary_smaller_than_json() {
    let tile = sample_tile();
    let json_bytes = serde_json::to_vec(&tile).unwrap();
    let binary_bytes = BinaryEncoder::encode_tile(&tile);
    let json_size = json_bytes.len();
    let bin_size = binary_bytes.len();
    // Binary should be substantially smaller
    assert!(bin_size < json_size, "binary ({bin_size}) should be < json ({json_size})");
    // Check ~60% smaller (binary <= 50% of json)
    let ratio = bin_size as f64 / json_size as f64;
    assert!(ratio < 0.55, "binary/json ratio = {ratio:.2}, expected < 0.55");
}

// 9. Corrupted data detection (bad CRC)
#[test]
fn corrupted_data_detected() {
    let tile = sample_tile();
    let mut encoded = BinaryEncoder::encode_tile(&tile);
    // Flip a bit in the payload
    let payload_start = WireHeader::SIZE;
    encoded[payload_start + 2] ^= 0xFF;
    let result = BinaryDecoder::decode_tile(&encoded);
    assert!(matches!(result, Err(SerializeError::CrcMismatch { .. })));
}

// 10. Truncated data handling (too short for header)
#[test]
fn truncated_header() {
    let data = [0u8; 3];
    let result = BinaryDecoder::decode_tile(&data);
    assert!(matches!(result, Err(SerializeError::TruncatedHeader { .. })));
}

// 11. Empty batch roundtrip
#[test]
fn empty_batch() {
    let tiles: Vec<Tile> = vec![];
    let encoded = BinaryEncoder::encode_batch(&tiles);
    let decoded = BinaryDecoder::decode_batch(&encoded).unwrap();
    assert!(decoded.is_empty());
}

// 12. Single byte input
#[test]
fn single_byte_input() {
    let result = BinaryDecoder::decode_tile(&[0u8; 1]);
    assert!(matches!(result, Err(SerializeError::TruncatedHeader { .. })));
}

// 13. Exact header but no payload
#[test]
fn header_only() {
    let header = WireHeader {
        version: 1,
        format: WireFormat::Binary,
        payload_len: 43,
        checksum: 0x1234,
    };
    let data = header.to_bytes().to_vec();
    let result = BinaryDecoder::decode_tile(&data);
    assert!(matches!(result, Err(SerializeError::TruncatedPayload { .. })));
}

// 14. Compact tile roundtrip at byte level
#[test]
fn compact_tile_bytes_roundtrip() {
    let tile = sample_tile();
    let compact = CompactTile::from_tile(&tile);
    let bytes = compact.to_bytes();
    assert_eq!(bytes.len(), COMPACT_TILE_SIZE);
    let restored = CompactTile::from_bytes(&bytes).unwrap();
    assert_eq!(compact, restored);
}

// 15. Unknown format byte
#[test]
fn unknown_format() {
    let data = [1, 0xFF, 0, 0, 0, 0];
    let result = WireHeader::from_bytes(&data);
    assert!(matches!(result, Err(SerializeError::UnknownFormat(0xFF))));
}

// 16. Unsupported version
#[test]
fn unsupported_version() {
    let tile = sample_tile();
    let mut encoded = BinaryEncoder::encode_tile(&tile);
    encoded[0] = 99; // bad version
    let result = BinaryDecoder::decode_tile(&encoded);
    assert!(matches!(result, Err(SerializeError::UnsupportedVersion(99))));
}

// 17. Batch size comparison
#[test]
fn batch_binary_smaller() {
    let tiles = vec![sample_tile(), sample_tile_int()];
    let json_bytes = serde_json::to_vec(&tiles).unwrap();
    let binary_bytes = BinaryEncoder::encode_batch(&tiles);
    assert!(binary_bytes.len() < json_bytes.len());
}

// 18. Max payload edge case — large batch
#[test]
fn large_batch() {
    let mut tiles = Vec::new();
    for i in 0..100u32 {
        tiles.push(Tile {
            id: Uuid::new_v4(),
            tile_type: TileType::Sensor,
            value: ValueType::Float(i as f64),
            confidence: 0.5,
            timestamp: i as u64,
            layer: (i % 5) as u8,
        });
    }
    let encoded = BinaryEncoder::encode_batch(&tiles);
    let decoded = BinaryDecoder::decode_batch(&encoded).unwrap();
    assert_eq!(tiles.len(), decoded.len());
    for (a, b) in tiles.iter().zip(decoded.iter()) {
        assert_eq!(a, b);
    }
}

// 19. Compact tile size is 43 bytes
#[test]
fn compact_size_constant() {
    assert_eq!(COMPACT_TILE_SIZE, 43);
}

// 20. WireFormat display
#[test]
fn wire_format_display() {
    assert_eq!(format!("{}", WireFormat::Json), "Json");
    assert_eq!(format!("{}", WireFormat::Compact), "Compact");
    assert_eq!(format!("{}", WireFormat::Binary), "Binary");
}
