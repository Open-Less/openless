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

//! `micyou-infer` — a tiny, dependency-free inference VM for **statically
//! compiled** MCYI model blobs.
//!
//! This crate replaces `ort`/ONNX Runtime for the PureVox6 noise suppressor
//! and the AEC7 echo canceller. The models are compiled ahead of time by the
//! `tools/onnx-port` compiler in the experimental
//! [libmicyou](https://github.com/OrientCOMPASS/libmicyou) repository (numpy
//! reference interpreter → constant-folded static instruction stream + flat
//! f32 weight blob), following the same methodology as the silero-v4
//! pure-Rust port. Compilation folds every shape-structural node away, so at
//! runtime the VM only executes numeric kernels over a pre-packed f32 arena —
//! no graph parsing, no dynamic shapes, no allocations per frame.
//!
//! # Semantics contract
//!
//! The executable specification of the blob format and every kernel is
//! `onnxref/replay.py` (numpy) in the libmicyou repository. The Rust kernels
//! are a mechanical transcription, verified three ways there:
//!
//! 1. `parity_ort.py` — numpy interpreter ↔ onnxruntime, streaming frames,
//!    bit-approximate (≤ ~2e-5 f32 noise);
//! 2. `replay_check.py` — compiled blob ↔ interpreter (numpy replay);
//! 3. golden fixtures (`gen_golden.py`) — per-op kernels and full end-to-end
//!    streaming sequences.
//!
//! This repository ships only the runtime artifacts (VM + compiled blobs);
//! `docs/pure-rust-inference.md` records the verification matrix and results.
//!
//! # Safety model
//!
//! Kernels operate on raw `*const f32`/`*mut f32` into a single arena.
//! Soundness rests on one invariant, enforced by the compiler's live-range
//! packing and re-checked in debug builds ([`Graph::validate_layout`]):
//! at every op, the output region never overlaps any input region.

use std::fmt;

mod ops;

// ─── blob format constants (keep in sync with onnxref/compiler.py) ───────

/// Blob magic: `MCYI` (MiCYou Inference).
pub const MAGIC: [u8; 4] = *b"MCYI";
/// Blob format version produced by the onnx-port compiler and expected here.
pub const VERSION: u32 = 1;

/// Sentinel input position meaning "absent optional input".
pub const NULL_SLOT: u32 = u32::MAX;

// Slot kinds
const SLOT_INPUT: u8 = 0;
const SLOT_CONST: u8 = 1;
const SLOT_ARENA: u8 = 2;
const SLOT_ALIAS: u8 = 3;

// Opcodes (mirror onnxref/compiler.py OPS)
const OP_ADD: u16 = 1;
const OP_SUB: u16 = 2;
const OP_MUL: u16 = 3;
const OP_DIV: u16 = 4;
const OP_POW: u16 = 5;
const OP_SIGMOID: u16 = 6;
const OP_SQRT: u16 = 7;
const OP_LOG: u16 = 8;
const OP_CLIP: u16 = 9;
const OP_MATMUL: u16 = 10;
const OP_CONV: u16 = 11;
const OP_CONV_T: u16 = 12;
const OP_GRU: u16 = 13;
const OP_BATCH_NORM: u16 = 14;
const OP_LAYER_NORM: u16 = 15;
const OP_TRANSPOSE: u16 = 16;
const OP_CONCAT: u16 = 17;
const OP_SLICE: u16 = 18;
const OP_PAD: u16 = 19;
const OP_EXPAND: u16 = 20;
const OP_GATHER: u16 = 21;
const OP_REDUCE_MEAN: u16 = 22;
const OP_REDUCE_L2: u16 = 23;
const OP_RESIZE: u16 = 24;
const OP_COPY: u16 = 25;

/// Errors surfaced by parsing or execution.
#[derive(Debug)]
pub enum InferError {
    BadMagic,
    BadVersion(u32),
    Truncated,
    BadSlot(u32),
    UnknownOpcode(u16),
    BadOperand(&'static str),
    BadInputCount {
        expected: usize,
        got: usize,
    },
    BadInputLen {
        input: usize,
        expected: usize,
        got: usize,
    },
    LayoutOverlap {
        op: usize,
    },
    BadOutput(usize),
}

impl fmt::Display for InferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => write!(f, "not an MCYI blob (bad magic)"),
            Self::BadVersion(v) => write!(f, "unsupported MCYI version {v}"),
            Self::Truncated => write!(f, "MCYI blob truncated"),
            Self::BadSlot(s) => write!(f, "slot id {s} out of range"),
            Self::UnknownOpcode(c) => write!(f, "unknown opcode {c}"),
            Self::BadOperand(what) => write!(f, "malformed operands for {what}"),
            Self::BadInputCount { expected, got } => {
                write!(f, "model expects {expected} inputs, got {got}")
            }
            Self::BadInputLen {
                input,
                expected,
                got,
            } => {
                write!(f, "input {input} expects {expected} f32 values, got {got}")
            }
            Self::LayoutOverlap { op } => write!(f, "arena layout overlap at op {op}"),
            Self::BadOutput(i) => write!(f, "output {i} unavailable"),
        }
    }
}

impl std::error::Error for InferError {}

type Result<T> = std::result::Result<T, InferError>;

// ─── little-endian reader ────────────────────────────────────────────────

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            return Err(InferError::Truncated);
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i64(&mut self) -> Result<i64> {
        let b = self.take(8)?;
        Ok(i64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    fn f32(&mut self) -> Result<f32> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n)?;
        Ok(())
    }
}

// ─── graph ───────────────────────────────────────────────────────────────

#[derive(Debug)]
struct Slot {
    kind: u8,
    /// Own shape of this slot (aliases carry the reshaped view).
    shape: Box<[u32]>,
    numel: usize,
    /// CONST: element offset into `const_data`. ARENA: element offset into
    /// the session arena. ALIAS: index of the aliased slot. INPUT: index
    /// into the bound-input table (filled at parse).
    off: u32,
    /// Storage-owning slot after alias resolution (== own id unless alias).
    root: u32,
}

#[derive(Debug)]
struct Op {
    code: u16,
    ins: Box<[u32]>,
    outs: Box<[u32]>,
    i64: Box<[i64]>,
    f32: Box<[f32]>,
}

/// A parsed, immutable MCYI graph. Cheap to share between sessions
/// (`Sync`); parsing copies the weight blob into aligned storage once.
pub struct Graph {
    slots: Vec<Slot>,
    ops: Vec<Op>,
    /// Slot ids of the model inputs, in blob order.
    inputs: Vec<u32>,
    /// Slot ids of the model outputs, in blob order.
    outputs: Vec<u32>,
    const_data: Box<[f32]>,
    arena_len: usize,
}

impl Graph {
    /// Parse an MCYI blob. `data` is typically an `include_bytes!` asset.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let magic = r.take(4)?;
        if magic != MAGIC {
            return Err(InferError::BadMagic);
        }
        let version = r.u32()?;
        if version != VERSION {
            return Err(InferError::BadVersion(version));
        }
        let n_slots = r.u32()? as usize;
        let n_ops = r.u32()? as usize;
        let n_in = r.u32()? as usize;
        let n_out = r.u32()? as usize;
        let arena_len = r.u32()? as usize;
        let const_len = r.u32()? as usize;

        let mut inputs = Vec::with_capacity(n_in);
        for _ in 0..n_in {
            inputs.push(r.u32()?);
        }
        let mut outputs = Vec::with_capacity(n_out);
        for _ in 0..n_out {
            outputs.push(r.u32()?);
        }

        // slot table (byte-length prefixed)
        let slot_bytes = r.u32()? as usize;
        let slot_end = r.pos + slot_bytes;
        let mut slots: Vec<Slot> = Vec::with_capacity(n_slots);
        for _ in 0..n_slots {
            let kind = r.u8()?;
            let ndim = r.u8()? as usize;
            r.skip(2)?; // pad u16
            let off = r.i32()? as u32;
            let mut shape = Vec::with_capacity(ndim);
            for _ in 0..ndim {
                let d = r.i32()?;
                if d < 0 {
                    return Err(InferError::Truncated);
                }
                shape.push(d as u32);
            }
            let numel = shape.iter().fold(1usize, |a, &d| a * d as usize);
            slots.push(Slot {
                kind,
                shape: shape.into_boxed_slice(),
                numel,
                off,
                root: u32::MAX,
            });
        }
        if r.pos != slot_end {
            return Err(InferError::Truncated);
        }

        // op table (byte-length prefixed)
        let op_bytes = r.u32()? as usize;
        let op_end = r.pos + op_bytes;
        let mut op_vec: Vec<Op> = Vec::with_capacity(n_ops);
        for _ in 0..n_ops {
            let code = r.u16()?;
            let n_i64 = r.u16()? as usize;
            let n_ins = r.u16()? as usize;
            let n_outs = r.u16()? as usize;
            let n_f32 = r.u16()? as usize;
            let mut ins = Vec::with_capacity(n_ins);
            for _ in 0..n_ins {
                ins.push(r.u32()?);
            }
            let mut outs = Vec::with_capacity(n_outs);
            for _ in 0..n_outs {
                outs.push(r.u32()?);
            }
            let mut i64s = Vec::with_capacity(n_i64);
            for _ in 0..n_i64 {
                i64s.push(r.i64()?);
            }
            let mut f32s = Vec::with_capacity(n_f32);
            for _ in 0..n_f32 {
                f32s.push(r.f32()?);
            }
            op_vec.push(Op {
                code,
                ins: ins.into_boxed_slice(),
                outs: outs.into_boxed_slice(),
                i64: i64s.into_boxed_slice(),
                f32: f32s.into_boxed_slice(),
            });
        }
        if r.pos != op_end {
            return Err(InferError::Truncated);
        }

        // const data (f32 LE)
        let bytes = r.take(const_len * 4)?;
        let mut const_data = Vec::with_capacity(const_len);
        for chunk in bytes.chunks_exact(4) {
            const_data.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }

        let mut g = Graph {
            slots,
            ops: op_vec,
            inputs,
            outputs,
            const_data: const_data.into_boxed_slice(),
            arena_len,
        };
        g.finalize()?;
        Ok(g)
    }

    /// Resolve alias chains, map input slots to bound-input indices and
    /// sanity-check references.
    fn finalize(&mut self) -> Result<()> {
        let n = self.slots.len() as u32;
        // alias roots (chains are acyclic by construction; guard anyway)
        for sid in 0..self.slots.len() {
            let mut root = sid as u32;
            let mut depth = 0u32;
            while self.slots[root as usize].kind == SLOT_ALIAS {
                root = self.slots[root as usize].off;
                if root >= n || depth > n {
                    return Err(InferError::BadSlot(root));
                }
                depth += 1;
            }
            self.slots[sid].root = root;
        }
        // input slots get their bound-table index in `off`
        for (i, &sid) in self.inputs.iter().enumerate() {
            if sid as usize >= self.slots.len() {
                return Err(InferError::BadSlot(sid));
            }
            let s = &mut self.slots[sid as usize];
            if s.kind != SLOT_INPUT {
                return Err(InferError::BadOperand("graph input is not an INPUT slot"));
            }
            s.off = i as u32;
        }
        for &sid in self.outputs.iter() {
            if sid as usize >= self.slots.len() {
                return Err(InferError::BadSlot(sid));
            }
            let root = self.slots[sid as usize].root;
            if self.slots[root as usize].kind == SLOT_INPUT {
                return Err(InferError::BadOperand(
                    "graph output aliases an input (compiler must emit COPY)",
                ));
            }
        }
        for (oi, op) in self.ops.iter().enumerate() {
            for &sid in op.ins.iter().chain(op.outs.iter()) {
                if sid != NULL_SLOT && sid as usize >= self.slots.len() {
                    return Err(InferError::BadSlot(sid));
                }
            }
            for &sid in op.outs.iter() {
                if sid == NULL_SLOT {
                    continue;
                }
                let root = self.slots[sid as usize].root as usize;
                if self.slots[root].kind != SLOT_ARENA {
                    return Err(InferError::BadOperand("op output is not an ARENA slot"));
                }
                let end = self.slots[root].off as usize + self.slots[sid as usize].numel;
                if end > self.arena_len {
                    return Err(InferError::LayoutOverlap { op: oi });
                }
            }
        }
        #[cfg(debug_assertions)]
        self.validate_layout()?;
        Ok(())
    }

    /// Debug-build check of the core soundness invariant: per op, output
    /// regions never overlap input regions of the same op.
    #[cfg(debug_assertions)]
    fn validate_layout(&self) -> Result<()> {
        for (oi, op) in self.ops.iter().enumerate() {
            let mut regions: Vec<(usize, usize, bool)> = Vec::new(); // (start, end, is_out)
            for &sid in op.ins.iter() {
                if sid == NULL_SLOT {
                    continue;
                }
                let s = &self.slots[sid as usize];
                if s.kind != SLOT_ARENA {
                    continue;
                }
                let root = &self.slots[s.root as usize];
                regions.push((root.off as usize, root.off as usize + s.numel, false));
            }
            for &sid in op.outs.iter() {
                if sid == NULL_SLOT {
                    continue;
                }
                let s = &self.slots[sid as usize];
                let root = &self.slots[s.root as usize];
                regions.push((root.off as usize, root.off as usize + s.numel, true));
            }
            for i in 0..regions.len() {
                if !regions[i].2 {
                    continue;
                }
                for j in 0..regions.len() {
                    if i == j {
                        continue;
                    }
                    // inputs may overlap each other (read-read is fine)
                    if !regions[j].2 && regions[i].0 < regions[j].1 && regions[j].0 < regions[i].1 {
                        return Err(InferError::LayoutOverlap { op: oi });
                    }
                }
            }
        }
        Ok(())
    }

    /// Number of model inputs.
    pub fn num_inputs(&self) -> usize {
        self.inputs.len()
    }
    /// Number of model outputs.
    pub fn num_outputs(&self) -> usize {
        self.outputs.len()
    }
    /// Shape (f32 element counts per axis) of model input `i`.
    pub fn input_shape(&self, i: usize) -> Option<&[u32]> {
        self.inputs
            .get(i)
            .map(|&sid| &*self.slots[sid as usize].shape)
    }
    /// Shape of model output `i`.
    pub fn output_shape(&self, i: usize) -> Option<&[u32]> {
        self.outputs
            .get(i)
            .map(|&sid| &*self.slots[sid as usize].shape)
    }
    /// Element count expected for model input `i`.
    pub fn input_len(&self, i: usize) -> Option<usize> {
        self.inputs
            .get(i)
            .map(|&sid| self.slots[sid as usize].numel)
    }
    /// Element count of model output `i`.
    pub fn output_len(&self, i: usize) -> Option<usize> {
        self.outputs
            .get(i)
            .map(|&sid| self.slots[sid as usize].numel)
    }
    /// Total f32 elements in the embedded weight/constant blob.
    pub fn const_len(&self) -> usize {
        self.const_data.len()
    }
    /// Total f32 elements in the runtime arena.
    pub fn arena_len(&self) -> usize {
        self.arena_len
    }
    /// Number of emitted ops.
    pub fn num_ops(&self) -> usize {
        self.ops.len()
    }
}

// ─── session ─────────────────────────────────────────────────────────────

struct Bound {
    ptr: *const f32,
}

/// Mutable execution context over a [`Graph`]: owns the f32 arena and the
/// per-run scratch buffer. Not `Clone`; create one per audio channel.
pub struct Session<'g> {
    g: &'g Graph,
    arena: Vec<f32>,
    bound: Vec<Bound>,
    scratch: Vec<f32>,
    scratch2: Vec<f32>,
    /// Per-op pre-transposed GRU weight tables `(Wt, Rt)` for GRU ops whose
    /// W/R inputs are constant (the shipped models). Layout per direction:
    /// `Wt[dir][k][j] = W[dir][j][k]`, `Rt[dir][k][j] = R[dir][j][k]`.
    /// Built once here so the audio thread never re-transposes (~0.6 ms per
    /// AEC7 frame otherwise). Parallel to `g.ops`; `None` for other opcodes.
    gru_pre: Vec<Option<(Box<[f32]>, Box<[f32]>)>>,
}

// SAFETY: `bound` holds caller pointers that are only dereferenced during
// `run()` and are cleared (set to dangling-empty) before `run` returns, so
// a `Session` never carries live references to another thread's memory.
// Everything else (arena, scratch, graph ref) is plain owned/shared data.
unsafe impl Send for Session<'_> {}

impl<'g> Session<'g> {
    pub fn new(g: &'g Graph) -> Self {
        let gru_pre = g
            .ops
            .iter()
            .map(|op| {
                if op.code == OP_GRU {
                    build_gru_pre(g, op)
                } else {
                    None
                }
            })
            .collect();
        Self {
            g,
            arena: vec![0.0f32; g.arena_len],
            bound: Vec::new(),
            scratch: Vec::new(),
            scratch2: Vec::new(),
            gru_pre,
        }
    }

    /// Execute one frame. `inputs` must match [`Graph::num_inputs`] in
    /// order, each with exactly [`Graph::input_len`] elements. Outputs are
    /// read afterwards via [`Session::output`]; they stay valid until the
    /// next `run`.
    pub fn run(&mut self, inputs: &[&[f32]]) -> Result<()> {
        if inputs.len() != self.g.inputs.len() {
            return Err(InferError::BadInputCount {
                expected: self.g.inputs.len(),
                got: inputs.len(),
            });
        }
        for (i, (&sid, inp)) in self.g.inputs.iter().zip(inputs.iter()).enumerate() {
            let expect = self.g.slots[sid as usize].numel;
            if inp.len() != expect {
                return Err(InferError::BadInputLen {
                    input: i,
                    expected: expect,
                    got: inp.len(),
                });
            }
        }
        self.bound.clear();
        for inp in inputs {
            self.bound.push(Bound { ptr: inp.as_ptr() });
        }
        let n_ops = self.g.ops.len();
        for oi in 0..n_ops {
            self.exec(oi)?;
        }
        // Do not let caller pointers outlive the call.
        self.bound.clear();
        Ok(())
    }

    /// Output `i` of the last [`Session::run`]. Valid until the next run.
    pub fn output(&self, i: usize) -> Result<&[f32]> {
        let &sid = self.g.outputs.get(i).ok_or(InferError::BadOutput(i))?;
        let s = &self.g.slots[sid as usize];
        let root = &self.g.slots[s.root as usize];
        match root.kind {
            SLOT_ARENA => Ok(&self.arena[root.off as usize..root.off as usize + s.numel]),
            SLOT_CONST => Ok(&self.g.const_data[root.off as usize..root.off as usize + s.numel]),
            _ => Err(InferError::BadOutput(i)),
        }
    }

    /// Copy output `i` into `dst` (must be exactly `output_len(i)` long).
    pub fn copy_output(&self, i: usize, dst: &mut [f32]) -> Result<()> {
        let src = self.output(i)?;
        if dst.len() != src.len() {
            return Err(InferError::BadOutput(i));
        }
        dst.copy_from_slice(src);
        Ok(())
    }

    // ── internals ──

    /// Resolve a slot to (storage pointer, element count).
    ///
    /// SAFETY: caller must hold the borrow of self for the pointer's
    /// lifetime; INPUT pointers are only valid while `bound` is populated
    /// (i.e. during `run`).
    fn slot_ptr(&self, sid: u32) -> Result<(*const f32, usize)> {
        let s = &self.g.slots[sid as usize];
        let root = &self.g.slots[s.root as usize];
        match root.kind {
            SLOT_INPUT => {
                let b = &self.bound[root.off as usize];
                Ok((b.ptr, s.numel))
            }
            SLOT_CONST => Ok((
                unsafe { self.g.const_data.as_ptr().add(root.off as usize) },
                s.numel,
            )),
            SLOT_ARENA => Ok((
                unsafe { self.arena.as_ptr().add(root.off as usize) },
                s.numel,
            )),
            _ => Err(InferError::BadSlot(sid)),
        }
    }

    fn exec(&mut self, oi: usize) -> Result<()> {
        let op = &self.g.ops[oi];
        let g = self.g;

        // Resolve inputs (≤16) and outputs (≤2) to raw pointers + shapes.
        let mut ip: [*const f32; 16] = [std::ptr::null(); 16];
        let mut ilen: [usize; 16] = [0; 16];
        if op.ins.len() > 16 {
            return Err(InferError::BadOperand("too many inputs"));
        }
        for (k, &sid) in op.ins.iter().enumerate() {
            if sid != NULL_SLOT {
                let (p, l) = self.slot_ptr(sid)?;
                ip[k] = p;
                ilen[k] = l;
            }
        }
        if op.outs.len() > 2 {
            return Err(InferError::BadOperand("too many outputs"));
        }

        let arena = self.arena.as_mut_ptr();
        let scratch = &mut self.scratch;
        let scratch2 = &mut self.scratch2;

        // shape borrows (from the immutable graph)
        let sh = |sid: u32| -> &[u32] { &g.slots[sid as usize].shape };

        match op.code {
            OP_ADD | OP_SUB | OP_MUL | OP_DIV | OP_POW => {
                let bin = match op.code {
                    OP_ADD => ops::BinOp::Add,
                    OP_SUB => ops::BinOp::Sub,
                    OP_MUL => ops::BinOp::Mul,
                    OP_DIV => ops::BinOp::Div,
                    _ => ops::BinOp::Pow,
                };
                require2(ip, ilen, 2)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::binary(bin, ip[0], sh(op.ins[0]), ip[1], sh(op.ins[1]), dst, sh(o));
                }
            }
            OP_SIGMOID | OP_SQRT | OP_LOG => {
                require2(ip, ilen, 1)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::unary(op.code, ip[0], dst, ilen[0]);
                }
            }
            OP_CLIP => {
                require2(ip, ilen, 1)?;
                let has_lo = op.i64[0] != 0;
                let has_hi = op.i64[1] != 0;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::clip(
                        ip[0],
                        dst,
                        ilen[0],
                        has_lo.then_some(op.f32[0]),
                        has_hi.then_some(op.f32[1]),
                    );
                }
            }
            OP_MATMUL => {
                require2(ip, ilen, 2)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::matmul(ip[0], sh(op.ins[0]), ip[1], sh(op.ins[1]), dst, sh(o));
                }
            }
            OP_CONV | OP_CONV_T => {
                let transpose = op.code == OP_CONV_T;
                let a = &op.i64;
                let need = if transpose { 14 } else { 12 };
                if a.len() < need {
                    return Err(InferError::BadOperand("conv attrs"));
                }
                let attrs = ops::ConvAttrs {
                    kh: a[0] as usize,
                    kw: a[1] as usize,
                    sh: a[2] as usize,
                    sw: a[3] as usize,
                    dh: a[4] as usize,
                    dw: a[5] as usize,
                    pt: a[6] as i32,
                    pl: a[7] as i32,
                    pb: a[8] as i32,
                    pr: a[9] as i32,
                    group: a[10] as usize,
                    opt_h: if transpose { a[12] as usize } else { 0 },
                    opt_w: if transpose { a[13] as usize } else { 0 },
                };
                let bias = if a[11] != 0 {
                    require2(ip, ilen, 3)?;
                    Some(ip[2])
                } else {
                    None
                };
                require2(ip, ilen, 2)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::conv(
                        transpose,
                        ip[0],
                        sh(op.ins[0]),
                        ip[1],
                        sh(op.ins[1]),
                        bias,
                        dst,
                        sh(o),
                        &attrs,
                        scratch,
                    );
                }
            }
            OP_GRU => {
                if op.ins.len() != 5 || op.outs.len() != 2 {
                    return Err(InferError::BadOperand("gru inputs/outputs"));
                }
                require2(ip, ilen, 3)?;
                let hidden = op.i64[0] as usize;
                let direction = op.i64[1];
                let lbr = op.i64[2] != 0;
                let b = if ip[3].is_null() { None } else { Some(ip[3]) };
                let h0 = if ip[4].is_null() { None } else { Some(ip[4]) };
                // outputs: [Y, Y_h]; either may be NULL_SLOT
                let y = if op.outs[0] != NULL_SLOT {
                    Some(unsafe { arena.add(g.slots[op.outs[0] as usize].off as usize) })
                } else {
                    None
                };
                let yh = if op.outs.len() > 1 && op.outs[1] != NULL_SLOT {
                    Some(unsafe { arena.add(g.slots[op.outs[1] as usize].off as usize) })
                } else {
                    None
                };
                let pre = self
                    .gru_pre
                    .get(oi)
                    .and_then(|v| v.as_ref())
                    .map(|(pw, pr)| (&pw[..], &pr[..]));
                unsafe {
                    ops::gru(
                        ip[0],
                        sh(op.ins[0]),
                        ip[1],
                        ip[2],
                        b,
                        h0,
                        y,
                        yh,
                        hidden,
                        direction,
                        lbr,
                        pre,
                        scratch,
                    );
                }
            }
            OP_BATCH_NORM => {
                require2(ip, ilen, 5)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::batch_norm(
                        ip[0],
                        sh(op.ins[0]),
                        ip[1],
                        ip[2],
                        ip[3],
                        ip[4],
                        dst,
                        op.f32[0],
                    );
                }
            }
            OP_LAYER_NORM => {
                let bias = if op.ins[2] != NULL_SLOT {
                    require2(ip, ilen, 3)?;
                    Some(ip[2])
                } else {
                    require2(ip, ilen, 2)?;
                    None
                };
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::layer_norm(
                        ip[0],
                        sh(op.ins[0]),
                        ip[1],
                        bias,
                        dst,
                        op.i64[0] as usize,
                        op.f32[0],
                    );
                }
            }
            OP_TRANSPOSE => {
                require2(ip, ilen, 1)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::transpose(ip[0], sh(op.ins[0]), &op.i64, dst);
                }
            }
            OP_CONCAT => {
                let axis = op.i64[0] as usize;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                let mut ins: Vec<(*const f32, &[u32])> = Vec::with_capacity(op.ins.len());
                for (k, &sid) in op.ins.iter().enumerate() {
                    if ip[k].is_null() {
                        return Err(InferError::BadOperand("concat null input"));
                    }
                    ins.push((ip[k], sh(sid)));
                }
                unsafe {
                    ops::concat(&ins, axis, dst, sh(o));
                }
            }
            OP_SLICE => {
                require2(ip, ilen, 1)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::slice(ip[0], sh(op.ins[0]), &op.i64, dst, sh(o));
                }
            }
            OP_PAD => {
                require2(ip, ilen, 1)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::pad(ip[0], sh(op.ins[0]), &op.i64, op.f32[0], dst, sh(o));
                }
            }
            OP_EXPAND => {
                require2(ip, ilen, 1)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::expand(ip[0], sh(op.ins[0]), dst, sh(o));
                }
            }
            OP_GATHER => {
                require2(ip, ilen, 1)?;
                let axis = op.i64[0];
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::gather(ip[0], sh(op.ins[0]), axis, &op.i64[1..], dst, sh(o));
                }
            }
            OP_REDUCE_MEAN | OP_REDUCE_L2 => {
                require2(ip, ilen, 1)?;
                let keepdims = op.i64[0] != 0;
                let n_ax = op.i64[1] as usize;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::reduce(
                        op.code == OP_REDUCE_L2,
                        ip[0],
                        sh(op.ins[0]),
                        &op.i64[2..2 + n_ax],
                        keepdims,
                        dst,
                        sh(o),
                    );
                }
            }
            OP_RESIZE => {
                require2(ip, ilen, 1)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    ops::resize(ip[0], sh(op.ins[0]), dst, sh(o), scratch, scratch2);
                }
            }
            OP_COPY => {
                require2(ip, ilen, 1)?;
                let o = op.outs[0];
                let dst = unsafe { arena.add(g.slots[o as usize].off as usize) };
                unsafe {
                    std::ptr::copy_nonoverlapping(ip[0], dst, ilen[0]);
                }
            }
            other => return Err(InferError::UnknownOpcode(other)),
        }
        Ok(())
    }
}

/// Pre-transpose constant GRU W/R into projection-friendly layouts.
fn build_gru_pre(g: &Graph, op: &Op) -> Option<(Box<[f32]>, Box<[f32]>)> {
    let w_slot = &g.slots[op.ins[1] as usize];
    let r_slot = &g.slots[op.ins[2] as usize];
    let w_root = &g.slots[w_slot.root as usize];
    let r_root = &g.slots[r_slot.root as usize];
    if w_root.kind != SLOT_CONST || r_root.kind != SLOT_CONST {
        return None;
    }
    if w_slot.shape.len() != 3 || r_slot.shape.len() != 3 {
        return None;
    }
    let dir = w_slot.shape[0] as usize;
    let h3 = w_slot.shape[1] as usize;
    let in_dim = w_slot.shape[2] as usize;
    let h = r_slot.shape[2] as usize;
    if h3 != 3 * h || r_slot.shape[0] as usize != dir || r_slot.shape[1] as usize != h3 {
        return None;
    }
    let w = &g.const_data[w_root.off as usize..w_root.off as usize + w_slot.numel];
    let r = &g.const_data[r_root.off as usize..r_root.off as usize + r_slot.numel];
    let mut wt = vec![0.0f32; dir * in_dim * h3];
    let mut rt = vec![0.0f32; dir * h * h3];
    for d in 0..dir {
        for j in 0..h3 {
            for k in 0..in_dim {
                wt[(d * in_dim + k) * h3 + j] = w[d * h3 * in_dim + j * in_dim + k];
            }
            for k in 0..h {
                rt[(d * h + k) * h3 + j] = r[d * h3 * h + j * h + k];
            }
        }
    }
    Some((wt.into_boxed_slice(), rt.into_boxed_slice()))
}

fn require2(ip: [*const f32; 16], _ilen: [usize; 16], n: usize) -> Result<()> {
    for p in ip.iter().take(n) {
        if p.is_null() {
            return Err(InferError::BadOperand("missing required input"));
        }
    }
    Ok(())
}
