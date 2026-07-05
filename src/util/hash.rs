const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Clone)]
pub struct StableHasher {
    hash: u64,
}

impl StableHasher {
    pub fn new() -> Self {
        Self {
            hash: FNV_OFFSET_BASIS,
        }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.hash ^= u64::from(*byte);
            self.hash = self.hash.wrapping_mul(FNV_PRIME);
        }
    }

    pub fn finish(&self) -> u64 {
        self.hash
    }
}

impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}

pub fn stable_u64(bytes: &[u8]) -> u64 {
    stable_u64_parts(&[bytes])
}

pub fn stable_u64_parts(parts: &[&[u8]]) -> u64 {
    let mut hasher = StableHasher::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finish()
}

pub fn stable_hex(bytes: &[u8]) -> String {
    format!("{:016x}", stable_u64(bytes))
}
