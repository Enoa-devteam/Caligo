//! Bounded ordinary PNG framing for owned send bytes. This does not decode pixels.

pub const MAX_SEND_BYTES: usize = 128 * 1024;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

/// Validate framing, chunk CRCs, bounded IHDR dimensions and IDAT/IEND ordering.
/// APNG and unknown critical chunks are rejected by this ordinary-image profile.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() > MAX_SEND_BYTES || bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut at = 8usize;
    let mut size = None;
    let mut palette = false;
    let mut idat = false;
    let mut idat_bytes = 0usize;
    let mut idat_ended = false;
    while at < bytes.len() {
        let length =
            u32::from_be_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?) as usize;
        let end = at.checked_add(12)?.checked_add(length)?;
        let chunk = bytes.get(at..end)?;
        let kind: &[u8; 4] = chunk.get(4..8)?.try_into().ok()?;
        if !kind.iter().all(u8::is_ascii_alphabetic)
            || !kind[2].is_ascii_uppercase()
            || crc32(&chunk[4..8 + length])
                != u32::from_be_bytes(chunk[8 + length..].try_into().ok()?)
        {
            return None;
        }
        let data = &chunk[8..8 + length];
        if size.is_none() && kind != b"IHDR" {
            return None;
        }
        match kind {
            b"IHDR" => {
                if at != 8 || size.is_some() || length != 13 {
                    return None;
                }
                let width = u32::from_be_bytes(data[..4].try_into().ok()?);
                let height = u32::from_be_bytes(data[4..8].try_into().ok()?);
                let depth = data[8];
                let color = data[9];
                let valid_depth = match color {
                    0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
                    2 | 4 | 6 => matches!(depth, 8 | 16),
                    3 => matches!(depth, 1 | 2 | 4 | 8),
                    _ => false,
                };
                if !(1..=32768).contains(&width)
                    || !(1..=32768).contains(&height)
                    || u64::from(width) * u64::from(height) > 100_000_000
                    || !valid_depth
                    || data[10] != 0
                    || data[11] != 0
                    || data[12] > 1
                {
                    return None;
                }
                size = Some((width, height));
            }
            b"PLTE" => {
                if palette
                    || idat
                    || length == 0
                    || length > 768
                    || length % 3 != 0
                    || matches!(bytes[25], 0 | 4)
                    || (bytes[25] == 3 && length / 3 > (1usize << bytes[24]))
                {
                    return None;
                }
                palette = true;
            }
            b"IDAT" => {
                if idat_ended || (bytes[25] == 3 && !palette) {
                    return None;
                }
                idat = true;
                idat_bytes += length;
            }
            b"IEND" => {
                return if idat && idat_bytes > 0 && length == 0 && end == bytes.len() {
                    size
                } else {
                    None
                }
            }
            b"acTL" | b"fcTL" | b"fdAT" => return None,
            _ => {
                if kind[0].is_ascii_uppercase() {
                    return None;
                }
                if idat {
                    idat_ended = true;
                }
            }
        }
        at = end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_png_rejects_crc_truncation_animation_and_dimension_errors() {
        let hex="89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082";
        let png: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(dimensions(&png), Some((1, 1)));
        for length in 0..png.len() {
            assert!(dimensions(&png[..length]).is_none());
        }
        let mut corrupt = png.clone();
        corrupt[45] ^= 1;
        assert!(dimensions(&corrupt).is_none());
        let mut trailing = png.clone();
        trailing.push(0);
        assert!(dimensions(&trailing).is_none());
        let mut huge = png.clone();
        huge.resize(MAX_SEND_BYTES + 1, 0);
        assert!(dimensions(&huge).is_none());
        let mut zero = png.clone();
        zero[16..20].copy_from_slice(&0u32.to_be_bytes());
        let crc = crc32(&zero[12..29]);
        zero[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(dimensions(&zero).is_none());
        let mut animation = png[..33].to_vec();
        animation.extend_from_slice(&0u32.to_be_bytes());
        animation.extend_from_slice(b"acTL");
        animation.extend_from_slice(&crc32(b"acTL").to_be_bytes());
        animation.extend_from_slice(&png[33..]);
        assert!(dimensions(&animation).is_none());
    }
}
