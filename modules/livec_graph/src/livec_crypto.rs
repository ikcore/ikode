/// Fixed sizes:
/// - plaintext:  20 bytes (u64 + u64 + u32)
/// - ciphertext: 20 bytes
/// - base32:     32 chars (because ceil(20*8/5) = 32)
///
/// NOT CRYPTOGRAPHICALLY SECURE. It's an obfuscation/ID encoding.
pub mod fast_id_codec {
    // ============================================================
    // Crockford Base32
    // ============================================================

    pub const CROCKFORD_BASE32: &[u8; 32] =
        //b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        b"0123456789abcdefghjkmnpqrstvwxyz";

    pub const CROCKFORD_DECODE: [i8; 256] = {
        let mut t = [-1i8; 256];

        // digits
        t[b'0' as usize] = 0;
        t[b'1' as usize] = 1;
        t[b'2' as usize] = 2;
        t[b'3' as usize] = 3;
        t[b'4' as usize] = 4;
        t[b'5' as usize] = 5;
        t[b'6' as usize] = 6;
        t[b'7' as usize] = 7;
        t[b'8' as usize] = 8;
        t[b'9' as usize] = 9;

        // letters
        t[b'A' as usize] = 10;
        t[b'a' as usize] = 10;
        t[b'B' as usize] = 11;
        t[b'b' as usize] = 11;
        t[b'C' as usize] = 12;
        t[b'c' as usize] = 12;
        t[b'D' as usize] = 13;
        t[b'd' as usize] = 13;
        t[b'E' as usize] = 14;
        t[b'e' as usize] = 14;
        t[b'F' as usize] = 15;
        t[b'f' as usize] = 15;
        t[b'G' as usize] = 16;
        t[b'g' as usize] = 16;
        t[b'H' as usize] = 17;
        t[b'h' as usize] = 17;
        t[b'J' as usize] = 18;
        t[b'j' as usize] = 18;
        t[b'K' as usize] = 19;
        t[b'k' as usize] = 19;
        t[b'M' as usize] = 20;
        t[b'm' as usize] = 20;
        t[b'N' as usize] = 21;
        t[b'n' as usize] = 21;
        t[b'P' as usize] = 22;
        t[b'p' as usize] = 22;
        t[b'Q' as usize] = 23;
        t[b'q' as usize] = 23;
        t[b'R' as usize] = 24;
        t[b'r' as usize] = 24;
        t[b'S' as usize] = 25;
        t[b's' as usize] = 25;
        t[b'T' as usize] = 26;
        t[b't' as usize] = 26;
        t[b'V' as usize] = 27;
        t[b'v' as usize] = 27;
        t[b'W' as usize] = 28;
        t[b'w' as usize] = 28;
        t[b'X' as usize] = 29;
        t[b'x' as usize] = 29;
        t[b'Y' as usize] = 30;
        t[b'y' as usize] = 30;
        t[b'Z' as usize] = 31;
        t[b'z' as usize] = 31;

        // tolerant aliases
        t[b'O' as usize] = 0;
        t[b'o' as usize] = 0;
        t[b'I' as usize] = 1;
        t[b'i' as usize] = 1;
        t[b'L' as usize] = 1;
        t[b'l' as usize] = 1;

        t
    };

    // ============================================================
    // Reversible diffusion (avalanche)
    // ============================================================

    #[inline(always)]
    fn feistel_round(x: u32, k: u32) -> u32 {
        let mut v = x ^ k;
        v ^= v.rotate_left(5);
        v = v.wrapping_mul(0x9E3779B9);
        v ^ (v >> 16)
    }

    #[inline(always)]
    fn f16(x: u16, k: u16) -> u16 {
        // tiny fast mixing function on 16 bits
        let mut v = x ^ k;
        v ^= v.rotate_left(5);
        v = v.wrapping_mul(0x9E37u16);
        v ^ (v >> 7)
    }

    #[inline(always)]
    fn mix32(x: u32, key: u64) -> u32 {
        let mut l = (x & 0xFFFF) as u16;
        let mut r = (x >> 16) as u16;

        let k0 = (key as u16) ^ 0xA3C5;
        let k1 = ((key >> 16) as u16) ^ 0x7F4A;
        let k2 = ((key >> 32) as u16) ^ 0x1D2B;

        // 3-round Feistel on 32 bits (16/16)
        let t = l ^ f16(r, k0);
        l = r;
        r = t;
        let t = l ^ f16(r, k1);
        l = r;
        r = t;
        let t = l ^ f16(r, k2);
        l = r;
        r = t;

        ((r as u32) << 16) | (l as u32)
    }

    #[inline(always)]
    fn unmix32(x: u32, key: u64) -> u32 {
        let mut l = (x & 0xFFFF) as u16;
        let mut r = (x >> 16) as u16;

        let k0 = (key as u16) ^ 0xA3C5;
        let k1 = ((key >> 16) as u16) ^ 0x7F4A;
        let k2 = ((key >> 32) as u16) ^ 0x1D2B;

        // reverse
        let t = r ^ f16(l, k2);
        r = l;
        l = t;
        let t = r ^ f16(l, k1);
        r = l;
        l = t;
        let t = r ^ f16(l, k0);
        r = l;
        l = t;

        ((r as u32) << 16) | (l as u32)
    }

    #[inline(always)]
    fn mix64(x: u64, key: u64) -> u64 {
        let mut l = x as u32;
        let mut r = (x >> 32) as u32;

        let k0 = key as u32;
        let k1 = (key >> 32) as u32;
        let k2 = k0 ^ 0xA5A5A5A5;

        // 3-round Feistel
        let t = l ^ feistel_round(r, k0);
        l = r;
        r = t;
        let t = l ^ feistel_round(r, k1);
        l = r;
        r = t;
        let t = l ^ feistel_round(r, k2);
        l = r;
        r = t;

        (r as u64) << 32 | l as u64
    }

    #[inline(always)]
    fn unmix64(x: u64, key: u64) -> u64 {
        let mut l = x as u32;
        let mut r = (x >> 32) as u32;

        let k0 = key as u32;
        let k1 = (key >> 32) as u32;
        let k2 = k0 ^ 0xA5A5A5A5;

        // reverse order
        let t = r ^ feistel_round(l, k2);
        r = l;
        l = t;
        let t = r ^ feistel_round(l, k1);
        r = l;
        l = t;
        let t = r ^ feistel_round(l, k0);
        r = l;
        l = t;

        (r as u64) << 32 | l as u64
    }

    // ============================================================
    // SplitMix64 keystream (XOR obfuscation)
    // ============================================================

    #[inline(always)]
    fn splitmix64(x: &mut u64) -> u64 {
        *x = x.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = *x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    #[inline(always)]
    fn keystream_20(key: u64) -> [u8; 20] {
        let mut s = key;
        let a = splitmix64(&mut s).to_le_bytes();
        let b = splitmix64(&mut s).to_le_bytes();
        let c = splitmix64(&mut s).to_le_bytes();

        let mut out = [0u8; 20];
        out[0..8].copy_from_slice(&a);
        out[8..16].copy_from_slice(&b);
        out[16..20].copy_from_slice(&c[..4]);
        out
    }

    #[inline(always)]
    fn derive_tweak(target: u64, ty: u32, key: u64) -> u64 {
        // Cheap, reversible diffusion into 64 bits
        let mut x = target ^ ((ty as u64) << 32) ^ key;
        x ^= x.rotate_left(17);
        x = x.wrapping_mul(0x9E3779B97F4A7C15);
        x ^= x >> 29;
        x
    }

    // ============================================================
    // Packing
    // ============================================================

    #[inline(always)]
    fn pack(tx: u64, target: u64, ty: u32, key: u64) -> [u8; 20] {
        // Derive tweak from target + ty
        let tweak = derive_tweak(target, ty, key);

        // Tx is now influenced by target + ty
        let tx_m = mix64(tx ^ tweak, key);

        // Target influenced by tx
        let target_m = mix64(target ^ tx_m, key ^ 0xDEADBEEFDEADBEEF);

        // Ty influenced by tx
        let ty_m = mix32(ty ^ (tx_m as u32), key ^ 0xBADC0FFEE0DDF00D);

        let mut out = [0u8; 20];
        out[0..8].copy_from_slice(&tx_m.to_le_bytes());
        out[8..16].copy_from_slice(&target_m.to_le_bytes());
        out[16..20].copy_from_slice(&ty_m.to_le_bytes());
        out
    }

    #[inline(always)]
    fn unpack(bytes: [u8; 20], key: u64) -> (u64, u64, u32) {
        let tx_m = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let target_m = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let ty_m = u32::from_le_bytes(bytes[16..20].try_into().unwrap());

        // Recover target first
        let target = unmix64(target_m, key ^ 0xDEADBEEFDEADBEEF) ^ tx_m;

        // Recover ty
        let ty = unmix32(ty_m, key ^ 0xBADC0FFEE0DDF00D) ^ (tx_m as u32);

        // Recompute tweak
        let tweak = derive_tweak(target, ty, key);

        // Recover tx
        let tx = unmix64(tx_m, key) ^ tweak;

        (tx, target, ty)
    }

    #[inline(always)]
    fn xor20(mut data: [u8; 20], key: u64) -> [u8; 20] {
        let ks = keystream_20(key);
        for i in 0..20 {
            data[i] ^= ks[i];
        }
        data
    }

    // ============================================================
    // Base32 encode/decode (20 <-> 32)
    // ============================================================

    #[inline]
    fn encode_20_to_32(input: [u8; 20]) -> [u8; 32] {
        let mut out = [0u8; 32];
        let mut buf = 0u32;
        let mut bits = 0;
        let mut j = 0;

        for &b in &input {
            buf = (buf << 8) | b as u32;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out[j] = CROCKFORD_BASE32[((buf >> bits) & 31) as usize];
                j += 1;
            }
        }
        out
    }

    #[inline]
    fn decode_32_to_20(input: &[u8]) -> Option<[u8; 20]> {
        if input.len() != 32 {
            return None;
        }

        let mut out = [0u8; 20];
        let mut buf = 0u32;
        let mut bits = 0;
        let mut j = 0;

        for &c in input {
            let v = CROCKFORD_DECODE[c as usize];
            if v < 0 {
                return None;
            }
            buf = (buf << 5) | v as u32;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                out[j] = ((buf >> bits) & 0xFF) as u8;
                j += 1;
            }
        }
        Some(out)
    }

    // ============================================================
    // Public API
    // ============================================================

    #[inline]
    pub fn encrypt(tx: u64, target: u64, ty: u32, key: u64) -> [u8; 32] {
        let packed = pack(tx, target, ty, key);
        let xored = xor20(packed, key);
        encode_20_to_32(xored)
    }

    #[inline]
    pub fn decrypt(input: &[u8], key: u64) -> Option<(u64, u64, u32)> {
        let decoded = decode_32_to_20(input)?;
        let plain = xor20(decoded, key);
        Some(unpack(plain, key))
    }

    #[inline]
    pub fn encrypt_string(tx: u64, target: u64, ty: u32, key: u64) -> String {
        unsafe { String::from_utf8_unchecked(encrypt(tx, target, ty, key).to_vec()) }
    }

    #[inline]
    pub fn decrypt_string(input: &str, key: u64) -> Option<(u64, u64, u32)> {
        decrypt(input.as_bytes(), key)
    }
}

#[cfg(test)]
mod tests {
    use super::fast_id_codec::*;

    #[test]
    fn round_trip() {
        let key = 0xD1CE_BEEF_1234_5678u64;
        let tx = 1234567890123456789u64;
        let tgt = 9876543210987654321u64;
        let ty = 42u32;

        let enc = encrypt(tx, tgt, ty, key);
        let dec = decrypt(&enc, key).unwrap();
        assert_eq!(dec, (tx, tgt, ty));
    }

    #[test]
    fn round_trip_strings() {
        let key = 0xD1CE_BEEF_1234_5678u64;
        let tx = 1234567890123456789u64;
        let tgt = 9876543210987654321u64;
        let ty = 42u32;

        let enc = encrypt_string(tx, tgt, ty, key);
        let dec = decrypt_string(&enc, key).unwrap();
        assert_eq!(dec, (tx, tgt, ty));
    }

    #[test]
    fn string_differences() {
        let key = 0xD1CE_BEEF_1234_5678u64;

        let enc0 = encrypt_string(0, 0, 2, key);
        let enc1 = encrypt_string(0, 1, 2, key);

        println!("{}", enc0);
        println!("{}", enc1);
    }
}
