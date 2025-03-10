use itertools::{izip, Itertools};
use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{
    backend::{ArithmeticOps, ModInit, ModularOpsU64, Modulus},
    utils::{mod_exponent, mod_inverse, ShoupMul},
};

pub trait NttInit<M> {
    /// Ntt istance must be compatible across different instances with same `q`
    /// and `n`
    fn new(q: &M, n: usize) -> Self;
}

pub trait Ntt {
    type Element;
    fn forward_lazy(&self, v: &mut [Self::Element]);
    fn forward(&self, v: &mut [Self::Element]);
    fn backward_lazy(&self, v: &mut [Self::Element]);
    fn backward(&self, v: &mut [Self::Element]);
}

/// Forward butterfly routine for Number theoretic transform. Given inputs `x <
/// 4q` and `y < 4q` mutates x and y in place to equal x' and y' where
/// x' = x + wy
/// y' = x - wy
/// and both x' and y' are \in [0, 4q)
///
/// Implements Algorithm 4 of [FASTER ARITHMETIC FOR NUMBER-THEORETIC TRANSFORMS](https://arxiv.org/pdf/1205.2926.pdf)
pub fn forward_butterly_0_to_4q(
    mut x: u64,
    y: u64,
    w: u64,
    w_shoup: u64,
    q: u64,
    q_twice: u64,
) -> (u64, u64) {
    debug_assert!(x < q * 4, "{} >= (4q){}", x, 4 * q);
    debug_assert!(y < q * 4, "{} >= (4q){}", y, 4 * q);

    if x >= q_twice {
        x = x - q_twice;
    }

    let t = ShoupMul::mul(y, w, w_shoup, q);

    (x + t, x + q_twice - t)
}

pub fn forward_butterly_0_to_2q(
    mut x: u64,
    y: u64,
    w: u64,
    w_shoup: u64,
    q: u64,
    q_twice: u64,
) -> (u64, u64) {
    debug_assert!(x < q * 4, "{} >= (4q){}", x, 4 * q);
    debug_assert!(y < q * 4, "{} >= (4q){}", y, 4 * q);

    if x >= q_twice {
        x = x - q_twice;
    }

    let t = ShoupMul::mul(y, w, w_shoup, q);

    let ox = x.wrapping_add(t);
    let oy = x.wrapping_sub(t);

    (
        (ox).min(ox.wrapping_sub(q_twice)),
        oy.min(oy.wrapping_add(q_twice)),
    )
}

/// Inverse butterfly routine of Inverse Number theoretic transform. Given
/// inputs `x < 2q` and `y < 2q` mutates x and y to equal x' and y' where
/// x'= x + y
/// y' = w(x - y)
/// and both x' and y' are \in [0, 2q)
///
/// Implements Algorithm 3 of [FASTER ARITHMETIC FOR NUMBER-THEORETIC TRANSFORMS](https://arxiv.org/pdf/1205.2926.pdf)
pub fn inverse_butterfly_0_to_2q(
    x: u64,
    y: u64,
    w_inv: u64,
    w_inv_shoup: u64,
    q: u64,
    q_twice: u64,
) -> (u64, u64) {
    debug_assert!(x < q_twice, "{} >= (2q){q_twice}", x);
    debug_assert!(y < q_twice, "{} >= (2q){q_twice}", y);

    let mut x_dash = x + y;
    if x_dash >= q_twice {
        x_dash -= q_twice
    }

    let t = x + q_twice - y;
    let y = ShoupMul::mul(t, w_inv, w_inv_shoup, q);

    (x_dash, y)
}

/// Number theoretic transform of vector `a` where each element can be in range
/// [0, 2q). Outputs NTT(a) where each element is in range [0,2q)
///
/// Implements Cooley-tukey based forward NTT as given in Algorithm 1 of https://eprint.iacr.org/2016/504.pdf.
pub fn ntt_lazy(a: &mut [u64], psi: &[u64], psi_shoup: &[u64], q: u64, q_twice: u64) {
    assert!(a.len() == psi.len());

    let n = a.len();
    let mut t = n;

    let mut m = 1;
    while m < n {
        t >>= 1;
        let w = &psi[m..];
        let w_shoup = &psi_shoup[m..];

        if t == 1 {
            for (a, w, w_shoup) in izip!(a.chunks_mut(2), w.iter(), w_shoup.iter()) {
                let (ox, oy) = forward_butterly_0_to_2q(a[0], a[1], *w, *w_shoup, q, q_twice);
                a[0] = ox;
                a[1] = oy;
            }
        } else {
            for i in 0..m {
                let a = &mut a[2 * i * t..(2 * (i + 1) * t)];
                let (left, right) = a.split_at_mut(t);

                for (x, y) in izip!(left.iter_mut(), right.iter_mut()) {
                    let (ox, oy) = forward_butterly_0_to_4q(*x, *y, w[i], w_shoup[i], q, q_twice);
                    *x = ox;
                    *y = oy;
                }
            }
        }

        m <<= 1;
    }
}

/// Same as `ntt_lazy` with output in range [0, q)
pub fn ntt(a: &mut [u64], psi: &[u64], psi_shoup: &[u64], q: u64, q_twice: u64) {
    assert!(a.len() == psi.len());

    let n = a.len();
    let mut t = n;

    let mut m = 1;
    while m < n {
        t >>= 1;
        let w = &psi[m..];
        let w_shoup = &psi_shoup[m..];

        if t == 1 {
            for (a, w, w_shoup) in izip!(a.chunks_mut(2), w.iter(), w_shoup.iter()) {
                let (ox, oy) = forward_butterly_0_to_2q(a[0], a[1], *w, *w_shoup, q, q_twice);
                // reduce from range [0, 2q) to [0, q)
                a[0] = ox.min(ox.wrapping_sub(q));
                a[1] = oy.min(oy.wrapping_sub(q));
            }
        } else {
            for i in 0..m {
                let a = &mut a[2 * i * t..(2 * (i + 1) * t)];
                let (left, right) = a.split_at_mut(t);

                for (x, y) in izip!(left.iter_mut(), right.iter_mut()) {
                    let (ox, oy) = forward_butterly_0_to_4q(*x, *y, w[i], w_shoup[i], q, q_twice);
                    *x = ox;
                    *y = oy;
                }
            }
        }

        m <<= 1;
    }
}

/// Inverse number theoretic transform of input vector `a` with each element can
/// be in range [0, 2q). Outputs vector INTT(a) with each element in range [0,
/// 2q)
///
/// Implements backward number theorectic transform using GS algorithm as given in Algorithm 2 of https://eprint.iacr.org/2016/504.pdf
pub fn ntt_inv_lazy(
    a: &mut [u64],
    psi_inv: &[u64],
    psi_inv_shoup: &[u64],
    n_inv: u64,
    n_inv_shoup: u64,
    q: u64,
    q_twice: u64,
) {
    assert!(a.len() == psi_inv.len());

    let mut m = a.len() >> 1;
    let mut t = 1;

    while m > 0 {
        if m == 1 {
            let (left, right) = a.split_at_mut(t);

            for (x, y) in izip!(left.iter_mut(), right.iter_mut()) {
                let (ox, oy) =
                    inverse_butterfly_0_to_2q(*x, *y, psi_inv[1], psi_inv_shoup[1], q, q_twice);
                *x = ShoupMul::mul(ox, n_inv, n_inv_shoup, q);
                *y = ShoupMul::mul(oy, n_inv, n_inv_shoup, q);
            }
        } else {
            let w_inv = &psi_inv[m..];
            let w_inv_shoup = &psi_inv_shoup[m..];
            for i in 0..m {
                let a = &mut a[2 * i * t..2 * (i + 1) * t];
                let (left, right) = a.split_at_mut(t);

                for (x, y) in izip!(left.iter_mut(), right.iter_mut()) {
                    let (ox, oy) =
                        inverse_butterfly_0_to_2q(*x, *y, w_inv[i], w_inv_shoup[i], q, q_twice);
                    *x = ox;
                    *y = oy;
                }
            }
        }

        t *= 2;
        m >>= 1;
    }
}

/// Same as `ntt_inv_lazy` with output in range [0, q)
pub fn ntt_inv(
    a: &mut [u64],
    psi_inv: &[u64],
    psi_inv_shoup: &[u64],
    n_inv: u64,
    n_inv_shoup: u64,
    q: u64,
    q_twice: u64,
) {
    assert!(a.len() == psi_inv.len());

    let mut m = a.len() >> 1;
    let mut t = 1;

    while m > 0 {
        if m == 1 {
            let (left, right) = a.split_at_mut(t);

            for (x, y) in izip!(left.iter_mut(), right.iter_mut()) {
                let (ox, oy) =
                    inverse_butterfly_0_to_2q(*x, *y, psi_inv[1], psi_inv_shoup[1], q, q_twice);
                let ox = ShoupMul::mul(ox, n_inv, n_inv_shoup, q);
                let oy = ShoupMul::mul(oy, n_inv, n_inv_shoup, q);
                *x = ox.min(ox.wrapping_sub(q));
                *y = oy.min(oy.wrapping_sub(q));
            }
        } else {
            let w_inv = &psi_inv[m..];
            let w_inv_shoup = &psi_inv_shoup[m..];
            for i in 0..m {
                let a = &mut a[2 * i * t..2 * (i + 1) * t];
                let (left, right) = a.split_at_mut(t);

                for (x, y) in izip!(left.iter_mut(), right.iter_mut()) {
                    let (ox, oy) =
                        inverse_butterfly_0_to_2q(*x, *y, w_inv[i], w_inv_shoup[i], q, q_twice);
                    *x = ox;
                    *y = oy;
                }
            }
        }

        t *= 2;
        m >>= 1;
    }
}

/// Find n^{th} root of unity in field F_q, if one exists
///
/// Note: n^{th} root of unity exists if and only if $q = 1 \mod{n}$
pub(crate) fn find_primitive_root<R: RngCore>(q: u64, n: u64, rng: &mut R) -> Option<u64> {
    assert!(n.is_power_of_two(), "{n} is not power of two");

    // n^th root of unity only exists if n|(q-1)
    assert!(q % n == 1, "{n}^th root of unity in F_{q} does not exists");

    let t = (q - 1) / n;

    for _ in 0..100 {
        let mut omega = rng.gen::<u64>() % q;

        // \omega = \omega^t. \omega is now n^th root of unity
        omega = mod_exponent(omega, t, q);

        // We restrict n to be power of 2. Thus checking whether \omega is primitive
        // n^th root of unity is as simple as checking: \omega^{n/2} != 1
        if mod_exponent(omega, n >> 1, q) == 1 {
            continue;
        } else {
            return Some(omega);
        }
    }

    None
}

#[derive(Debug)]
pub struct NttBackendU64 {
    q: u64,
    q_twice: u64,
    _n: u64,
    n_inv: u64,
    n_inv_shoup: u64,
    psi_powers_bo: Box<[u64]>,
    psi_inv_powers_bo: Box<[u64]>,
    psi_powers_bo_shoup: Box<[u64]>,
    psi_inv_powers_bo_shoup: Box<[u64]>,
}

impl NttBackendU64 {
    fn _new(q: u64, n: usize) -> Self {
        // \psi = 2n^{th} primitive root of unity in F_q
        let mut rng = ChaCha8Rng::from_seed([0u8; 32]);
        let psi = find_primitive_root(q, (n * 2) as u64, &mut rng)
            .expect("Unable to find 2n^th root of unity");
        let psi_inv = mod_inverse(psi, q);

        // assert!(
        //     ((psi_inv as u128 * psi as u128) % q as u128) == 1,
        //     "psi:{psi}, psi_inv:{psi_inv}"
        // );

        let modulus = ModularOpsU64::new(q);

        let mut psi_powers = Vec::with_capacity(n as usize);
        let mut psi_inv_powers = Vec::with_capacity(n as usize);
        let mut running_psi = 1;
        let mut running_psi_inv = 1;
        for _ in 0..n {
            psi_powers.push(running_psi);
            psi_inv_powers.push(running_psi_inv);

            running_psi = modulus.mul(&running_psi, &psi);
            running_psi_inv = modulus.mul(&running_psi_inv, &psi_inv);
        }

        // powers stored in bit reversed order
        let mut psi_powers_bo = vec![0u64; n as usize];
        let mut psi_inv_powers_bo = vec![0u64; n as usize];
        let shift_by = n.leading_zeros() + 1;
        for i in 0..n as usize {
            // i in bit reversed order
            let bo_index = i.reverse_bits() >> shift_by;

            psi_powers_bo[bo_index] = psi_powers[i];
            psi_inv_powers_bo[bo_index] = psi_inv_powers[i];
        }

        // shoup representation
        let psi_powers_bo_shoup = psi_powers_bo
            .iter()
            .map(|v| ShoupMul::representation(*v, q))
            .collect_vec();
        let psi_inv_powers_bo_shoup = psi_inv_powers_bo
            .iter()
            .map(|v| ShoupMul::representation(*v, q))
            .collect_vec();

        // n^{-1} \mod{q}
        let n_inv = mod_inverse(n as u64, q);

        NttBackendU64 {
            q,
            q_twice: 2 * q,
            _n: n as u64,
            n_inv,
            n_inv_shoup: ShoupMul::representation(n_inv, q),
            psi_powers_bo: psi_powers_bo.into_boxed_slice(),
            psi_inv_powers_bo: psi_inv_powers_bo.into_boxed_slice(),
            psi_powers_bo_shoup: psi_powers_bo_shoup.into_boxed_slice(),
            psi_inv_powers_bo_shoup: psi_inv_powers_bo_shoup.into_boxed_slice(),
        }
    }
}

impl<M: Modulus<Element = u64>> NttInit<M> for NttBackendU64 {
    fn new(q: &M, n: usize) -> Self {
        // This NTT does not support native modulus
        assert!(!q.is_native());
        NttBackendU64::_new(q.q().unwrap(), n)
    }
}

impl Ntt for NttBackendU64 {
    type Element = u64;

    fn forward_lazy(&self, v: &mut [Self::Element]) {
        ntt_lazy(
            v,
            &self.psi_powers_bo,
            &self.psi_powers_bo_shoup,
            self.q,
            self.q_twice,
        )
    }

    fn forward(&self, v: &mut [Self::Element]) {
        ntt(
            v,
            &self.psi_powers_bo,
            &self.psi_powers_bo_shoup,
            self.q,
            self.q_twice,
        );
    }

    fn backward_lazy(&self, v: &mut [Self::Element]) {
        ntt_inv_lazy(
            v,
            &self.psi_inv_powers_bo,
            &self.psi_inv_powers_bo_shoup,
            self.n_inv,
            self.n_inv_shoup,
            self.q,
            self.q_twice,
        )
    }

    fn backward(&self, v: &mut [Self::Element]) {
        ntt_inv(
            v,
            &self.psi_inv_powers_bo,
            &self.psi_inv_powers_bo_shoup,
            self.n_inv,
            self.n_inv_shoup,
            self.q,
            self.q_twice,
        );
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use rand::{thread_rng, Rng};
    use rand_distr::Uniform;

    use super::NttBackendU64;
    use crate::{
        backend::{ModInit, ModularOpsU64, VectorOps},
        ntt::Ntt,
        utils::{generate_prime, negacyclic_mul},
    };

    const Q_60_BITS: u64 = 1152921504606748673;
    const N: usize = 1 << 4;

    const K: usize = 128;

    fn random_vec_in_fq(size: usize, q: u64) -> Vec<u64> {
        thread_rng()
            .sample_iter(Uniform::new(0, q))
            .take(size)
            .collect_vec()
    }

    fn assert_output_range(a: &[u64], max_val: u64) {
        a.iter()
            .for_each(|v| assert!(v <= &max_val, "{v} > {max_val}"));
    }

    #[test]
    fn native_ntt_backend_works() {
        // TODO(Jay): Improve tests. Add tests for different primes and ring size.
        let ntt_backend = NttBackendU64::_new(Q_60_BITS, N);
        for _ in 0..K {
            let mut a = random_vec_in_fq(N, Q_60_BITS);
            let a_clone = a.clone();

            ntt_backend.forward(&mut a);
            assert_output_range(a.as_ref(), Q_60_BITS - 1);
            assert_ne!(a, a_clone);
            ntt_backend.backward(&mut a);
            assert_output_range(a.as_ref(), Q_60_BITS - 1);
            assert_eq!(a, a_clone);

            ntt_backend.forward_lazy(&mut a);
            assert_output_range(a.as_ref(), (2 * Q_60_BITS) - 1);
            assert_ne!(a, a_clone);
            ntt_backend.backward(&mut a);
            assert_output_range(a.as_ref(), Q_60_BITS - 1);
            assert_eq!(a, a_clone);

            ntt_backend.forward(&mut a);
            assert_output_range(a.as_ref(), Q_60_BITS - 1);
            ntt_backend.backward_lazy(&mut a);
            assert_output_range(a.as_ref(), (2 * Q_60_BITS) - 1);
            // reduce
            a.iter_mut().for_each(|a0| {
                if *a0 >= Q_60_BITS {
                    *a0 -= *a0 - Q_60_BITS;
                }
            });
            assert_eq!(a, a_clone);
        }
    }

    #[test]
    fn native_ntt_negacylic_mul() {
        let primes = [25, 40, 50, 60]
            .iter()
            .map(|bits| generate_prime(*bits, (2 * N) as u64, 1u64 << bits).unwrap())
            .collect_vec();

        for p in primes.into_iter() {
            let ntt_backend = NttBackendU64::_new(p, N);
            let modulus_backend = ModularOpsU64::new(p);
            for _ in 0..K {
                let a = random_vec_in_fq(N, p);
                let b = random_vec_in_fq(N, p);

                let mut a_clone = a.clone();
                let mut b_clone = b.clone();
                ntt_backend.forward_lazy(&mut a_clone);
                ntt_backend.forward_lazy(&mut b_clone);
                modulus_backend.elwise_mul_mut(&mut a_clone, &b_clone);
                ntt_backend.backward(&mut a_clone);

                let mul = |a: &u64, b: &u64| {
                    let tmp = *a as u128 * *b as u128;
                    (tmp % p as u128) as u64
                };
                let expected_out = negacyclic_mul(&a, &b, mul, p);

                assert_eq!(a_clone, expected_out);
            }
        }
    }
}
