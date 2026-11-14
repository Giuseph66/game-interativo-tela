//! Gerador pseudoaleatorio minimo (xorshift64*), sem dependencias e deterministico por semente.

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// `seed == 0` usa uma semente derivada do relogio.
    pub fn new(seed: u64) -> Self {
        let s = if seed != 0 {
            seed
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15)
        };
        // splitmix para espalhar sementes proximas; estado nunca pode ser zero
        let mut z = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Self(if z == 0 { 0x2545_F491_4F6C_DD1D } else { z })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniforme em [0, 1).
    pub fn f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) / (1u32 << 24) as f32
    }

    /// Uniforme em [lo, hi).
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    /// `true` com probabilidade `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    /// Indice sorteado proporcionalmente aos pesos (>= 0). `None` se a soma for 0.
    pub fn weighted(&mut self, weights: &[f32]) -> Option<usize> {
        let total: f32 = weights.iter().map(|w| w.max(0.0)).sum();
        if total <= 0.0 {
            return None;
        }
        let mut roll = self.f32() * total;
        for (i, w) in weights.iter().enumerate() {
            let w = w.max(0.0);
            if roll < w {
                return Some(i);
            }
            roll -= w;
        }
        weights.iter().rposition(|w| *w > 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence_and_range_ok() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            let v = r.f32();
            assert!((0.0..1.0).contains(&v));
            let x = r.range(-3.0, 5.0);
            assert!((-3.0..5.0).contains(&x));
        }
    }

    #[test]
    fn weighted_respects_zero_weights_and_proportions() {
        let mut r = Rng::new(1);
        assert_eq!(r.weighted(&[]), None);
        assert_eq!(r.weighted(&[0.0, 0.0]), None);
        let mut counts = [0usize; 3];
        for _ in 0..20_000 {
            counts[r.weighted(&[1.0, 0.0, 3.0]).unwrap()] += 1;
        }
        assert_eq!(counts[1], 0);
        let ratio = counts[2] as f32 / counts[0] as f32;
        assert!((2.5..3.6).contains(&ratio), "ratio {ratio}");
    }

    #[test]
    fn chance_extremes() {
        let mut r = Rng::new(3);
        assert!((0..1000).all(|_| !r.chance(0.0)));
        assert!((0..1000).all(|_| r.chance(1.0)));
    }
}
