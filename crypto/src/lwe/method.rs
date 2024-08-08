use crate::{
    distribution::NoiseDistribution,
    lwe::structure::{
        LweCiphertextMutView, LweCiphertextView, LweKeySwitchKeyMutView, LweKeySwitchKeyView,
        LwePlaintext, LweSecretKeyView,
    },
};
use phantom_zone_math::{
    decomposer::Decomposer,
    izip_eq,
    ring::{ElemFrom, RingOps},
};
use rand::RngCore;

pub fn sk_encrypt<'a, 'b, R, T>(
    ring: &R,
    ct: impl Into<LweCiphertextMutView<'a, R::Elem>>,
    sk: impl Into<LweSecretKeyView<'b, T>>,
    pt: LwePlaintext<R::Elem>,
    noise_distribution: NoiseDistribution,
    mut rng: impl RngCore,
) where
    R: RingOps + ElemFrom<T>,
    T: 'b + Copy,
{
    let mut ct = ct.into();
    ring.sample_uniform_into(ct.a_mut(), &mut rng);
    let a_sk = ring.slice_dot_elem_from(ct.a(), sk.into().as_ref());
    let e = ring.sample::<i64>(&noise_distribution, &mut rng);
    *ct.b_mut() = ring.add(&ring.add(&a_sk, &e), &pt.0);
}

pub fn decrypt<'a, 'b, R, T>(
    ring: &R,
    sk: impl Into<LweSecretKeyView<'a, T>>,
    ct: impl Into<LweCiphertextView<'b, R::Elem>>,
) -> LwePlaintext<R::Elem>
where
    R: RingOps + ElemFrom<T>,
    T: 'a + Copy,
{
    let ct = ct.into();
    let a_sk = ring.slice_dot_elem_from(ct.a(), sk.into().as_ref());
    LwePlaintext(ring.sub(ct.b(), &a_sk))
}

pub fn ks_key_gen<'a, 'b, 'c, R, T>(
    ring: &R,
    ks_key: impl Into<LweKeySwitchKeyMutView<'a, R::Elem>>,
    sk_from: impl Into<LweSecretKeyView<'b, T>>,
    sk_to: impl Into<LweSecretKeyView<'c, T>>,
    noise_distribution: NoiseDistribution,
    mut rng: impl RngCore,
) where
    R: RingOps + ElemFrom<T>,
    T: 'b + 'c + Copy,
{
    let (mut ks_key, sk_to) = (ks_key.into(), sk_to.into());
    let decomposer = R::Decomposer::new(ring.modulus(), ks_key.decomposition_param());
    izip_eq!(ks_key.cts_iter_mut(), sk_from.into().as_ref()).for_each(
        |(mut ks_key_i, sk_from_i)| {
            izip_eq!(ks_key_i.iter_mut(), decomposer.gadget_iter()).for_each(
                |(ks_key_i_j, beta_j)| {
                    let pt = LwePlaintext(ring.mul_elem_from(&ring.neg(&beta_j), sk_from_i));
                    sk_encrypt(ring, ks_key_i_j, sk_to, pt, noise_distribution, &mut rng)
                },
            )
        },
    );
}

pub fn key_switch<'a, 'b, 'c, R: RingOps>(
    ring: &R,
    ct_to: impl Into<LweCiphertextMutView<'a, R::Elem>>,
    ks_key: impl Into<LweKeySwitchKeyView<'b, R::Elem>>,
    ct_from: impl Into<LweCiphertextView<'c, R::Elem>>,
) {
    let (mut ct_to, ks_key, ct_from) = (ct_to.into(), ks_key.into(), ct_from.into());
    let decomposer = R::Decomposer::new(ring.modulus(), ks_key.decomposition_param());
    izip_eq!(ks_key.cts_iter(), ct_from.a())
        .enumerate()
        .for_each(|(i, (ks_key_i, a_i))| {
            izip_eq!(ks_key_i.iter(), decomposer.decompose_iter(a_i))
                .enumerate()
                .for_each(|(j, (ks_key_i_j, a_i_j))| {
                    let slice_scalar_fma = if i == 0 && j == 0 {
                        R::slice_scalar_mul
                    } else {
                        R::slice_scalar_fma
                    };
                    slice_scalar_fma(ring, ct_to.as_mut(), ks_key_i_j.as_ref(), &a_i_j)
                })
        });
    *ct_to.b_mut() = ring.add(ct_to.b(), ct_from.b());
}
