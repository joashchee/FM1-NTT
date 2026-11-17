// Vendored from esp32sim (emu-core/src/jit_a64.rs, MIT, (c) 2026 Joakim Eriksson
// and Alice); its encodings are checked against clang there. Added here: ldrsb_u.
//! A tiny AArch64 encoder: only the instructions the block compiler emits. Every encoding is
//! checked against clang's assembler in `tests::encodings_match_clang`.
#![allow(dead_code)]

pub type Reg = u32;
pub const ZR: Reg = 31;
pub const SP: Reg = 31;

/// Condition codes (the `cond` field).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cond { Eq = 0, Ne = 1, Hs = 2, Lo = 3, Mi = 4, Pl = 5, Vs = 6, Vc = 7, Hi = 8, Ls = 9, Ge = 10, Lt = 11, Gt = 12, Le = 13, Al = 14 }
impl Cond {
    pub fn invert(self) -> Cond {
        use Cond::*;
        match self { Eq => Ne, Ne => Eq, Hs => Lo, Lo => Hs, Mi => Pl, Pl => Mi, Vs => Vc, Vc => Vs, Hi => Ls, Ls => Hi, Ge => Lt, Lt => Ge, Gt => Le, Le => Gt, Al => Al }
    }
}

#[derive(Clone, Copy)]
pub struct Label(pub usize);

enum Fix { B26, B19, B14, Adr21 }

pub struct Asm {
    pub code: Vec<u32>,
    labels: Vec<Option<usize>>,
    fixups: Vec<(usize, usize, Fix)>,
    /// 64-bit constants `ldr_lit_x` loads, laid out after the code (`finish`), and the loads
    /// that refer to them: (code index, pool index)
    pool: Vec<u64>,
    pool_refs: Vec<(usize, usize)>,
}

#[allow(clippy::new_without_default, reason = "assembler construction reserves its expected code capacity")]
impl Asm {
    pub fn new() -> Self { Asm { code: Vec::with_capacity(256), labels: Vec::new(), fixups: Vec::new(), pool: Vec::new(), pool_refs: Vec::new() } }
    #[allow(clippy::len_without_is_empty, reason = "the length is exposed as a code offset, not as a collection API")]
    pub fn len(&self) -> usize { self.code.len() }
    pub fn here(&self) -> usize { self.code.len() }
    fn e(&mut self, w: u32) { self.code.push(w); }

    pub fn label(&mut self) -> Label { self.labels.push(None); Label(self.labels.len() - 1) }
    pub fn bind(&mut self, l: Label) { self.labels[l.0] = Some(self.code.len()); }
    pub fn is_bound(&self, l: Label) -> bool { self.labels[l.0].is_some() }

    /// Resolve every branch to its label; every label must be bound. The literal pool follows
    /// the code (8-byte aligned; nothing branches there).
    pub fn finish(mut self) -> Vec<u32> {
        if !self.pool.is_empty() {
            if self.code.len() % 2 == 1 { self.e(0xd503_201f); } // nop
            let base = self.code.len();
            for (at, i) in std::mem::take(&mut self.pool_refs) {
                let off = (base + 2 * i) as i64 - at as i64;
                assert!((-(1 << 18)..(1 << 18)).contains(&off));
                self.code[at] |= ((off as u32) & 0x7ffff) << 5;
            }
            for v in std::mem::take(&mut self.pool) { self.e(v as u32); self.e((v >> 32) as u32); }
        }
        for (at, l, kind) in std::mem::take(&mut self.fixups) {
            let target = self.labels[l].expect("unbound label");
            let off = target as i64 - at as i64;
            match kind {
                Fix::B26 => { assert!((-(1 << 25)..(1 << 25)).contains(&off)); self.code[at] |= (off as u32) & 0x03ff_ffff; }
                Fix::B19 => { assert!((-(1 << 18)..(1 << 18)).contains(&off)); self.code[at] |= ((off as u32) & 0x7ffff) << 5; }
                Fix::B14 => { assert!((-(1 << 13)..(1 << 13)).contains(&off)); self.code[at] |= ((off as u32) & 0x3fff) << 5; }
                Fix::Adr21 => { let b = off * 4; assert!((-(1 << 20)..(1 << 20)).contains(&b)); let b = b as u32; self.code[at] |= (b & 3) << 29 | ((b >> 2) & 0x7ffff) << 5; }
            }
        }
        self.code
    }

    // ---------------------------------------------------------------- moves and immediates
    pub fn movz(&mut self, rd: Reg, imm16: u32, shift: u32) { self.e(0x5280_0000 | (shift / 16) << 21 | (imm16 & 0xffff) << 5 | rd); }
    pub fn movk(&mut self, rd: Reg, imm16: u32, shift: u32) { self.e(0x7280_0000 | (shift / 16) << 21 | (imm16 & 0xffff) << 5 | rd); }
    pub fn movn(&mut self, rd: Reg, imm16: u32, shift: u32) { self.e(0x1280_0000 | (shift / 16) << 21 | (imm16 & 0xffff) << 5 | rd); }
    pub fn movz_x(&mut self, rd: Reg, imm16: u32, shift: u32) { self.e(0xd280_0000 | (shift / 16) << 21 | (imm16 & 0xffff) << 5 | rd); }
    pub fn movk_x(&mut self, rd: Reg, imm16: u32, shift: u32) { self.e(0xf280_0000 | (shift / 16) << 21 | (imm16 & 0xffff) << 5 | rd); }
    /// 32-bit immediate in one or two instructions.
    pub fn mov32(&mut self, rd: Reg, imm: u32) {
        if imm & 0xffff_0000 == 0 { self.movz(rd, imm, 0); }
        else if imm & 0xffff == 0 { self.movz(rd, imm >> 16, 16); }
        else if imm & 0xffff_0000 == 0xffff_0000 { self.movn(rd, !imm & 0xffff, 0); }
        else { self.movz(rd, imm & 0xffff, 0); self.movk(rd, imm >> 16, 16); }
    }
    /// `ldr xd, =imm`: a 64-bit constant from the literal pool (one instruction; `mov64` is four).
    pub fn ldr_lit_x(&mut self, rd: Reg, imm: u64) {
        let i = self.pool.iter().position(|&v| v == imm).unwrap_or_else(|| { self.pool.push(imm); self.pool.len() - 1 });
        self.pool_refs.push((self.code.len(), i));
        self.e(0x5800_0000 | rd);
    }
    /// 64-bit immediate (addresses).
    pub fn mov64(&mut self, rd: Reg, imm: u64) {
        self.movz_x(rd, (imm & 0xffff) as u32, 0);
        for sh in [16u32, 32, 48] { let part = ((imm >> sh) & 0xffff) as u32; if part != 0 { self.movk_x(rd, part, sh); } }
    }
    // single-precision FP (s registers = the low 32 bits of v0..v31)
    pub fn fmov_s_w(&mut self, sd: Reg, wn: Reg) { self.e(0x1e27_0000 | wn << 5 | sd); }
    pub fn fmov_w_s(&mut self, wd: Reg, sn: Reg) { self.e(0x1e26_0000 | sn << 5 | wd); }
    /// fadd 0x1e202800, fsub 0x1e203800, fmul 0x1e200800, fdiv 0x1e201800,
    /// fminnm 0x1e207800, fmaxnm 0x1e206800
    pub fn fop(&mut self, opc: u32, sd: Reg, sn: Reg, sm: Reg) { self.e(opc | sm << 16 | sn << 5 | sd); }
    pub fn fcmp(&mut self, sn: Reg, sm: Reg) { self.e(0x1e20_2000 | sm << 16 | sn << 5); }
    pub fn fcvtzs(&mut self, wd: Reg, sn: Reg) { self.e(0x1e38_0000 | sn << 5 | wd); }
    pub fn fcvtzu(&mut self, wd: Reg, sn: Reg) { self.e(0x1e39_0000 | sn << 5 | wd); }
    pub fn scvtf(&mut self, sd: Reg, wn: Reg) { self.e(0x1e22_0000 | wn << 5 | sd); }
    pub fn ucvtf(&mut self, sd: Reg, wn: Reg) { self.e(0x1e23_0000 | wn << 5 | sd); }
    pub fn mov(&mut self, rd: Reg, rm: Reg) { self.e(0x2a00_03e0 | rm << 16 | rd); }
    pub fn mov_x(&mut self, rd: Reg, rm: Reg) { self.e(0xaa00_03e0 | rm << 16 | rd); }

    // ---------------------------------------------------------------- arithmetic
    pub fn add_imm(&mut self, rd: Reg, rn: Reg, imm: u32) { debug_assert!(imm < 4096); self.e(0x1100_0000 | imm << 10 | rn << 5 | rd); }
    pub fn sub_imm(&mut self, rd: Reg, rn: Reg, imm: u32) { debug_assert!(imm < 4096); self.e(0x5100_0000 | imm << 10 | rn << 5 | rd); }
    pub fn subs_imm(&mut self, rd: Reg, rn: Reg, imm: u32) { debug_assert!(imm < 4096); self.e(0x7100_0000 | imm << 10 | rn << 5 | rd); }
    pub fn add_imm_x(&mut self, rd: Reg, rn: Reg, imm: u32) { debug_assert!(imm < 4096); self.e(0x9100_0000 | imm << 10 | rn << 5 | rd); }
    pub fn cmp_imm(&mut self, rn: Reg, imm: u32) { self.subs_imm(ZR, rn, imm); }
    /// add/sub with a 32-bit immediate of any size (may use a scratch register).
    pub fn add_imm32(&mut self, rd: Reg, rn: Reg, imm: u32, scratch: Reg) {
        if imm < 4096 { self.add_imm(rd, rn, imm); }
        else if imm.wrapping_neg() < 4096 { self.sub_imm(rd, rn, imm.wrapping_neg()); }
        else { self.mov32(scratch, imm); self.add(rd, rn, scratch); }
    }
    pub fn add(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x0b00_0000 | rm << 16 | rn << 5 | rd); }
    pub fn add_x(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x8b00_0000 | rm << 16 | rn << 5 | rd); }
    pub fn add_lsl(&mut self, rd: Reg, rn: Reg, rm: Reg, sh: u32) { self.e(0x0b00_0000 | rm << 16 | sh << 10 | rn << 5 | rd); }
    pub fn add_lsr(&mut self, rd: Reg, rn: Reg, rm: Reg, sh: u32) { self.e(0x0b40_0000 | rm << 16 | sh << 10 | rn << 5 | rd); }
    pub fn add_x_lsl(&mut self, rd: Reg, rn: Reg, rm: Reg, sh: u32) { self.e(0x8b00_0000 | rm << 16 | sh << 10 | rn << 5 | rd); }
    pub fn eor_lsr(&mut self, rd: Reg, rn: Reg, rm: Reg, sh: u32) { self.e(0x4a40_0000 | rm << 16 | sh << 10 | rn << 5 | rd); }
    pub fn sub(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x4b00_0000 | rm << 16 | rn << 5 | rd); }
    pub fn sub_lsl(&mut self, rd: Reg, rn: Reg, rm: Reg, sh: u32) { self.e(0x4b00_0000 | rm << 16 | sh << 10 | rn << 5 | rd); }
    pub fn subs(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x6b00_0000 | rm << 16 | rn << 5 | rd); }
    pub fn cmp(&mut self, rn: Reg, rm: Reg) { self.subs(ZR, rn, rm); }
    pub fn cmp_x(&mut self, rn: Reg, rm: Reg) { self.e(0xeb00_0000 | rm << 16 | rn << 5 | ZR); }
    pub fn neg(&mut self, rd: Reg, rm: Reg) { self.sub(rd, ZR, rm); }
    pub fn mvn(&mut self, rd: Reg, rm: Reg) { self.e(0x2a20_03e0 | rm << 16 | rd); }
    pub fn rev(&mut self, rd: Reg, rn: Reg) { self.e(0x5ac0_0800 | rn << 5 | rd); }
    pub fn rev16(&mut self, rd: Reg, rn: Reg) { self.e(0x5ac0_0400 | rn << 5 | rd); }
    pub fn sdiv(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x1ac0_0c00 | rm << 16 | rn << 5 | rd); }
    pub fn udiv(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x1ac0_0800 | rm << 16 | rn << 5 | rd); }
    /// rd = ra - rn * rm
    pub fn msub(&mut self, rd: Reg, rn: Reg, rm: Reg, ra: Reg) { self.e(0x1b00_8000 | rm << 16 | ra << 10 | rn << 5 | rd); }
    pub fn and(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x0a00_0000 | rm << 16 | rn << 5 | rd); }
    pub fn orr(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x2a00_0000 | rm << 16 | rn << 5 | rd); }
    pub fn orr_lsl(&mut self, rd: Reg, rn: Reg, rm: Reg, sh: u32) { self.e(0x2a00_0000 | rm << 16 | sh << 10 | rn << 5 | rd); }
    pub fn orr_x(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0xaa00_0000 | rm << 16 | rn << 5 | rd); }
    pub fn eor(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x4a00_0000 | rm << 16 | rn << 5 | rd); }
    pub fn bic(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x0a20_0000 | rm << 16 | rn << 5 | rd); }
    pub fn ands(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x6a00_0000 | rm << 16 | rn << 5 | rd); }
    pub fn tst(&mut self, rn: Reg, rm: Reg) { self.ands(ZR, rn, rm); }
    /// `and` / `orr` / `eor` / `ands` of w(rn) with a bitmask immediate (`logical_imm`), None when `v` is not one. `kind`: 0 and, 1 orr, 2 eor, 3 ands.
    pub fn logic_imm(&mut self, kind: u32, rd: Reg, rn: Reg, v: u32) -> bool {
        let Some(bits) = logical_imm(v) else { return false };
        let base = [0x1200_0000u32, 0x3200_0000, 0x5200_0000, 0x7200_0000][kind as usize];
        self.e(base | bits << 10 | rn << 5 | rd);
        true
    }
    /// `and` with a contiguous mask `((1 << ones) - 1) << shift` (32-bit).
    pub fn and_mask(&mut self, rd: Reg, rn: Reg, ones: u32, shift: u32) {
        debug_assert!((1..=31).contains(&ones) && ones + shift <= 32);
        self.e(0x1200_0000 | ((32 - shift) & 31) << 16 | (ones - 1) << 10 | rn << 5 | rd);
    }
    /// `eor` with a contiguous mask `((1 << ones) - 1) << shift` (32-bit).
    pub fn eor_mask(&mut self, rd: Reg, rn: Reg, ones: u32, shift: u32) {
        debug_assert!((1..=31).contains(&ones) && ones + shift <= 32);
        self.e(0x5200_0000 | ((32 - shift) & 31) << 16 | (ones - 1) << 10 | rn << 5 | rd);
    }
    /// Flags for w(rn) - imm12 << (12 if `lsl12`).
    pub fn cmp_imm12(&mut self, rn: Reg, imm: u32, lsl12: bool) { debug_assert!(imm < 4096); self.e(0x7100_0000 | (lsl12 as u32) << 22 | imm << 10 | rn << 5 | ZR); }
    /// Flags for x(rn) - imm12 << (12 if `lsl12`).
    pub fn cmp_imm12_x(&mut self, rn: Reg, imm: u32, lsl12: bool) { debug_assert!(imm < 4096); self.e(0xf100_0000 | (lsl12 as u32) << 22 | imm << 10 | rn << 5 | ZR); }
    pub fn tst_mask(&mut self, rn: Reg, ones: u32, shift: u32) {
        debug_assert!((1..=31).contains(&ones) && ones + shift <= 32);
        self.e(0x7200_0000 | ((32 - shift) & 31) << 16 | (ones - 1) << 10 | rn << 5 | ZR);
    }
    pub fn lsl_imm(&mut self, rd: Reg, rn: Reg, sh: u32) { let sh = sh & 31; self.e(0x5300_0000 | ((32 - sh) & 31) << 16 | (31 - sh) << 10 | rn << 5 | rd); }
    pub fn lsr_imm(&mut self, rd: Reg, rn: Reg, sh: u32) { let sh = sh & 31; self.e(0x5300_0000 | sh << 16 | 31 << 10 | rn << 5 | rd); }
    pub fn asr_imm(&mut self, rd: Reg, rn: Reg, sh: u32) { let sh = sh & 31; self.e(0x1300_0000 | sh << 16 | 31 << 10 | rn << 5 | rd); }
    pub fn lsr_imm_x(&mut self, rd: Reg, rn: Reg, sh: u32) { self.e(0xd340_0000 | sh << 16 | 63 << 10 | rn << 5 | rd); }
    pub fn asr_imm_x(&mut self, rd: Reg, rn: Reg, sh: u32) { self.e(0x9340_0000 | sh << 16 | 63 << 10 | rn << 5 | rd); }
    pub fn lsl_imm_x(&mut self, rd: Reg, rn: Reg, sh: u32) { self.e(0xd340_0000 | ((64 - sh) & 63) << 16 | (63 - sh) << 10 | rn << 5 | rd); }
    pub fn ubfx(&mut self, rd: Reg, rn: Reg, lsb: u32, width: u32) { self.e(0x5300_0000 | lsb << 16 | (lsb + width - 1) << 10 | rn << 5 | rd); }
    pub fn sbfx(&mut self, rd: Reg, rn: Reg, lsb: u32, width: u32) { self.e(0x1300_0000 | lsb << 16 | (lsb + width - 1) << 10 | rn << 5 | rd); }
    pub fn sxth(&mut self, rd: Reg, rn: Reg) { self.sbfx(rd, rn, 0, 16); }
    pub fn uxth(&mut self, rd: Reg, rn: Reg) { self.ubfx(rd, rn, 0, 16); }
    pub fn lslv(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x1ac0_2000 | rm << 16 | rn << 5 | rd); }
    pub fn lsrv(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x1ac0_2400 | rm << 16 | rn << 5 | rd); }
    pub fn asrv(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x1ac0_2800 | rm << 16 | rn << 5 | rd); }
    pub fn lsrv_x(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x9ac0_2400 | rm << 16 | rn << 5 | rd); }
    pub fn mul(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x1b00_7c00 | rm << 16 | rn << 5 | rd); }
    pub fn umull(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x9ba0_7c00 | rm << 16 | rn << 5 | rd); }
    pub fn smull(&mut self, rd: Reg, rn: Reg, rm: Reg) { self.e(0x9b20_7c00 | rm << 16 | rn << 5 | rd); }
    pub fn clz(&mut self, rd: Reg, rn: Reg) { self.e(0x5ac0_1000 | rn << 5 | rd); }
    pub fn csel(&mut self, rd: Reg, rn: Reg, rm: Reg, c: Cond) { self.e(0x1a80_0000 | rm << 16 | (c as u32) << 12 | rn << 5 | rd); }
    pub fn csinc(&mut self, rd: Reg, rn: Reg, rm: Reg, c: Cond) { self.e(0x1a80_0400 | rm << 16 | (c as u32) << 12 | rn << 5 | rd); }
    pub fn csneg(&mut self, rd: Reg, rn: Reg, rm: Reg, c: Cond) { self.e(0x5a80_0400 | rm << 16 | (c as u32) << 12 | rn << 5 | rd); }
    pub fn cset(&mut self, rd: Reg, c: Cond) { self.csinc(rd, ZR, ZR, c.invert()); }
    pub fn cneg(&mut self, rd: Reg, rn: Reg, c: Cond) { self.csneg(rd, rn, rn, c.invert()); }

    // ---------------------------------------------------------------- memory
    pub fn ldr(&mut self, rt: Reg, rn: Reg, off: u32) { debug_assert!(off.is_multiple_of(4) && off < 16384); self.e(0xb940_0000 | (off / 4) << 10 | rn << 5 | rt); }
    pub fn str(&mut self, rt: Reg, rn: Reg, off: u32) { debug_assert!(off.is_multiple_of(4) && off < 16384); self.e(0xb900_0000 | (off / 4) << 10 | rn << 5 | rt); }
    pub fn ldrh(&mut self, rt: Reg, rn: Reg, off: u32) { debug_assert!(off.is_multiple_of(2) && off < 8192); self.e(0x7940_0000 | (off / 2) << 10 | rn << 5 | rt); }
    pub fn ldr_x(&mut self, rt: Reg, rn: Reg, off: u32) { debug_assert!(off.is_multiple_of(8) && off < 32768); self.e(0xf940_0000 | (off / 8) << 10 | rn << 5 | rt); }
    pub fn str_x(&mut self, rt: Reg, rn: Reg, off: u32) { debug_assert!(off.is_multiple_of(8) && off < 32768); self.e(0xf900_0000 | (off / 8) << 10 | rn << 5 | rt); }
    /// `ldr wt, [xn, wm, uxtw #2]`
    pub fn ldr_idx(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0xb860_0800 | wm << 16 | 0b010 << 13 | 1 << 12 | rn << 5 | rt); }
    /// `str wt, [xn, wm, uxtw #2]`
    pub fn str_idx(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0xb820_0800 | wm << 16 | 0b010 << 13 | 1 << 12 | rn << 5 | rt); }
    /// Byte-offset register forms `[xn, wm, uxtw]` (no scaling): loads zero-extend, `ldrsh`/`ldrsb` sign-extend to 32 bits.
    pub fn ldr_u(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0xb860_4800 | wm << 16 | rn << 5 | rt); }
    pub fn ldrh_u(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0x7860_4800 | wm << 16 | rn << 5 | rt); }
    pub fn ldrb_u(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0x3860_4800 | wm << 16 | rn << 5 | rt); }
    /// LDRSB Wt, [Xn, Wm, UXTW] (signed byte, 32-bit result)
    pub fn ldrsb_u(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0x38e0_4800 | wm << 16 | rn << 5 | rt); }
    pub fn ldrsh_u(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0x78e0_4800 | wm << 16 | rn << 5 | rt); }
    pub fn str_u(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0xb820_4800 | wm << 16 | rn << 5 | rt); }
    pub fn strh_u(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0x7820_4800 | wm << 16 | rn << 5 | rt); }
    pub fn strb_u(&mut self, rt: Reg, rn: Reg, wm: Reg) { self.e(0x3820_4800 | wm << 16 | rn << 5 | rt); }
    pub fn strb(&mut self, rt: Reg, rn: Reg, off: u32) { debug_assert!(off < 4096); self.e(0x3900_0000 | off << 10 | rn << 5 | rt); }
    /// `stp xt1, xt2, [sp, #-imm]!`
    pub fn stp_pre(&mut self, rt1: Reg, rt2: Reg, rn: Reg, off: i32) { self.e(0xa980_0000 | (((off / 8) as u32) & 0x7f) << 15 | rt2 << 10 | rn << 5 | rt1); }
    pub fn stp(&mut self, rt1: Reg, rt2: Reg, rn: Reg, off: i32) { self.e(0xa900_0000 | (((off / 8) as u32) & 0x7f) << 15 | rt2 << 10 | rn << 5 | rt1); }
    pub fn ldp(&mut self, rt1: Reg, rt2: Reg, rn: Reg, off: i32) { self.e(0xa940_0000 | (((off / 8) as u32) & 0x7f) << 15 | rt2 << 10 | rn << 5 | rt1); }
    /// `ldp xt1, xt2, [sp], #imm`
    pub fn ldp_post(&mut self, rt1: Reg, rt2: Reg, rn: Reg, off: i32) { self.e(0xa8c0_0000 | (((off / 8) as u32) & 0x7f) << 15 | rt2 << 10 | rn << 5 | rt1); }

    // ---------------------------------------------------------------- control
    pub fn b(&mut self, l: Label) { self.fixups.push((self.code.len(), l.0, Fix::B26)); self.e(0x1400_0000); }
    pub fn b_cond(&mut self, c: Cond, l: Label) { self.fixups.push((self.code.len(), l.0, Fix::B19)); self.e(0x5400_0000 | c as u32); }
    pub fn cbz(&mut self, rt: Reg, l: Label) { self.fixups.push((self.code.len(), l.0, Fix::B19)); self.e(0x3400_0000 | rt); }
    pub fn cbnz(&mut self, rt: Reg, l: Label) { self.fixups.push((self.code.len(), l.0, Fix::B19)); self.e(0x3500_0000 | rt); }
    pub fn cbz_x(&mut self, rt: Reg, l: Label) { self.fixups.push((self.code.len(), l.0, Fix::B19)); self.e(0xb400_0000 | rt); }
    pub fn cbnz_x(&mut self, rt: Reg, l: Label) { self.fixups.push((self.code.len(), l.0, Fix::B19)); self.e(0xb500_0000 | rt); }
    /// `tbz xt, #bit, label` (bit may be up to 63 — the test is on the 64-bit register)
    pub fn tbz(&mut self, rt: Reg, bit: u32, l: Label) { let at = self.code.len(); self.e(0x3600_0000 | (bit >> 5) << 31 | (bit & 31) << 19 | rt); self.tb_fix(at, l); }
    pub fn tbnz(&mut self, rt: Reg, bit: u32, l: Label) { let at = self.code.len(); self.e(0x3700_0000 | (bit >> 5) << 31 | (bit & 31) << 19 | rt); self.tb_fix(at, l); }
    fn tb_fix(&mut self, at: usize, l: Label) { self.fixups.push((at, l.0, Fix::B14)); }
    pub fn blr(&mut self, rn: Reg) { self.e(0xd63f_0000 | rn << 5); }
    pub fn br(&mut self, rn: Reg) { self.e(0xd61f_0000 | rn << 5); }
    pub fn ret(&mut self) { self.e(0xd65f_03c0); }
    /// `adr xd, label`
    pub fn adr(&mut self, rd: Reg, l: Label) { self.fixups.push((self.code.len(), l.0, Fix::Adr21)); self.e(0x1000_0000 | rd); }
}

/// The immr:imms field (12 bits, with N = 0 for 32-bit) of a bitmask immediate: `v` is a run of ones, rotated, repeated across the register in a
/// power-of-two element size. None for 0 and for all ones (not encodable) and for anything that is not such a pattern.
pub fn logical_imm(v: u32) -> Option<u32> {
    if v == 0 || v == !0 {
        return None;
    }
    let mut size = 32u32;
    while size > 2 {
        let half = size / 2;
        let mask = (1u32 << half) - 1;
        if v & mask == (v >> half) & mask {
            size = half;
        } else {
            break;
        }
    }
    let mask = if size == 32 { !0u32 } else { (1u32 << size) - 1 };
    let e = v & mask;
    let ones = e.count_ones();
    let run = (1u32 << ones) - 1; // (ones < size <= 32 here: all ones was refused above, and an element of all ones repeats to all ones)
    for immr in 0..size {
        let rotated = if immr == 0 { run } else { ((run >> immr) | (run << (size - immr))) & mask };
        if rotated == e {
            let imms = ((!(size - 1) << 1) | (ones - 1)) & 0x3f;
            return Some(immr << 6 | imms);
        }
    }
    None
}

#[cfg(test)]
mod logical_imm_tests {
    use super::logical_imm;
    /// What the A64 decoder makes of N = 0, immr, imms (DecodeBitMasks, 32-bit): None for a reserved encoding.
    fn decode(immr: u32, imms: u32) -> Option<u32> {
        let t = !imms & 0x3f;
        if t == 0 {
            return None;
        }
        let len = 31 - t.leading_zeros();
        if len < 1 || len > 4 + 1 {
            return None;
        }
        let size = 1u32 << len;
        let levels = size - 1;
        let (s, r) = (imms & levels, immr & levels);
        if s == levels {
            return None;
        }
        let welem = if s + 1 == 32 { !0 } else { (1u32 << (s + 1)) - 1 };
        let mask = if size == 32 { !0u32 } else { (1u32 << size) - 1 };
        let elem = if r == 0 { welem } else { ((welem >> r) | (welem << (size - r))) & mask };
        let mut v = 0u32;
        let mut i = 0;
        while i < 32 {
            v |= elem << i;
            i += size;
        }
        Some(v)
    }
    /// Encodings from clang's assembler: `and w0, w1, #v` assembled, bits 21 to 10 of the word.
    #[test]
    fn matches_the_assembler() {
        let cases: &[(u32, u32)] = &[
        (0x00000001, 0x0),
        (0x00000002, 0x7c0),
        (0x000000ff, 0x7),
        (0x0000ff00, 0x607),
        (0x80000000, 0x40),
        (0x55555555, 0x3c),
        (0xaaaaaaaa, 0x7c),
        (0x0000ffff, 0xf),
        (0xfffffffe, 0x7de),
        (0xf0f0f0f0, 0x133),
        (0x0f0f0f0f, 0x33),
        (0x7fffffff, 0x1e),
        (0xfffffff0, 0x71b),
        (0x00ff00ff, 0x27),
        (0x00000003, 0x1),
        (0x00000006, 0x7c1),
        (0xc0000003, 0x83),
        (0x00000018, 0x741),
        (0x000001c0, 0x682),
        (0xffff0000, 0x40f),
        (0x01010101, 0x30),
        (0x80000001, 0x41),
        (0x000000fe, 0x7c6),
        (0x7ffffffe, 0x7dd),
        ];
        for &(v, want) in cases {
            assert_eq!(logical_imm(v), Some(want), "{v:#x}");
        }
        for v in [0u32, 0xffff_ffff, 0x1234_5678, 0x0000_0026, 0x0000_ff0f] {
            assert_eq!(logical_imm(v), None, "{v:#x} must not encode");
        }
    }
    /// Every canonical encoding the decoder accepts comes back from `logical_imm` as itself.
    #[test]
    fn every_encodable_value_round_trips() {
        let mut n = 0;
        for immr in 0..32u32 {
            for imms in 0..64u32 {
                let Some(v) = decode(immr, imms) else { continue };
                let size = 1u32 << (31 - (!imms & 0x3f).leading_zeros());
                if immr >= size {
                    continue;
                }
                assert_eq!(logical_imm(v), Some(immr << 6 | imms), "immr {immr} imms {imms:#x} v {v:#x}");
                n += 1;
            }
        }
        assert_eq!(n, 2 + 12 + 56 + 240 + 992, "every element size from 2 to 32 bits");
    }
}
