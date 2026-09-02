//! Port of `ImageProvider.CreateHashFromImageList` (`ImageProvider.cs:294`).
//!
//! The C# writes each `ProviderImageInfo` to a `BinaryWriter` — string
//! as 7-bit-encoded length + UTF-8 bytes, size as little-endian
//! `long` — SHA-1 hashes the stream, and encodes with `cYo` Base32
//! (`A-Z2-7`, no padding). The result is the archive's cache key, so
//! the encoding must stay byte-compatible.

use std::io::Write;

use sha1::{Digest, Sha1};

use crate::provider::ProviderImageInfo;

fn write_binary_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let mut len = bytes.len();
    while len >= 0x80 {
        out.push(((len & 0x7f) | 0x80) as u8);
        len >>= 7;
    }
    out.push(len as u8);
    out.extend_from_slice(bytes);
}

const BASE32_CHARS: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Port of `Base32.ToBase32String` — RFC 4648 alphabet, no padding;
/// the remainder emits 2/4/5/7 chars for 1-4 trailing bytes, exactly
/// as the C# switch does.
fn base32(data: &[u8]) -> String {
    let mut out = String::new();
    let mut push = |v: u32| out.push(BASE32_CHARS[(v & 0x1f) as usize] as char);
    let groups = data.len() / 5;
    let mut i = 0;
    for _ in 0..groups {
        let (b, b2, b3, b4, b5) = (
            data[i] as u32,
            data[i + 1] as u32,
            data[i + 2] as u32,
            data[i + 3] as u32,
            data[i + 4] as u32,
        );
        i += 5;
        push(b >> 3);
        push(((b << 2) & 0x1f) | (b2 >> 6));
        push((b2 >> 1) & 0x1f);
        push(((b2 << 4) & 0x1f) | (b3 >> 4));
        push(((b3 << 1) & 0x1f) | (b4 >> 7));
        push((b4 >> 2) & 0x1f);
        push(((b4 << 3) & 0x1f) | (b5 >> 5));
        push(b5 & 0x1f);
    }
    let rem = data.len() - i;
    if rem > 0 {
        let b6 = data[i] as u32;
        push(b6 >> 3);
        match rem {
            1 => push((b6 << 2) & 0x1f),
            2 => {
                let b7 = data[i + 1] as u32;
                push(((b6 << 2) & 0x1f) | (b7 >> 6));
                push((b7 >> 1) & 0x1f);
                push((b7 << 4) & 0x1f);
            }
            3 => {
                let (b7, b8) = (data[i + 1] as u32, data[i + 2] as u32);
                push(((b6 << 2) & 0x1f) | (b7 >> 6));
                push((b7 >> 1) & 0x1f);
                push(((b7 << 4) & 0x1f) | (b8 >> 4));
                push((b8 << 1) & 0x1f);
            }
            4 => {
                let (b7, b8, b9) = (data[i + 1] as u32, data[i + 2] as u32, data[i + 3] as u32);
                push(((b6 << 2) & 0x1f) | (b7 >> 6));
                push((b7 >> 1) & 0x1f);
                push(((b7 << 4) & 0x1f) | (b8 >> 4));
                push(((b8 << 1) & 0x1f) | (b9 >> 7));
                push((b9 >> 2) & 0x1f);
                push((b9 << 3) & 0x1f);
            }
            _ => unreachable!(),
        }
    }
    out
}

/// `CreateHashFromImageList` — SHA-1 over the image list, Base32
/// encoded.
pub fn create_hash_from_image_list(images: &[ProviderImageInfo]) -> String {
    let mut buf = Vec::new();
    for image in images {
        write_binary_string(&mut buf, &image.name);
        buf.write_all(&image.size.to_le_bytes()).unwrap();
    }
    let mut sha = Sha1::new();
    sha.update(&buf);
    base32(&sha.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_known_vectors() {
        // RFC 4648 test vectors (no padding).
        assert_eq!(base32(b""), "");
        assert_eq!(base32(b"f"), "MY");
        assert_eq!(base32(b"fo"), "MZXQ");
        assert_eq!(base32(b"foo"), "MZXW6");
        assert_eq!(base32(b"foob"), "MZXW6YQ");
        assert_eq!(base32(b"fooba"), "MZXW6YTB");
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn hash_is_stable() {
        let images = vec![
            ProviderImageInfo::new(0, "a.jpg", 100),
            ProviderImageInfo::new(1, "b.jpg", 200),
        ];
        let h1 = create_hash_from_image_list(&images);
        let h2 = create_hash_from_image_list(&images);
        assert_eq!(h1, h2);
        // SHA-1 output is 20 bytes -> exactly 32 base32 chars.
        assert_eq!(h1.len(), 32);
    }
}
