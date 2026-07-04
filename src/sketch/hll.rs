#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperLogLog {
    precision: u8,
    registers: Vec<u8>,
}

impl HyperLogLog {
    pub fn new(precision: u8) -> Self {
        let register_count = 1usize << precision;
        Self {
            precision,
            registers: vec![0; register_count],
        }
    }

    pub fn insert_hash(&mut self, hash: u64) {
        let hash = mix64(hash);
        let index = (hash >> (64 - self.precision)) as usize;
        let shifted = hash << self.precision;
        let max_rank = 64 - u32::from(self.precision) + 1;
        let rank = (shifted.leading_zeros() + 1).min(max_rank) as u8;
        self.registers[index] = self.registers[index].max(rank);
    }

    pub fn estimate(&self) -> u64 {
        let m = self.registers.len() as f64;
        let alpha = match self.registers.len() {
            16 => 0.673,
            32 => 0.697,
            64 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / m),
        };

        let sum: f64 = self
            .registers
            .iter()
            .map(|register| 2.0_f64.powi(-i32::from(*register)))
            .sum();
        let raw = alpha * m * m / sum;

        let zero_registers = self
            .registers
            .iter()
            .filter(|register| **register == 0)
            .count();
        let corrected = if raw <= 2.5 * m && zero_registers > 0 {
            m * (m / zero_registers as f64).ln()
        } else {
            raw
        };

        corrected.round().max(0.0) as u64
    }

    pub fn relative_error(&self) -> f64 {
        1.04 / ((1u64 << self.precision) as f64).sqrt()
    }
}

fn mix64(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^ (value >> 33)
}
