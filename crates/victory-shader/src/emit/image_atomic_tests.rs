//! 64-bit image atomics (`image_atomic_*` with two data dwords, four for
//! `cmpswap`): synthetic GFX10 compute programs lowered to SM 6.6 64-bit
//! typed atomics, validated by `dxil.dll` when present, and run on every
//! host device that supports them on an R32G32_UINT texture UAV. Each case
//! is chosen so that a per-dword atomic would give another answer.

use super::host::{Dispatch, Host, Root, View};
use super::{HostShader, lower, validator};
use crate::dxbc::{Container, FourCC};
use crate::native::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::{io, ir, resource, structure};

const S_ENDPGM: u32 = 0xBF81_0000;

/// SOP1 `s_mov_b32 sdst, src0`.
const fn s_mov_b32(sdst: u8, src0: u8) -> u32 {
    0xBE80_0000 | (sdst as u32) << 16 | 0x03 << 8 | src0 as u32
}

/// SMEM `s_load_dwordx8 s[dst:dst+7], s[base:base+1], 0` (soffset = null).
const fn s_load_dwordx8(dst: u8, base: u8) -> [u32; 2] {
    [
        0x3D << 26 | 0x03 << 18 | (dst as u32) << 6 | (base as u32 >> 1),
        125 << 25,
    ]
}

/// VOP1 `v_mov_b32 vdst, src0` (`src0`: 0..105 SGPR, 128.. inline
/// constants).
const fn v_mov_b32(vdst: u8, src0: u16) -> u32 {
    0x7E00_0000 | (vdst as u32) << 17 | 0x01 << 9 | (src0 as u32 & 0x1FF)
}

/// MIMG `op vdata, v[vaddr:vaddr+1], s[srsrc:srsrc+7] dmask dim:2D unorm`.
const fn image_2d(op: u8, vdata: u8, vaddr: u8, srsrc: u8, dmask: u8, glc: bool) -> [u32; 2] {
    [
        0x3C << 26
            | (op as u32) << 18
            | (glc as u32) << 13
            | 1 << 12
            | (dmask as u32) << 8
            | 1 << 3,
        (srsrc as u32 >> 2) << 16 | (vdata as u32) << 8 | vaddr as u32,
    ]
}

const UMAX: u8 = 0x17;
const ADD: u8 = 0x11;
const SMIN: u8 = 0x14;
const SWAP: u8 = 0x0F;
const CMPSWAP: u8 = 0x10;

/// The descriptor in s[8:15], texel coordinates (0, 0), (1, 0) and (2, 0)
/// in v[20:21], v[22:23] and v[24:25], then `body`. The root constants
/// arrive in s0..s3.
fn build(body: &[u32]) -> HostShader {
    let mut words = vec![s_mov_b32(5, 0x80)]; // s[4:5]: the table, offset 0
    words.extend(s_load_dwordx8(8, 4));
    for (reg, k) in [
        (20, 128),
        (21, 128),
        (22, 129),
        (23, 128),
        (24, 130),
        (25, 128),
    ] {
        words.push(v_mov_b32(reg, k));
    }
    words.extend_from_slice(body);
    words.push(S_ENDPGM);
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: false,
        vgprs: 32,
        layout: vec![
            RootParam {
                root: RootType::Constants,
                table_offset_dwords: 0,
                sgprs: 4,
                base_register: 0,
                user_data: 0,
                slot: 0,
            },
            RootParam {
                root: RootType::Table,
                table_offset_dwords: 0,
                sgprs: 1,
                base_register: 0,
                user_data: 4,
                slot: 1,
            },
        ],
        used: vec![UsedResource {
            kind: ResourceType::Uav,
            root: RootType::Table,
            table_offset_dwords: 0,
            lower_bound: 0,
            stride: 0,
            dwords: 8,
            user_data: 4,
            slot: 1,
        }],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 5 << 1, // 5 user SGPRs: s0..s4
            threads: [1, 1, 1],
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
    let res = resource::track(&p, &native.layout, native.stage).expect("resource::track");
    let io = io::raise(&p, &native).expect("io::raise");
    let st = structure::structure(&p).expect("structure");
    let lowered = lower::lower(&lower::Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: Vec::new(),
        outputs: Vec::new(),
        prims: Vec::new(),
        mesh_stride: 0,
    })
    .expect("lower");
    let shader = HostShader {
        stage: HwStage::Cs,
        dxil: lowered.dxil,
        user_data: lowered.user_data,
        samplers: lowered.samplers,
        static_samplers: Vec::new(),
        root_cbvs: lowered.root_cbvs,
        root_srvs: lowered.root_srvs,
    };
    if let Ok(mut v) = validator::Validator::load(validator::Validator::default_path().as_deref()) {
        v.validate(&mut shader.dxil.clone())
            .expect("DXIL validation failed");
    }
    // SM 6.6's 64-bit typed atomics on a heap resource: Int64Ops (0x8000),
    // AtomicInt64OnTypedResource (0x40_0000) and AtomicInt64OnHeapResource
    // (0x1000_0000), as DXC declares them for `RWTexture2D<uint64_t>`.
    let c = Container::parse(&shader.dxil).unwrap();
    let sfi0 = c.part(FourCC::SFI0).unwrap().data;
    let features = u64::from_le_bytes(sfi0.try_into().unwrap());
    assert_eq!(features & 0x1040_8000, 0x1040_8000, "SFI0 {features:#x}");
    shader
}

/// Runs `shader` with each case's four root constants on every SM 6.6
/// host with 64-bit atomics on heap typed resources; `check` gets the
/// first three texels of row 0 of a zeroed 4x1 R32G32_UINT texture.
fn run(shader: &HostShader, cases: &[[u32; 4]], mut check: impl FnMut(&str, [u32; 4], [u64; 3])) {
    let _one = super::host::gpu_test_lock();
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            eprintln!("no SM 6.6 {label} device; skipped");
            continue;
        };
        if !host.typed_heap_atomics64() {
            eprintln!("{label}: no 64-bit atomics on heap typed resources; skipped");
            continue;
        }
        let (root, pso) = host
            .compute_pipeline(shader)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        for &inputs in cases {
            let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
            for u in &shader.user_data {
                ud[u.index as usize] = match (u.root, u.slot) {
                    (RootType::Constants, 0) => inputs[u.dword as usize],
                    (RootType::Table, 1) => 0,
                    other => panic!("unexpected user data {other:?}"),
                };
            }
            let out = host
                .dispatch(
                    &root,
                    &pso,
                    &Dispatch {
                        roots: vec![(0, Root::Constants(ud))],
                        views: &[View::Uint2TextureUav(4, 1)],
                        samplers: &[],
                        groups: [1, 1, 1],
                    },
                )
                .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
            // R holds the low dword, G the high one.
            let t = |k: usize| u64::from(out[0][2 * k]) | u64::from(out[0][2 * k + 1]) << 32;
            check(label, inputs, [t(0), t(1), t(2)]);
        }
    }
}

fn pair(lo: u32, hi: u32) -> u64 {
    u64::from(lo) | u64::from(hi) << 32
}

/// Two atomics on texel (0, 0), which starts at zero: first with (s0, s1),
/// then with (s2, s3), each (low dword, high dword).
fn twice(op: u8) -> HostShader {
    let mut body = Vec::new();
    for (reg, s) in [(4, 0), (5, 1), (6, 2), (7, 3)] {
        body.push(v_mov_b32(reg, s));
    }
    body.extend(image_2d(op, 4, 20, 8, 0x3, false));
    body.extend(image_2d(op, 6, 20, 8, 0x3, false));
    build(&body)
}

#[test]
fn unsigned_max_compares_whole_64_bit_texels() {
    // (hi 1, lo max) then (hi 2, lo 0): the larger is (2, 0); a per-dword
    // maximum would give (2, max).
    let cases = [
        [u32::MAX, 1, 0, 2],
        [5, 7, 9, 7],
        [3, 0x8000_0000, u32::MAX, 0x7FFF_FFFF],
    ];
    run(&twice(UMAX), &cases, |label, [a, b, c, d], got| {
        let want = pair(a, b).max(pair(c, d));
        assert_eq!(got[0], want, "{label}: umax {:x?}", [a, b, c, d]);
    });
}

#[test]
fn add_carries_into_the_high_dword() {
    let cases = [[u32::MAX, 0, 1, 0], [0x8000_0000, 3, 0x8000_0001, 4]];
    run(&twice(ADD), &cases, |label, [a, b, c, d], got| {
        let want = pair(a, b).wrapping_add(pair(c, d));
        assert_eq!(got[0], want, "{label}: add {:x?}", [a, b, c, d]);
    });
}

#[test]
fn signed_min_orders_by_the_high_dword_sign() {
    // From zero: a negative 64-bit value (high dword's sign) is the minimum
    // whatever its low dword.
    let cases = [
        [1, 0, 0, 0xFFFF_FFFF],
        [0, 0xFFFF_FFFE, 0xFFFF_FFFF, 0xFFFF_FFFF],
    ];
    run(&twice(SMIN), &cases, |label, [a, b, c, d], got| {
        let want = 0i64.min(pair(a, b) as i64).min(pair(c, d) as i64) as u64;
        assert_eq!(got[0], want, "{label}: smin {:x?}", [a, b, c, d]);
    });
}

#[test]
fn compare_swap_returns_the_old_texel() {
    // texel 0: 0 -> (s0, s1) (compare 0 holds) -> (s2, s3) (compare
    // (s0, s1) holds) -> unchanged (compare (s0, s1) is stale). The old
    // values the last two return (glc) are swapped into texels 1 and 2.
    let mut body = Vec::new();
    let movs = [
        (4, 0),
        (5, 1),
        (6, 128),
        (7, 128),
        (8, 2),
        (9, 3),
        (10, 0),
        (11, 1),
        (12, 129),
        (13, 130),
        (14, 0),
        (15, 1),
    ];
    for (reg, src) in movs {
        body.push(v_mov_b32(reg, src));
    }
    body.extend(image_2d(CMPSWAP, 4, 20, 8, 0xF, true));
    body.extend(image_2d(CMPSWAP, 8, 20, 8, 0xF, true));
    body.extend(image_2d(CMPSWAP, 12, 20, 8, 0xF, true));
    body.extend(image_2d(SWAP, 8, 22, 8, 0x3, false));
    body.extend(image_2d(SWAP, 12, 24, 8, 0x3, false));
    let shader = build(&body);
    let cases = [
        [0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444],
        [7, 0, 0, 7],
    ];
    run(&shader, &cases, |label, [a, b, c, d], got| {
        let (first, second) = (pair(a, b), pair(c, d));
        assert_eq!(
            got,
            [second, first, second],
            "{label}: cmpswap {:x?}",
            [a, b, c, d]
        );
    });
}
