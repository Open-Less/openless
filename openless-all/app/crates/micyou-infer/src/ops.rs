/*
 * MicYou — Turns your Android device into a high-quality PC microphone.
 * Copyright (C) 2026 LanRhyme <https://github.com/MicYou-Dev/MicYou>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version, with the MicYou Plugin Exception.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 */

//! Numeric kernels for the MCYI VM.
//!
//! Every kernel is a mechanical transcription of the corresponding numpy
//! reference in `onnxref/replay.py` (libmicyou repository, `tools/onnx-port`;
//! itself validated bit-approximately against onnxruntime). All arithmetic is
//! f32; rounding points mirror the reference so streaming goldens stay
//! within ~1e-5.
//!
//! # Safety & performance contract
//!
//! Kernels receive raw pointers (the VM cannot express disjoint arena
//! borrows through `&mut Vec`). Each kernel immediately re-materializes
//! them as `&[f32]`/`&mut [f32]` slices. This is sound **and** essential
//! for speed: the compiler's live-range packing guarantees output regions
//! never overlap input regions of the same op (re-validated in debug
//! builds), and slice references give LLVM the `noalias` facts it needs —
//! the same loops over raw pointers derived from one arena base do **not**
//! auto-vectorize (~2.5× slower in measurements).

/// Element-wise binary operations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

const MAX_NDIM: usize = 6;

#[inline]
fn numel(shape: &[u32]) -> usize {
    shape.iter().fold(1usize, |a, &d| a * d as usize)
}

#[inline]
fn row_major_strides(shape: &[u32]) -> [usize; MAX_NDIM] {
    let mut st = [0usize; MAX_NDIM];
    let n = shape.len();
    if n == 0 {
        return st;
    }
    st[n - 1] = 1;
    for k in (0..n - 1).rev() {
        st[k] = st[k + 1] * shape[k + 1] as usize;
    }
    st
}

/// Map own-shape strides into the output axis space (right-aligned), using
/// stride 0 for broadcast (size-1 or missing) axes.
fn broadcast_strides(ish: &[u32], osh: &[u32]) -> [usize; MAX_NDIM] {
    let own = row_major_strides(ish);
    let mut out = [0usize; MAX_NDIM];
    let n = osh.len();
    debug_assert!(ish.len() <= n);
    let pad = n - ish.len();
    for k in 0..n {
        if k < pad {
            out[k] = 0; // axis absent in input → broadcast
        } else {
            let ik = k - pad;
            out[k] = if ish[ik] == 1 { 0 } else { own[ik] };
        }
    }
    out
}

/// Cephes-style f32 `exp`: degree-6 Taylor on the reduced argument,
/// relative error ≈ 1.2e-7 — indistinguishable from libm `expf` at the
/// tolerances this VM is validated against (2e-4), and several times
/// faster with straight-line vectorizable code (sigmoid/tanh dominate the
/// GRU/gate paths: >100k calls per AEC7 frame).
///
/// Range behavior (matches expf within 1e-37 absolute — far below any
/// tolerance-relevant threshold):
/// * `x > 88.0`  → +inf
/// * `x < -87.0` → 0.0
#[inline]
pub fn fast_exp(x: f32) -> f32 {
    const LN2_HI: f32 = 0.6931457519531250; // trailing zero bits → k*LN2_HI exact
    const LN2_LO: f32 = 1.4286068203094172e-6;
    const LOG2E: f32 = 1.4426950408889634;
    // Fully branchless (clamp + copysign) so callers vectorize: outside the
    // f32-normal range the result saturates to ~1.7e38 / ~2e-38 — within
    // 1e-37 absolute of the true expf value, i.e. far below any tolerance
    // that matters here (sigmoid/tanh saturate identically).
    let xc = x.clamp(-87.3, 88.0);
    let t = xc * LOG2E;
    let k = (t + 0.5f32.copysign(t)) as i32; // round-to-nearest, branchless
    let kf = k as f32;
    let r = (xc - kf * LN2_HI) - kf * LN2_LO;
    let p = 1.0
        + r * (1.0
            + r * (0.5
                + r * (0.16666667 + r * (0.041666668 + r * (0.008333334 + r * 0.0013888889)))));
    let bits = ((127 + k) as u32) << 23;
    p * f32::from_bits(bits)
}

/// `tanh` via `fast_exp(2z)`: absolute error ≈ 6e-8 near zero, ~1.2e-7
/// elsewhere — within golden tolerances, avoids libm call overhead.
#[inline]
pub fn fast_tanh(z: f32) -> f32 {
    // branchless: fast_exp saturates at ~1.7e38, so (e-1)/(e+1) → 1.0 in f32
    let e = fast_exp(2.0 * z);
    (e - 1.0) / (e + 1.0)
}

#[inline(always)]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + fast_exp(-x))
}

/// NumPy-style broadcasting binary op. `osh` is the (statically known)
/// broadcast result shape.
///
/// Per-op dispatch happens ONCE per call (never per element) so every inner
/// loop monomorphizes into branch-free, auto-vectorizable code. The Pow
/// scalar-square case (the only Pow in both models) ignores `y` and becomes
/// a bare x*x — bit-identical to correctly-rounded powf(x, 2).
///
/// # Safety
/// See module docs.
#[inline(always)]
pub(crate) unsafe fn binary_impl(
    op: BinOp,
    ap: *const f32,
    ash: &[u32],
    bp: *const f32,
    bsh: &[u32],
    op_: *mut f32,
    osh: &[u32],
) {
    let a = std::slice::from_raw_parts(ap, numel(ash));
    let b = std::slice::from_raw_parts(bp, numel(bsh));
    let o = std::slice::from_raw_parts_mut(op_, numel(osh));
    match op {
        BinOp::Add => binary_core(a, ash, b, bsh, o, osh, |x, y| x + y),
        BinOp::Sub => binary_core(a, ash, b, bsh, o, osh, |x, y| x - y),
        BinOp::Mul => binary_core(a, ash, b, bsh, o, osh, |x, y| x * y),
        BinOp::Div => binary_core(a, ash, b, bsh, o, osh, |x, y| x / y),
        BinOp::Pow => {
            if b.len() == 1 && b[0] == 2.0 {
                binary_core(a, ash, b, bsh, o, osh, |x, _y| x * x)
            } else {
                binary_core(a, ash, b, bsh, o, osh, |x, y| x.powf(y))
            }
        }
    }
}

#[inline(always)]
fn binary_core<F: Fn(f32, f32) -> f32>(
    a: &[f32],
    ash: &[u32],
    b: &[f32],
    bsh: &[u32],
    o: &mut [f32],
    osh: &[u32],
    f: F,
) {
    debug_assert!(osh.len() <= MAX_NDIM);
    let on = o.len();
    let n = osh.len();

    // fast path: identical shapes → flat zip
    if a.len() == on && b.len() == on && ash == osh && bsh == osh {
        for i in 0..on {
            o[i] = f(a[i], b[i]);
        }
        return;
    }
    // scalar fast paths
    if b.len() == 1 && a.len() == on && ash == osh {
        let bv = b[0];
        for i in 0..on {
            o[i] = f(a[i], bv);
        }
        return;
    }
    if a.len() == 1 && b.len() == on && bsh == osh {
        let av = a[0];
        for i in 0..on {
            o[i] = f(av, b[i]);
        }
        return;
    }

    if n == 0 {
        o[0] = f(a[0], b[0]);
        return;
    }

    let astr = broadcast_strides(ash, osh);
    let bstr = broadcast_strides(bsh, osh);
    let inner = osh[n - 1] as usize;
    let outer = if inner == 0 { 0 } else { on / inner };
    let alast = astr[n - 1];
    let blast = bstr[n - 1];
    debug_assert!(inner == 0 || alast <= 1);
    debug_assert!(inner == 0 || blast <= 1);

    let mut coords = [0usize; MAX_NDIM];
    for oi in 0..outer {
        let mut aoff = 0usize;
        let mut boff = 0usize;
        for k in 0..n - 1 {
            aoff += coords[k] * astr[k];
            boff += coords[k] * bstr[k];
        }
        let dst = &mut o[oi * inner..(oi + 1) * inner];
        match (alast, blast) {
            (1, 1) => {
                let sa = &a[aoff..aoff + inner];
                let sb = &b[boff..boff + inner];
                for j in 0..inner {
                    dst[j] = f(sa[j], sb[j]);
                }
            }
            (1, _) => {
                let sa = &a[aoff..aoff + inner];
                let bv = b[boff];
                for j in 0..inner {
                    dst[j] = f(sa[j], bv);
                }
            }
            (_, 1) => {
                let av = a[aoff];
                let sb = &b[boff..boff + inner];
                for j in 0..inner {
                    dst[j] = f(av, sb[j]);
                }
            }
            _ => {
                let av = a[aoff];
                let bv = b[boff];
                for j in 0..inner {
                    dst[j] = f(av, bv);
                }
            }
        }
        // odometer over outer axes 0..n-2
        if n >= 2 {
            let mut k = n - 2;
            loop {
                coords[k] += 1;
                if coords[k] < osh[k] as usize {
                    break;
                }
                coords[k] = 0;
                if k == 0 {
                    break;
                }
                k -= 1;
            }
        }
    }
}

/// SIGMOID (6) / SQRT (7) / LOG (8).
///
/// # Safety
/// See module docs.
#[inline(always)]
pub(crate) unsafe fn unary_impl(opcode: u16, xp: *const f32, op_: *mut f32, n: usize) {
    let x = std::slice::from_raw_parts(xp, n);
    let o = std::slice::from_raw_parts_mut(op_, n);
    match opcode {
        6 => {
            for i in 0..n {
                o[i] = sigmoid(x[i]);
            }
        }
        7 => {
            for i in 0..n {
                o[i] = x[i].sqrt();
            }
        }
        _ => {
            for i in 0..n {
                o[i] = x[i].ln();
            }
        }
    }
}

/// # Safety
/// See module docs.
pub unsafe fn clip(xp: *const f32, op_: *mut f32, n: usize, lo: Option<f32>, hi: Option<f32>) {
    let x = std::slice::from_raw_parts(xp, n);
    let o = std::slice::from_raw_parts_mut(op_, n);
    match (lo, hi) {
        (Some(l), Some(h)) => {
            for i in 0..n {
                o[i] = if x[i] < l {
                    l
                } else if x[i] > h {
                    h
                } else {
                    x[i]
                };
            }
        }
        (Some(l), None) => {
            for i in 0..n {
                o[i] = if x[i] < l { l } else { x[i] };
            }
        }
        (None, Some(h)) => {
            for i in 0..n {
                o[i] = if x[i] > h { h } else { x[i] };
            }
        }
        (None, None) => o.copy_from_slice(x),
    }
}

/// Batched MatMul: `a [..., M, K] × b [..., K, N] → o [..., M, N]` with
/// numpy batch broadcasting (b is 2-D in both shipped models).
///
/// # Safety
/// See module docs.
#[inline(always)]
pub(crate) unsafe fn matmul_impl(
    ap: *const f32,
    ash: &[u32],
    bp: *const f32,
    bsh: &[u32],
    op_: *mut f32,
    osh: &[u32],
) {
    debug_assert!(ash.len() >= 2 && bsh.len() >= 2 && osh.len() >= 2);
    debug_assert!(bsh.len() == 2 || bsh.len() == osh.len());
    let a = std::slice::from_raw_parts(ap, numel(ash));
    let b = std::slice::from_raw_parts(bp, numel(bsh));
    let o = std::slice::from_raw_parts_mut(op_, numel(osh));

    let nb = osh.len() - 2;
    let m = osh[nb] as usize;
    let n = osh[nb + 1] as usize;
    let k = ash[ash.len() - 1] as usize;
    debug_assert_eq!(bsh[bsh.len() - 2] as usize, k);

    // Batch-axis strides come from the FULL tensor layout (a batch step
    // skips the whole trailing [M,K] / [K,N] block), right-aligned with
    // stride-0 broadcasting for size-1 batch dims.
    fn batch_strides(ish: &[u32], nb: usize) -> [usize; MAX_NDIM] {
        let full = row_major_strides(ish);
        let mut out = [0usize; MAX_NDIM];
        let inb = ish.len().saturating_sub(2);
        let pad = nb.saturating_sub(inb);
        for kk in 0..nb {
            if kk >= pad {
                let ik = kk - pad;
                out[kk] = if ish[ik] == 1 { 0 } else { full[ik] };
            }
        }
        out
    }
    let astr = batch_strides(ash, nb);
    let bstr = batch_strides(bsh, nb);

    let n_batches = numel(&osh[..nb]).max(1);
    let mut coords = [0usize; MAX_NDIM];
    for batch in 0..n_batches {
        let mut aoff = 0usize;
        let mut boff = 0usize;
        for d in 0..nb {
            aoff += coords[d] * astr[d];
            boff += coords[d] * bstr[d];
        }
        let abase = &a[aoff..]; // [M, K]
        let bbase = &b[boff..]; // [K, N]
        let obase = &mut o[batch * m * n..(batch + 1) * m * n];
        for i in 0..m {
            let orow = &mut obase[i * n..(i + 1) * n];
            orow.fill(0.0);
            let arow = &abase[i * k..(i + 1) * k];
            for kk in 0..k {
                let av = arow[kk];
                let brow = &bbase[kk * n..(kk + 1) * n];
                for j in 0..n {
                    orow[j] += av * brow[j];
                }
            }
        }
        if nb > 0 {
            let mut d = nb - 1;
            loop {
                coords[d] += 1;
                if coords[d] < osh[d] as usize {
                    break;
                }
                coords[d] = 0;
                if d == 0 {
                    break;
                }
                d -= 1;
            }
        }
    }
}

/// Conv2d / ConvTranspose2d attribute block (decoded by the VM from the
/// op's i64 attribute table). `pb`/`pr`/`opt_*` are carried for
/// completeness — output sizes come from the compiled output slot shapes.
#[allow(dead_code)]
pub struct ConvAttrs {
    pub kh: usize,
    pub kw: usize,
    pub sh: usize,
    pub sw: usize,
    pub dh: usize,
    pub dw: usize,
    pub pt: i32,
    pub pl: i32,
    pub pb: i32,
    pub pr: i32,
    pub group: usize,
    pub opt_h: usize,
    pub opt_w: usize,
}

/// NCHW Conv2d (`transpose == false`, weight `[M, C/g, kh, kw]`) or
/// ConvTranspose2d (`transpose == true`, weight `[C, M/g, kh, kw]`).
/// `scratch` is used for im2col staging (general path only).
///
/// # Safety
/// See module docs.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) unsafe fn conv_impl(
    transpose: bool,
    xp: *const f32,
    xsh: &[u32],
    wp: *const f32,
    wsh: &[u32],
    bias: Option<*const f32>,
    op_: *mut f32,
    osh: &[u32],
    at: &ConvAttrs,
    scratch: &mut Vec<f32>,
) {
    debug_assert_eq!(xsh.len(), 4);
    let x = std::slice::from_raw_parts(xp, numel(xsh));
    let w = std::slice::from_raw_parts(wp, numel(wsh));
    let o = std::slice::from_raw_parts_mut(op_, numel(osh));
    let bias = bias.map(|bp| {
        let m = if transpose {
            (wsh[1] as usize) * at.group
        } else {
            wsh[0] as usize
        };
        std::slice::from_raw_parts(bp, m)
    });

    let nbatch = xsh[0] as usize;
    let c_in = xsh[1] as usize;
    let h_in = xsh[2] as usize;
    let w_in = xsh[3] as usize;
    let (kh, kw, sh, sw, dh, dw, group) = (at.kh, at.kw, at.sh, at.sw, at.dh, at.dw, at.group);
    let (pt, pl) = (at.pt as i64, at.pl as i64);
    let h_out = osh[2] as usize;
    let w_out = osh[3] as usize;
    let cpg = c_in / group;
    let hw_in = h_in * w_in;
    let hw_out = h_out * w_out;

    if !transpose {
        let m_out = wsh[0] as usize;
        let mpg = m_out / group;

        if kh == 1 && kw == 1 && sh == 1 && sw == 1 && dh == 1 && dw == 1 {
            // pointwise == GEMM per group: [mpg, cpg] × [cpg, HW]
            for nb in 0..nbatch {
                for gi in 0..group {
                    let xg = &x[nb * c_in * hw_in + gi * cpg * hw_in..];
                    let wg = &w[gi * mpg * cpg..];
                    let og = &mut o[nb * m_out * hw_out + gi * mpg * hw_out..];
                    for mi in 0..mpg {
                        let orow = &mut og[mi * hw_out..(mi + 1) * hw_out];
                        let bv = bias.map_or(0.0, |b| b[gi * mpg + mi]);
                        orow.fill(bv);
                        for ci in 0..cpg {
                            let wv = wg[mi * cpg + ci];
                            let xrow = &xg[ci * hw_in..(ci + 1) * hw_in];
                            for p in 0..hw_out {
                                orow[p] += wv * xrow[p];
                            }
                        }
                    }
                }
            }
            return;
        }

        if group == c_in && m_out == c_in {
            // depthwise: w [C, 1, kh, kw] — axpy passes so the output row is
            // contiguous in the inner loop.
            for nb in 0..nbatch {
                for c in 0..c_in {
                    let xp_ = &x[(nb * c_in + c) * hw_in..];
                    let wp_ = &w[c * kh * kw..];
                    let op2 = &mut o[(nb * c_in + c) * hw_out..];
                    let bv = bias.map_or(0.0, |b| b[c]);
                    op2[..hw_out].fill(bv);
                    for oh in 0..h_out {
                        let orow = &mut op2[oh * w_out..(oh + 1) * w_out];
                        for i in 0..kh {
                            let ih = (oh * sh + i * dh) as i64 - pt;
                            if ih < 0 || ih >= h_in as i64 {
                                continue;
                            }
                            let xrow = &xp_[ih as usize * w_in..];
                            for j in 0..kw {
                                let wv = wp_[i * kw + j];
                                let base = j as i64 * dw as i64 - pl;
                                // valid ow: 0 ≤ ow*sw + base < w_in
                                let lo = if base >= 0 {
                                    0
                                } else {
                                    ((-base + sw as i64 - 1) / sw as i64) as usize
                                };
                                let max_ow = (w_in as i64 - 1 - base) / sw as i64;
                                let hi_excl = (max_ow + 1).clamp(0, w_out as i64) as usize;
                                for ow in lo..hi_excl {
                                    let iw = (ow as i64 * sw as i64 + base) as usize;
                                    orow[ow] += wv * xrow[iw];
                                }
                            }
                        }
                    }
                }
            }
            return;
        }

        // general grouped conv: im2col + GEMM
        let col_k = cpg * kh * kw;
        scratch.clear();
        scratch.resize(col_k * hw_out, 0.0);
        for nb in 0..nbatch {
            for gi in 0..group {
                let xg = &x[nb * c_in * hw_in + gi * cpg * hw_in..];
                {
                    let cols: &mut [f32] = scratch;
                    for ci in 0..cpg {
                        for i in 0..kh {
                            for j in 0..kw {
                                let col_row = ci * kh * kw + i * kw + j;
                                for oh in 0..h_out {
                                    let ih = (oh * sh + i * dh) as i64 - pt;
                                    let row_ok = ih >= 0 && ih < h_in as i64;
                                    let crow = &mut cols[col_row * hw_out + oh * w_out..][..w_out];
                                    for ow in 0..w_out {
                                        let iw = (ow * sw + j * dw) as i64 - pl;
                                        crow[ow] = if row_ok && iw >= 0 && iw < w_in as i64 {
                                            xg[ci * hw_in + ih as usize * w_in + iw as usize]
                                        } else {
                                            0.0
                                        };
                                    }
                                }
                            }
                        }
                    }
                }
                let wg = &w[gi * mpg * col_k..];
                let og = &mut o[nb * m_out * hw_out + gi * mpg * hw_out..];
                let cols: &[f32] = scratch;
                for mi in 0..mpg {
                    let orow = &mut og[mi * hw_out..(mi + 1) * hw_out];
                    let bv = bias.map_or(0.0, |b| b[gi * mpg + mi]);
                    orow.fill(bv);
                    for kk in 0..col_k {
                        let wv = wg[mi * col_k + kk];
                        let crow = &cols[kk * hw_out..(kk + 1) * hw_out];
                        for p in 0..hw_out {
                            orow[p] += wv * crow[p];
                        }
                    }
                }
            }
        }
        return;
    }

    // ── ConvTranspose: scatter-add ──
    let mpg = wsh[1] as usize;
    let m_out = mpg * group;
    debug_assert_eq!(osh[1] as usize, m_out);
    o.fill(0.0);
    for nb in 0..nbatch {
        for gi in 0..group {
            for ci in 0..cpg {
                let c = gi * cpg + ci;
                let xpc = &x[nb * c_in * hw_in + c * hw_in..];
                let wpc = &w[c * mpg * kh * kw..];
                for a in 0..h_in {
                    let oh_base = a as i64 * sh as i64 - pt;
                    for bidx in 0..w_in {
                        let xv = xpc[a * w_in + bidx];
                        let ow_base = bidx as i64 * sw as i64 - pl;
                        for i in 0..kh {
                            let oh = oh_base + i as i64 * dh as i64;
                            if oh < 0 || oh >= h_out as i64 {
                                continue;
                            }
                            for j in 0..kw {
                                let ow = ow_base + j as i64 * dw as i64;
                                if ow < 0 || ow >= w_out as i64 {
                                    continue;
                                }
                                let pos = (oh as usize) * w_out + ow as usize;
                                for mi in 0..mpg {
                                    o[nb * m_out * hw_out + (gi * mpg + mi) * hw_out + pos] +=
                                        wpc[mi * kh * kw + i * kw + j] * xv;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(b) = bias {
        for nb in 0..nbatch {
            for m in 0..m_out {
                let bv = b[m];
                let p0 = nb * m_out * hw_out + m * hw_out;
                for p in 0..hw_out {
                    o[p0 + p] += bv;
                }
            }
        }
    }
}

/// ONNX GRU (gate order z,r,h; optional biases; forward/backward/
/// bidirectional; `linear_before_reset` 0/1).
///
/// X `[seq, batch, in]`, W `[dir, 3h, in]`, R `[dir, 3h, h]`,
/// B `[dir, 6h]` (W-biases ‖ R-biases), H0 `[dir, batch, h]`.
/// Y `[seq, dir, batch, h]` (optional), Y_h `[dir, batch, h]` (optional).
///
/// # Safety
/// See module docs.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) unsafe fn gru_impl(
    xp: *const f32,
    xsh: &[u32],
    wp: *const f32,
    rp: *const f32,
    b: Option<*const f32>,
    h0: Option<*const f32>,
    y: Option<*mut f32>,
    yh: Option<*mut f32>,
    hidden: usize,
    direction: i64,
    lbr: bool,
    pre: Option<(&[f32], &[f32])>,
    scratch: &mut Vec<f32>,
) {
    debug_assert_eq!(xsh.len(), 3);
    let seq = xsh[0] as usize;
    let batch = xsh[1] as usize;
    let in_dim = xsh[2] as usize;
    let h = hidden;
    let h3 = 3 * h;
    let num_dir = if direction == 2 { 2usize } else { 1usize };

    let x = std::slice::from_raw_parts(xp, seq * batch * in_dim);
    let w = std::slice::from_raw_parts(wp, num_dir * h3 * in_dim);
    let r = std::slice::from_raw_parts(rp, num_dir * h3 * h);
    let b = b.map(|bp| std::slice::from_raw_parts(bp, num_dir * 6 * h));
    let h0 = h0.map(|hp| std::slice::from_raw_parts(hp, num_dir * batch * h));
    let mut y = y.map(|yp| std::slice::from_raw_parts_mut(yp, seq * num_dir * batch * h));
    let mut yh = yh.map(|yp| std::slice::from_raw_parts_mut(yp, num_dir * batch * h));

    // scratch layout (split_at_mut → mutually noalias slices → vectorizes):
    //   wt [in*h3] | rt [h*h3] | xproj [seq*batch*h3] | state [batch*h]
    //   | rproj [batch*h3] | bz [h3] | zt [batch*h] | rtg [batch*h]
    let wt_n = in_dim * h3;
    let rt_n = h * h3;
    let xp_n = seq * batch * h3;
    let total = wt_n + rt_n + xp_n + batch * h + batch * h3 + h3 + 2 * batch * h;
    if scratch.len() < total {
        scratch.resize(total, 0.0);
    }
    let (wt_s, rest) = scratch.split_at_mut(wt_n);
    let (rt_s, rest) = rest.split_at_mut(rt_n);
    let (xproj_s, rest) = rest.split_at_mut(xp_n);
    let (state_s, rest) = rest.split_at_mut(batch * h);
    let (rproj_s, rest) = rest.split_at_mut(batch * h3);
    let (bz_s, rest) = rest.split_at_mut(h3);
    let (zt_s, rtg_s) = rest.split_at_mut(batch * h);

    for w_idx in 0..num_dir {
        let reverse = direction == 1 || (direction == 2 && w_idx == 1);

        // Projections need W/R transposed to [in][3h] / [h][3h] for
        // contiguous inner loops. For constant weights (the shipped models)
        // the Session pre-transposes once; otherwise fall back to a
        // per-call transpose into scratch.
        let (wt_use, rt_use): (&[f32], &[f32]) = match pre {
            Some((pw, pr)) => (
                &pw[w_idx * wt_n..(w_idx + 1) * wt_n],
                &pr[w_idx * rt_n..(w_idx + 1) * rt_n],
            ),
            None => {
                let wd = &w[w_idx * h3 * in_dim..(w_idx + 1) * h3 * in_dim];
                let rd = &r[w_idx * h3 * h..(w_idx + 1) * h3 * h];
                for j in 0..h3 {
                    for kk in 0..in_dim {
                        wt_s[kk * h3 + j] = wd[j * in_dim + kk];
                    }
                    for kk in 0..h {
                        rt_s[kk * h3 + j] = rd[j * h + kk];
                    }
                }
                (&*wt_s, &*rt_s)
            }
        };
        // xproj[t] = X[t] @ Wdᵀ : [batch, in] × [in, h3]
        // Loop order kk-outer keeps each transposed weight row L1-hot while
        // it feeds every batch row (batch-major would re-stream the whole
        // weight matrix per row). Per-element accumulation order over kk is
        // unchanged → bit-identical results.
        for t in 0..seq {
            let xb = &x[t * batch * in_dim..(t + 1) * batch * in_dim];
            let pb = &mut xproj_s[t * batch * h3..(t + 1) * batch * h3];
            pb.fill(0.0);
            for kk in 0..in_dim {
                let wrow = &wt_use[kk * h3..(kk + 1) * h3];
                for i in 0..batch {
                    let xv = xb[i * in_dim + kk];
                    let prow = &mut pb[i * h3..(i + 1) * h3];
                    for j in 0..h3 {
                        prow[j] += xv * wrow[j];
                    }
                }
            }
        }
        // combined gate biases bz[j] = Wb[j] + Rb[j]
        bz_s[..h3].fill(0.0);
        if let Some(bb) = b {
            let bd = &bb[w_idx * 6 * h..(w_idx + 1) * 6 * h];
            for j in 0..h3 {
                bz_s[j] = bd[j] + bd[h3 + j];
            }
        }
        // initial state
        state_s[..batch * h].fill(0.0);
        if let Some(hp) = h0 {
            state_s[..batch * h].copy_from_slice(&hp[w_idx * batch * h..(w_idx + 1) * batch * h]);
        }

        for si in 0..seq {
            let t = if reverse { seq - 1 - si } else { si };
            let pb = &xproj_s[t * batch * h3..(t + 1) * batch * h3];
            // rproj = state @ Rᵀ : [batch, h] × [h, h3] — kk-outer for
            // weight reuse (same accumulation order → bit-identical)
            rproj_s.fill(0.0);
            for kk in 0..h {
                let wrow = &rt_use[kk * h3..(kk + 1) * h3];
                for i in 0..batch {
                    let sv = state_s[i * h + kk];
                    let orow = &mut rproj_s[i * h3..(i + 1) * h3];
                    for j in 0..h3 {
                        orow[j] += sv * wrow[j];
                    }
                }
            }
            // gates (elementwise passes — vectorize; rounding order matches
            // replay.py exactly)
            for i in 0..batch {
                let prow = &pb[i * h3..(i + 1) * h3];
                let rprow = &rproj_s[i * h3..(i + 1) * h3];
                let zt = &mut zt_s[i * h..(i + 1) * h];
                let rtg = &mut rtg_s[i * h..(i + 1) * h];
                let st = &mut state_s[i * h..(i + 1) * h];
                let (wb_h, rb_h): (&[f32], Option<&[f32]>) = match b {
                    Some(bb) => (
                        &bb[w_idx * 6 * h + 2 * h..w_idx * 6 * h + 3 * h],
                        Some(&bb[w_idx * 6 * h + 5 * h..w_idx * 6 * h + 6 * h]),
                    ),
                    None => (&[], None),
                };
                for j in 0..h {
                    zt[j] = sigmoid((prow[j] + rprow[j]) + bz_s[j]);
                }
                for j in 0..h {
                    rtg[j] = sigmoid((prow[h + j] + rprow[h + j]) + bz_s[h + j]);
                }
                if lbr {
                    match rb_h {
                        Some(rbh) => {
                            for j in 0..h {
                                let ht = fast_tanh(
                                    (prow[2 * h + j] + wb_h[j])
                                        + rtg[j] * (rprow[2 * h + j] + rbh[j]),
                                );
                                st[j] = (1.0 - zt[j]) * ht + zt[j] * st[j];
                            }
                        }
                        None => {
                            for j in 0..h {
                                let ht = fast_tanh(prow[2 * h + j] + rtg[j] * rprow[2 * h + j]);
                                st[j] = (1.0 - zt[j]) * ht + zt[j] * st[j];
                            }
                        }
                    }
                } else {
                    match rb_h {
                        Some(rbh) => {
                            for j in 0..h {
                                let ht = fast_tanh(
                                    (prow[2 * h + j] + rtg[j] * rprow[2 * h + j])
                                        + (wb_h[j] + rbh[j]),
                                );
                                st[j] = (1.0 - zt[j]) * ht + zt[j] * st[j];
                            }
                        }
                        None => {
                            for j in 0..h {
                                let ht = fast_tanh(prow[2 * h + j] + rtg[j] * rprow[2 * h + j]);
                                st[j] = (1.0 - zt[j]) * ht + zt[j] * st[j];
                            }
                        }
                    }
                }
            }
            if let Some(ys) = y.as_deref_mut() {
                // Y layout [seq, dir, batch, h]
                let dst = &mut ys[(t * num_dir + w_idx) * batch * h..][..batch * h];
                dst.copy_from_slice(&state_s[..batch * h]);
            }
        }
        if let Some(yhs) = yh.as_deref_mut() {
            let dst = &mut yhs[w_idx * batch * h..(w_idx + 1) * batch * h];
            dst.copy_from_slice(&state_s[..batch * h]);
        }
    }
}

/// BatchNormalization (inference), NCHW, channels on axis 1.
///
/// # Safety
/// See module docs.
#[allow(clippy::too_many_arguments)]
pub unsafe fn batch_norm(
    xp: *const f32,
    xsh: &[u32],
    scale: *const f32,
    bias: *const f32,
    mean: *const f32,
    var: *const f32,
    op_: *mut f32,
    eps: f32,
) {
    let c = xsh[1] as usize;
    let spatial = numel(xsh) / c;
    let x = std::slice::from_raw_parts(xp, numel(xsh));
    let o = std::slice::from_raw_parts_mut(op_, numel(xsh));
    let scale = std::slice::from_raw_parts(scale, c);
    let bias = std::slice::from_raw_parts(bias, c);
    let mean = std::slice::from_raw_parts(mean, c);
    let var = std::slice::from_raw_parts(var, c);
    for ch in 0..c {
        let inv = 1.0 / (var[ch] + eps).sqrt();
        let sc = scale[ch];
        let bi = bias[ch];
        let mu = mean[ch];
        let xs = &x[ch * spatial..(ch + 1) * spatial];
        let os = &mut o[ch * spatial..(ch + 1) * spatial];
        for p in 0..spatial {
            os[p] = ((xs[p] - mu) * inv) * sc + bi;
        }
    }
}

/// LayerNormalization (opset-17 semantics): normalize over dims `axis..`;
/// scale/bias have the shape of the normalized tail.
///
/// # Safety
/// See module docs.
#[inline(always)]
pub(crate) unsafe fn layer_norm_impl(
    xp: *const f32,
    xsh: &[u32],
    scale: *const f32,
    bias: Option<*const f32>,
    op_: *mut f32,
    axis: usize,
    eps: f32,
) {
    let block: usize = xsh[axis..].iter().fold(1usize, |a, &d| a * d as usize);
    let total = numel(xsh);
    let leading = total / block;
    let x = std::slice::from_raw_parts(xp, total);
    let o = std::slice::from_raw_parts_mut(op_, total);
    let scale = std::slice::from_raw_parts(scale, block);
    let bias = bias.map(|bp| std::slice::from_raw_parts(bp, block));
    for l in 0..leading {
        let xs = &x[l * block..(l + 1) * block];
        let os = &mut o[l * block..(l + 1) * block];
        let mut sum = 0.0f32;
        for i in 0..block {
            sum += xs[i];
        }
        let mean = sum / block as f32;
        let mut vs = 0.0f32;
        for i in 0..block {
            let c = xs[i] - mean;
            vs += c * c;
        }
        let inv = 1.0 / (vs / block as f32 + eps).sqrt();
        match bias {
            Some(bp) => {
                for i in 0..block {
                    let c = xs[i] - mean;
                    os[i] = ((c * inv) * scale[i]) + bp[i];
                }
            }
            None => {
                for i in 0..block {
                    let c = xs[i] - mean;
                    os[i] = (c * inv) * scale[i];
                }
            }
        }
    }
}

/// General N-D transpose.
///
/// # Safety
/// See module docs.
pub unsafe fn transpose(xp: *const f32, xsh: &[u32], perm: &[i64], op_: *mut f32) {
    let n = xsh.len();
    debug_assert!(n <= MAX_NDIM && n == perm.len());
    let total = numel(xsh);
    let x = std::slice::from_raw_parts(xp, total);
    let o = std::slice::from_raw_parts_mut(op_, total);
    let mut osh = [0u32; MAX_NDIM];
    for k in 0..n {
        osh[k] = xsh[perm[k] as usize];
    }
    let xstr = row_major_strides(xsh);
    // source-index delta when the output odometer advances on axis k
    let mut dsrc = [0usize; MAX_NDIM];
    for k in 0..n {
        dsrc[k] = xstr[perm[k] as usize];
    }
    let mut src = 0usize;
    let mut coords = [0usize; MAX_NDIM];
    for oi in 0..total {
        o[oi] = x[src];
        if oi + 1 == total {
            break;
        }
        let mut k = n - 1;
        loop {
            coords[k] += 1;
            src += dsrc[k];
            if coords[k] < osh[k] as usize {
                break;
            }
            coords[k] = 0;
            src -= dsrc[k] * osh[k] as usize;
            if k == 0 {
                break;
            }
            k -= 1;
        }
    }
}

/// Concat along `axis`.
///
/// # Safety
/// See module docs.
pub unsafe fn concat(ins: &[(*const f32, &[u32])], axis: usize, op_: *mut f32, osh: &[u32]) {
    let inner: usize = osh[axis + 1..].iter().fold(1usize, |a, &d| a * d as usize);
    let outer: usize = osh[..axis].iter().fold(1usize, |a, &d| a * d as usize);
    let out_axis = osh[axis] as usize;
    let o = std::slice::from_raw_parts_mut(op_, numel(osh));
    for oi in 0..outer {
        let mut cum = 0usize;
        for &(ptr, sh) in ins {
            let rows = sh[axis] as usize;
            let src = std::slice::from_raw_parts(ptr, numel(sh));
            let from = oi * rows * inner;
            let to = oi * out_axis * inner + cum * inner;
            o[to..to + rows * inner].copy_from_slice(&src[from..from + rows * inner]);
            cum += rows;
        }
        debug_assert_eq!(cum, out_axis);
    }
}

/// Slice with compile-time-normalized ranges.
/// i64 attrs: `[n, (axis, start, end, step) × n]`; for negative steps,
/// `end == -1` means "through index 0" (ONNX semantics) and the compiler
/// encodes an empty selection as `(start=0, end=0)`.
///
/// # Safety
/// See module docs.
pub unsafe fn slice(xp: *const f32, xsh: &[u32], attrs: &[i64], op_: *mut f32, osh: &[u32]) {
    let n = xsh.len();
    debug_assert!(n <= MAX_NDIM && n >= 1);
    let total_in = numel(xsh);
    let total = numel(osh);
    let x = std::slice::from_raw_parts(xp, total_in);
    if total == 0 {
        return;
    }
    let o = std::slice::from_raw_parts_mut(op_, total);
    let cnt = attrs[0] as usize;
    let mut start = [0i64; MAX_NDIM];
    let mut step = [1i64; MAX_NDIM];
    let mut counts = [1usize; MAX_NDIM];
    for d in 0..n {
        counts[d] = xsh[d] as usize;
    }
    for k in 0..cnt {
        let axis = attrs[1 + 4 * k] as usize;
        let s = attrs[2 + 4 * k];
        let e = attrs[3 + 4 * k];
        let st = attrs[4 + 4 * k];
        start[axis] = s;
        step[axis] = st;
        let c = if st > 0 {
            ((e - s) + st - 1) / st
        } else if s == 0 && e == 0 {
            0 // empty-selection sentinel from the compiler
        } else {
            ((s - e) + (-st) - 1) / (-st)
        };
        counts[axis] = c.max(0) as usize;
    }
    debug_assert_eq!(counts[..n].iter().fold(1usize, |a, &d| a * d), total);
    let xstr = row_major_strides(xsh);

    // fast path: contiguous run along the last axis
    if n == 1 || step[n - 1] == 1 {
        let inner_run = counts[n - 1];
        let outer_n = total / inner_run.max(1);
        let mut coords = [0usize; MAX_NDIM];
        let mut dst = 0usize;
        for oi in 0..outer_n {
            if oi > 0 && n >= 2 {
                let mut k = n - 2;
                loop {
                    coords[k] += 1;
                    if coords[k] < counts[k] {
                        break;
                    }
                    coords[k] = 0;
                    if k == 0 {
                        break;
                    }
                    k -= 1;
                }
            }
            let mut src = start[n - 1] as usize * xstr[n - 1];
            for k in 0..n - 1 {
                src += (start[k] + coords[k] as i64 * step[k]) as usize * xstr[k];
            }
            o[dst..dst + inner_run].copy_from_slice(&x[src..src + inner_run]);
            dst += inner_run;
        }
        return;
    }

    // general path: per-element odometer
    let mut coords = [0usize; MAX_NDIM];
    for oi in 0..total {
        if oi > 0 {
            let mut k = n - 1;
            loop {
                coords[k] += 1;
                if coords[k] < counts[k] {
                    break;
                }
                coords[k] = 0;
                if k == 0 {
                    break;
                }
                k -= 1;
            }
        }
        let mut src = 0usize;
        for k in 0..n {
            src += (start[k] + coords[k] as i64 * step[k]) as usize * xstr[k];
        }
        o[oi] = x[src];
    }
}

/// Constant padding. i64 attrs: `[begin_0..begin_{n-1}, end_0..end_{n-1}]`.
///
/// # Safety
/// See module docs.
pub unsafe fn pad(xp: *const f32, xsh: &[u32], pads: &[i64], cv: f32, op_: *mut f32, osh: &[u32]) {
    let n = xsh.len();
    debug_assert!(n <= MAX_NDIM && pads.len() == 2 * n);
    let o = std::slice::from_raw_parts_mut(op_, numel(osh));
    o.fill(cv);
    let in_total = numel(xsh);
    if in_total == 0 {
        return;
    }
    let x = std::slice::from_raw_parts(xp, in_total);
    let ostr = row_major_strides(osh);
    let inner = xsh[n - 1] as usize; // contiguous in both src and dst
    let outer = in_total / inner;
    let mut coords = [0usize; MAX_NDIM];
    for oi in 0..outer {
        if oi > 0 && n >= 2 {
            let mut k = n - 2;
            loop {
                coords[k] += 1;
                if coords[k] < xsh[k] as usize {
                    break;
                }
                coords[k] = 0;
                if k == 0 {
                    break;
                }
                k -= 1;
            }
        }
        let mut dst = pads[n - 1] as usize * ostr[n - 1];
        for k in 0..n - 1 {
            dst += (coords[k] + pads[k] as usize) * ostr[k];
        }
        o[dst..dst + inner].copy_from_slice(&x[oi * inner..(oi + 1) * inner]);
    }
}

/// Broadcast copy (Expand): every input dim is 1 or equal to the out dim.
///
/// # Safety
/// See module docs.
pub unsafe fn expand(xp: *const f32, xsh: &[u32], op_: *mut f32, osh: &[u32]) {
    let n = osh.len();
    let on = numel(osh);
    let o = std::slice::from_raw_parts_mut(op_, on);
    let a = std::slice::from_raw_parts(xp, numel(xsh));
    if n == 0 {
        o[0] = a[0];
        return;
    }
    if a.len() == on {
        o.copy_from_slice(a);
        return;
    }
    let astr = broadcast_strides(xsh, osh);
    let inner = osh[n - 1] as usize;
    let outer = if inner == 0 { 0 } else { on / inner };
    let alast = astr[n - 1];
    let mut coords = [0usize; MAX_NDIM];
    for oi in 0..outer {
        let mut aoff = 0usize;
        for k in 0..n - 1 {
            aoff += coords[k] * astr[k];
        }
        let dst = &mut o[oi * inner..(oi + 1) * inner];
        if alast == 1 {
            dst.copy_from_slice(&a[aoff..aoff + inner]);
        } else {
            dst.fill(a[aoff]);
        }
        if n >= 2 {
            let mut k = n - 2;
            loop {
                coords[k] += 1;
                if coords[k] < osh[k] as usize {
                    break;
                }
                coords[k] = 0;
                if k == 0 {
                    break;
                }
                k -= 1;
            }
        }
    }
}

/// Gather with constant indices (`idx` = attribute values after the axis).
///
/// # Safety
/// See module docs.
pub unsafe fn gather(
    xp: *const f32,
    xsh: &[u32],
    axis: i64,
    idx: &[i64],
    op_: *mut f32,
    osh: &[u32],
) {
    let n = xsh.len();
    let axis = if axis < 0 {
        (axis + n as i64) as usize
    } else {
        axis as usize
    };
    let inner: usize = xsh[axis + 1..].iter().fold(1usize, |a, &d| a * d as usize);
    let outer: usize = xsh[..axis].iter().fold(1usize, |a, &d| a * d as usize);
    let dim = xsh[axis] as i64;
    debug_assert_eq!(numel(osh), outer * idx.len() * inner);
    let x = std::slice::from_raw_parts(xp, numel(xsh));
    let o = std::slice::from_raw_parts_mut(op_, numel(osh));
    for oi in 0..outer {
        for (t, &ix) in idx.iter().enumerate() {
            let ixd = if ix < 0 { ix + dim } else { ix } as usize;
            let from = (oi * dim as usize + ixd) * inner;
            let to = (oi * idx.len() + t) * inner;
            o[to..to + inner].copy_from_slice(&x[from..from + inner]);
        }
    }
}

/// ReduceMean / ReduceL2 over the given axes (l2 = sqrt of summed squares).
///
/// # Safety
/// See module docs.
#[inline(always)]
pub(crate) unsafe fn reduce_impl(
    l2: bool,
    xp: *const f32,
    xsh: &[u32],
    axes: &[i64],
    _keepdims: bool,
    op_: *mut f32,
    osh: &[u32],
) {
    let n = xsh.len();
    debug_assert!(n <= MAX_NDIM);
    let total_in = numel(xsh);
    let total_out = numel(osh);
    let x = std::slice::from_raw_parts(xp, total_in);
    let o = std::slice::from_raw_parts_mut(op_, total_out);
    o.fill(0.0);
    let mut reduced = [false; MAX_NDIM];
    for &a in axes {
        let a = if a < 0 {
            (a + n as i64) as usize
        } else {
            a as usize
        };
        reduced[a] = true;
    }

    // Vectorized special cases (cover every reduction in both models):
    if axes.len() == 1 {
        let ax = {
            let a = axes[0];
            if a < 0 {
                (a + n as i64) as usize
            } else {
                a as usize
            }
        };
        if ax == n - 1 && n >= 1 {
            // contiguous trailing axis → row sums
            let run = xsh[ax] as usize;
            let rows = total_out;
            for row in 0..rows {
                let xs = &x[row * run..(row + 1) * run];
                if l2 {
                    let mut acc = 0.0f32;
                    for i in 0..run {
                        acc += xs[i] * xs[i];
                    }
                    o[row] = acc.sqrt();
                } else {
                    let mut acc = 0.0f32;
                    for i in 0..run {
                        acc += xs[i];
                    }
                    o[row] = acc / run as f32;
                }
            }
            return;
        }
        if ax == 1 && n == 4 {
            // NCHW channel axis → axpy passes over the contiguous HW plane
            let c = xsh[1] as usize;
            let hw = (xsh[2] as usize) * (xsh[3] as usize);
            let nb = xsh[0] as usize;
            for nn in 0..nb {
                for ch in 0..c {
                    let xs = &x[(nn * c + ch) * hw..(nn * c + ch + 1) * hw];
                    let os = &mut o[nn * hw..(nn + 1) * hw];
                    if l2 {
                        for p in 0..hw {
                            os[p] += xs[p] * xs[p];
                        }
                    } else {
                        for p in 0..hw {
                            os[p] += xs[p];
                        }
                    }
                }
            }
            if l2 {
                for i in 0..total_out {
                    o[i] = o[i].sqrt();
                }
            } else {
                let inv = 1.0f32 / c as f32;
                for i in 0..total_out {
                    o[i] *= inv;
                }
            }
            return;
        }
    }
    // output flat index as a linear form over input coordinates
    let mut ostr = [0usize; MAX_NDIM];
    {
        let mut acc = 1usize;
        for k in (0..n).rev() {
            if !reduced[k] {
                ostr[k] = acc;
                acc *= xsh[k] as usize;
            }
        }
        debug_assert_eq!(acc, total_out);
    }
    let mut count = 1usize;
    for k in 0..n {
        if reduced[k] {
            count *= xsh[k] as usize;
        }
    }
    let mut dst = 0usize;
    let mut coords = [0usize; MAX_NDIM];
    for xi in 0..total_in {
        if xi > 0 {
            // advance the input odometer, maintaining dst incrementally
            let mut k = n - 1;
            loop {
                coords[k] += 1;
                dst += ostr[k];
                if coords[k] < xsh[k] as usize {
                    break;
                }
                coords[k] = 0;
                dst -= ostr[k] * xsh[k] as usize;
                if k == 0 {
                    break;
                }
                k -= 1;
            }
        }
        let v = x[xi];
        o[dst] += if l2 { v * v } else { v };
    }
    if l2 {
        for i in 0..total_out {
            o[i] = o[i].sqrt();
        }
    } else {
        let inv = 1.0f32 / count as f32;
        for i in 0..total_out {
            o[i] *= inv;
        }
    }
}

/// Resize: linear mode, half_pixel transform, target sizes from `osh`.
/// Separable per-dimension passes ping-pong between the scratch buffers;
/// the final pass writes to `o`.
///
/// # Safety
/// See module docs.
pub unsafe fn resize(
    xp: *const f32,
    xsh: &[u32],
    op_: *mut f32,
    osh: &[u32],
    scratch: &mut Vec<f32>,
    scratch2: &mut Vec<f32>,
) {
    let n = xsh.len();
    debug_assert_eq!(n, osh.len());
    debug_assert!(n <= MAX_NDIM);
    let out_total = numel(osh);
    let x = std::slice::from_raw_parts(xp, numel(xsh));
    let mut max_total = 1usize;
    let mut dims: [usize; MAX_NDIM] = [0; MAX_NDIM];
    let mut m = 0usize;
    for d in 0..n {
        let mx = xsh[d].max(osh[d]) as usize;
        max_total *= mx;
        if xsh[d] != osh[d] {
            dims[m] = d;
            m += 1;
        }
    }
    if m == 0 {
        let o = std::slice::from_raw_parts_mut(op_, out_total);
        o.copy_from_slice(x);
        return;
    }
    scratch.resize(max_total, 0.0);
    scratch2.resize(max_total, 0.0);
    let s1 = scratch.as_mut_ptr();
    let s2 = scratch2.as_mut_ptr();

    // pass p: src = x (p=0) / s1 (odd) / s2 (even, >0); dst = o (last) /
    // s1 (even) / s2 (odd) — src and dst are always distinct buffers.
    let mut cur_shape = [0u32; MAX_NDIM];
    cur_shape[..n].copy_from_slice(xsh);
    for pass in 0..m {
        let d = dims[pass];
        let last = pass + 1 == m;
        let in_d = cur_shape[d] as usize;
        let out_d = osh[d] as usize;
        let ratio = in_d as f32 / out_d as f32;
        let outer: usize = cur_shape[..d].iter().fold(1usize, |a, &v| a * v as usize);
        let inner: usize = cur_shape[d + 1..n]
            .iter()
            .fold(1usize, |a, &v| a * v as usize);
        let cur_total = outer * in_d * inner;
        let out_pass_total = outer * out_d * inner;

        // per-dim coordinate tables (tiny; recomputed per pass)
        let mut lohi: Vec<(i64, i64)> = Vec::with_capacity(out_d);
        let mut frac_tab: Vec<f32> = Vec::with_capacity(out_d);
        for p in 0..out_d {
            let coord = (p as f32 + 0.5) * ratio - 0.5;
            let lo = coord.floor() as i64;
            let lo_c = lo.clamp(0, in_d as i64 - 1);
            let hi_c = (lo + 1).clamp(0, in_d as i64 - 1);
            lohi.push((lo_c, hi_c));
            frac_tab.push(coord - lo as f32);
        }

        let src_ptr: *const f32 = if pass == 0 {
            x.as_ptr()
        } else if pass % 2 == 1 {
            s1 as *const f32
        } else {
            s2 as *const f32
        };
        let dst_ptr: *mut f32 = if last {
            op_
        } else if pass % 2 == 0 {
            s1
        } else {
            s2
        };
        let src = std::slice::from_raw_parts(src_ptr, cur_total);
        let dst = std::slice::from_raw_parts_mut(dst_ptr, out_pass_total);
        resize_pass(src, dst, outer, in_d, out_d, inner, &lohi, &frac_tab);
        cur_shape[d] = osh[d];
    }
}

#[allow(clippy::too_many_arguments)]
fn resize_pass(
    src: &[f32],
    dst: &mut [f32],
    outer: usize,
    in_d: usize,
    out_d: usize,
    inner: usize,
    lohi: &[(i64, i64)],
    frac_tab: &[f32],
) {
    for oo in 0..outer {
        for p in 0..out_d {
            let (lo, hi) = lohi[p];
            let frac = frac_tab[p];
            let slow = &src[(oo * in_d + lo as usize) * inner..][..inner];
            let shigh = &src[(oo * in_d + hi as usize) * inner..][..inner];
            let drow = &mut dst[(oo * out_d + p) * inner..][..inner];
            for i in 0..inner {
                let lv = slow[i];
                drow[i] = lv + (shigh[i] - lv) * frac;
            }
        }
    }
}

// ─── AVX2+FMA runtime dispatch ─────────────────────────────────────────────
//
// The workspace builds for baseline x86-64 (SSE2) so the shipped binaries
// run everywhere; ONNX Runtime's MLAS gets its speed from runtime CPUID
// dispatch to AVX kernels. We mirror that: each hot kernel body is compiled
// twice from the SAME source (an `#[inline(always)]` impl inlined into a
// `#[target_feature(enable = "avx2,fma")]` wrapper and into the baseline
// entry point), selected once per process via `is_x86_feature_detected`.
// Non-x86 targets (e.g. macOS arm64) use the baseline path, where LLVM
// already auto-vectorizes with NEON.

#[cfg(target_arch = "x86_64")]
fn have_avx2() -> bool {
    use std::sync::OnceLock;
    static DETECTED: OnceLock<bool> = OnceLock::new();
    *DETECTED.get_or_init(|| {
        std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma")
    })
}

pub unsafe fn binary(
    op: BinOp,
    ap: *const f32,
    ash: &[u32],
    bp: *const f32,
    bsh: &[u32],
    op_: *mut f32,
    osh: &[u32],
) {
    #[cfg(target_arch = "x86_64")]
    {
        if have_avx2() {
            return binary_avx2(op, ap, ash, bp, bsh, op_, osh);
        }
    }
    binary_impl(op, ap, ash, bp, bsh, op_, osh)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn binary_avx2(
    op: BinOp,
    ap: *const f32,
    ash: &[u32],
    bp: *const f32,
    bsh: &[u32],
    op_: *mut f32,
    osh: &[u32],
) {
    binary_impl(op, ap, ash, bp, bsh, op_, osh)
}

pub unsafe fn unary(opcode: u16, xp: *const f32, op_: *mut f32, n: usize) {
    #[cfg(target_arch = "x86_64")]
    {
        if have_avx2() {
            return unary_avx2(opcode, xp, op_, n);
        }
    }
    unary_impl(opcode, xp, op_, n)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn unary_avx2(opcode: u16, xp: *const f32, op_: *mut f32, n: usize) {
    unary_impl(opcode, xp, op_, n)
}

pub unsafe fn matmul(
    ap: *const f32,
    ash: &[u32],
    bp: *const f32,
    bsh: &[u32],
    op_: *mut f32,
    osh: &[u32],
) {
    #[cfg(target_arch = "x86_64")]
    {
        if have_avx2() {
            return matmul_avx2(ap, ash, bp, bsh, op_, osh);
        }
    }
    matmul_impl(ap, ash, bp, bsh, op_, osh)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn matmul_avx2(
    ap: *const f32,
    ash: &[u32],
    bp: *const f32,
    bsh: &[u32],
    op_: *mut f32,
    osh: &[u32],
) {
    matmul_impl(ap, ash, bp, bsh, op_, osh)
}

pub unsafe fn conv(
    transpose: bool,
    xp: *const f32,
    xsh: &[u32],
    wp: *const f32,
    wsh: &[u32],
    bias: Option<*const f32>,
    op_: *mut f32,
    osh: &[u32],
    at: &ConvAttrs,
    scratch: &mut Vec<f32>,
) {
    #[cfg(target_arch = "x86_64")]
    {
        if have_avx2() {
            return conv_avx2(transpose, xp, xsh, wp, wsh, bias, op_, osh, at, scratch);
        }
    }
    conv_impl(transpose, xp, xsh, wp, wsh, bias, op_, osh, at, scratch)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn conv_avx2(
    transpose: bool,
    xp: *const f32,
    xsh: &[u32],
    wp: *const f32,
    wsh: &[u32],
    bias: Option<*const f32>,
    op_: *mut f32,
    osh: &[u32],
    at: &ConvAttrs,
    scratch: &mut Vec<f32>,
) {
    conv_impl(transpose, xp, xsh, wp, wsh, bias, op_, osh, at, scratch)
}

pub unsafe fn gru(
    xp: *const f32,
    xsh: &[u32],
    wp: *const f32,
    rp: *const f32,
    b: Option<*const f32>,
    h0: Option<*const f32>,
    y: Option<*mut f32>,
    yh: Option<*mut f32>,
    hidden: usize,
    direction: i64,
    lbr: bool,
    pre: Option<(&[f32], &[f32])>,
    scratch: &mut Vec<f32>,
) {
    #[cfg(target_arch = "x86_64")]
    {
        if have_avx2() {
            return gru_avx2(
                xp, xsh, wp, rp, b, h0, y, yh, hidden, direction, lbr, pre, scratch,
            );
        }
    }
    gru_impl(
        xp, xsh, wp, rp, b, h0, y, yh, hidden, direction, lbr, pre, scratch,
    )
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn gru_avx2(
    xp: *const f32,
    xsh: &[u32],
    wp: *const f32,
    rp: *const f32,
    b: Option<*const f32>,
    h0: Option<*const f32>,
    y: Option<*mut f32>,
    yh: Option<*mut f32>,
    hidden: usize,
    direction: i64,
    lbr: bool,
    pre: Option<(&[f32], &[f32])>,
    scratch: &mut Vec<f32>,
) {
    gru_impl(
        xp, xsh, wp, rp, b, h0, y, yh, hidden, direction, lbr, pre, scratch,
    )
}

pub unsafe fn reduce(
    l2: bool,
    xp: *const f32,
    xsh: &[u32],
    axes: &[i64],
    _keepdims: bool,
    op_: *mut f32,
    osh: &[u32],
) {
    #[cfg(target_arch = "x86_64")]
    {
        if have_avx2() {
            return reduce_avx2(l2, xp, xsh, axes, _keepdims, op_, osh);
        }
    }
    reduce_impl(l2, xp, xsh, axes, _keepdims, op_, osh)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn reduce_avx2(
    l2: bool,
    xp: *const f32,
    xsh: &[u32],
    axes: &[i64],
    _keepdims: bool,
    op_: *mut f32,
    osh: &[u32],
) {
    reduce_impl(l2, xp, xsh, axes, _keepdims, op_, osh)
}

pub unsafe fn layer_norm(
    xp: *const f32,
    xsh: &[u32],
    scale: *const f32,
    bias: Option<*const f32>,
    op_: *mut f32,
    axis: usize,
    eps: f32,
) {
    #[cfg(target_arch = "x86_64")]
    {
        if have_avx2() {
            return layer_norm_avx2(xp, xsh, scale, bias, op_, axis, eps);
        }
    }
    layer_norm_impl(xp, xsh, scale, bias, op_, axis, eps)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn layer_norm_avx2(
    xp: *const f32,
    xsh: &[u32],
    scale: *const f32,
    bias: Option<*const f32>,
    op_: *mut f32,
    axis: usize,
    eps: f32,
) {
    layer_norm_impl(xp, xsh, scale, bias, op_, axis, eps)
}
