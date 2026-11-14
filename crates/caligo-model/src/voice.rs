//! Bounded Tencent SILK v3 framing for the fixed short private-send profile.
//! Packet payload decoding belongs to media tooling.

pub const MAX_SEND_BYTES: usize = 128 * 1024;
const MAX_PACKETS: usize = 50;

pub fn validate_short_tencent_silk(silk: &[u8]) -> bool {
    let Some(mut packets) = silk.strip_prefix(b"\x02#!SILK_V3") else {
        return false;
    };
    if silk.len() > MAX_SEND_BYTES || packets.is_empty() {
        return false;
    }
    let mut count = 0;
    while !packets.is_empty() {
        let Some((len, rest)) = packets.split_first_chunk::<2>() else {
            return false;
        };
        let size = i16::from_le_bytes(*len);
        count += 1;
        if size <= 0 || count > MAX_PACKETS {
            return false;
        }
        let Some(remaining) = rest.get(size as usize..) else {
            return false;
        };
        packets = remaining;
    }
    true
}
