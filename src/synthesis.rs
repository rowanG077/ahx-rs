// Derived from mpg123 1.33.7; LGPL-2.1-or-later. See COPYING and NOTICE.
// Portable execution of the pinned NEON64 float synthesis arithmetic.
use crate::{dct::dct, tables::WINDOW};

pub(crate) struct Synthesis {
    buffers: [[f32; 272]; 2],
    window: [f32; 1056],
    bo: usize,
}

impl Synthesis {
    pub(crate) fn new() -> Self {
        let mut window = [0.; 1056];
        let mut index = 0i32;
        let mut scale = -0.5;
        for i in 0..512 {
            let j = if i < 256 { i } else { 512 - i };
            if index < 528 {
                let v = WINDOW[j] as f32 * scale;
                window[index as usize] = v;
                window[index as usize + 16] = v;
            }
            index += 32;
            if i % 32 == 31 {
                index -= 1023;
            }
            if i % 64 == 63 {
                scale = -scale;
            }
        }
        for i in (512..544).step_by(2) {
            window[i] = 0.;
        }
        for i in 0..512 {
            window[544 + i] = -window[511 - i];
        }
        for i in (0..512).step_by(2) {
            window[i] = -window[i];
        }
        Self {
            buffers: [[0.; 272]; 2],
            window,
            bo: 1,
        }
    }

    pub(crate) fn decode(&mut self, bands: &[f32; 32], out: &mut [i16; 32]) {
        self.bo = self.bo.wrapping_sub(1) & 15;
        let [a, b] = &mut self.buffers;
        let (buf, bo1) = if self.bo & 1 != 0 {
            dct(bands, &mut b[(self.bo + 1) & 15..], &mut a[self.bo..]);
            (&*a, self.bo)
        } else {
            dct(bands, &mut a[self.bo..], &mut b[self.bo + 1..]);
            (&*b, self.bo + 1)
        };
        for (i, sample) in out.iter_mut().enumerate() {
            let wi = 16 - bo1 + i * 32;
            let bi = if i < 16 { i * 16 } else { (32 - i) * 16 };
            let mut sums = [0f32; 4];
            for (lane, sum) in sums.iter_mut().enumerate() {
                *sum = self.window[wi + lane] * buf[bi + lane];
                for group in 1..4 {
                    let k = group * 4 + lane;
                    *sum = fused_multiply_add(self.window[wi + k], buf[bi + k], *sum);
                }
            }
            let sum = (sums[0] + sums[1]) + (sums[2] + sums[3]);
            *sample = ((sum * (1.0 / 32768.0)) * 32767.0) as i16;
        }
    }
}

// Keep the canonical single-rounding operation in both feature configurations.
// Plain a * b + c, or a naive f64 intermediate, can change PCM16 results.
#[inline]
fn fused_multiply_add(a: f32, b: f32, c: f32) -> f32 {
    #[cfg(feature = "std")]
    {
        a.mul_add(b, c)
    }
    #[cfg(not(feature = "std"))]
    {
        libm::fmaf(a, b, c)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn portable_fma_matches_single_rounding() {
        // This product-plus-addition is nonzero only when it is fused.
        let a = f32::from_bits(0x3f80_0001);
        let b = f32::from_bits(0x3f7f_fffe);
        assert_eq!(a * b - 1.0, 0.0);
        assert_ne!(libm::fmaf(a, b, -1.0), 0.0);
        assert_eq!(libm::fmaf(a, b, -1.0), a.mul_add(b, -1.0));

        // Include subnormals, cancellation, signed zero, and overflow. Compare
        // NaNs by class; their payload is not part of the PCM contract.
        let values = [
            0.0,
            -0.0,
            1.0,
            -1.0,
            a,
            b,
            f32::MIN_POSITIVE,
            f32::from_bits(1),
            f32::MAX,
            f32::INFINITY,
        ];
        for a in values {
            for b in values {
                for c in values {
                    let expected = a.mul_add(b, c);
                    let actual = libm::fmaf(a, b, c);
                    if expected.is_nan() {
                        assert!(actual.is_nan());
                    } else {
                        assert_eq!(actual.to_bits(), expected.to_bits(), "{a} * {b} + {c}");
                    }
                }
            }
        }
    }
}
