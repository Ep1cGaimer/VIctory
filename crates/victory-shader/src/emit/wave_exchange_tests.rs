//! A console wave64 compute program on a host of 32-lane waves whose code
//! crosses its 32-lane halves ([`super::lower::wave_exchange`]): lane reads,
//! lane counts, a ballot read as numbers and a first-lane read, lowered with
//! the two host waves exchanging through LDS, validated by `dxil.dll` when
//! present, and run on the hardware GPU. Each of the 64 threads stores what
//! it got at dword `thread` of a raw UAV.

use super::host::{Dispatch, Host, Root, View};
use super::{HostShader, lower, validator};
use crate::native::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::{io, ir, resource, structure};

const S_ENDPGM: u32 = 0xBF81_0000;
/// `v_mov_b32 v3, s7`; `v_lshlrev_b32 v2, 2, v0`;
/// `buffer_store_dword v3, v2, s[8:11], 0 offen`.
const STORE_S7: [u32; 4] = [0x7E06_0207, 0x3404_0082, 0xE070_1000, 0x8002_0302];

/// 64 threads in one console wave (the thread index in v0), a root constant
/// in s0 and a raw UAV's descriptor loaded into s[8:11], then `body`.
fn native(body: &[u32]) -> NativeProgram {
    let mut words = vec![
        0xBE85_0380, // s_mov_b32 s5, 0
        0x3D << 26 | 0x02 << 18 | 8 << 6 | 2,
        125 << 25, // s_load_dwordx4 s[8:11], s[4:5], 0
    ];
    words.extend_from_slice(body);
    words.extend_from_slice(&STORE_S7);
    words.push(S_ENDPGM);
    NativeProgram {
        stage: HwStage::Cs,
        wave64: true,
        vgprs: 16,
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
            dwords: 4,
            user_data: 4,
            slot: 1,
        }],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 5 << 1, // 5 user SGPRs: s0..s4
            threads: [64, 1, 1],
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    }
}

/// `body` lowered for a 32-lane host with the exchange; it must keep it.
fn lowered(body: &[u32]) -> HostShader {
    let native = native(body);
    let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
    let st = structure::structure(&p).expect("structure");
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).expect("track");
    let io = io::raise(&p, &native).expect("io::raise");
    let was = lower::TEST_WAVE.replace(Some((32, true)));
    let l = lower::lower(&lower::Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: Vec::new(),
        outputs: Vec::new(),
        prims: Vec::new(),
        mesh_stride: 0,
        view_marks: false,
    });
    let kept = lower::exchanged();
    lower::TEST_WAVE.set(was);
    let l = l.expect("lower");
    assert!(kept, "lowered without the exchange");
    let shader = HostShader {
        stage: HwStage::Cs,
        dxil: l.dxil,
        user_data: l.user_data,
        samplers: l.samplers,
        static_samplers: Vec::new(),
        root_cbvs: l.root_cbvs,
        root_srvs: l.root_srvs,
        root_uavs: l.root_uavs,
        memory: l.memory,
        memory_writes: l.memory_writes,
        view_addresses: l.view_addresses,
        heap: l.heap,
        heap_writes: l.heap_writes,
        view_marks: l.view_marks,
    };
    if let Ok(mut v) = validator::Validator::load(validator::Validator::default_path().as_deref()) {
        v.validate(&mut shader.dxil.clone()).expect("DXIL validation failed");
    } else {
        eprintln!("no dxil.dll; validation skipped");
    }
    shader
}

/// The 64 dwords the threads stored, with s0 = `s0`, on the hardware GPU
/// (None without one that takes SM 6.6).
fn on_the_card(shader: &HostShader, s0: u32) -> Option<Vec<u32>> {
    let Ok(host) = Host::new(false) else {
        eprintln!("no SM 6.6 hardware device; not run");
        return None;
    };
    let (root, pso) = host
        .compute_pipeline(shader)
        .unwrap_or_else(|e| panic!("compute_pipeline failed: {e}"));
    let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
    for u in &shader.user_data {
        if let (RootType::Constants, 0, 0) = (u.root, u.slot, u.dword) {
            ud[u.index as usize] = s0;
        }
    }
    let out = host
        .dispatch(
            &root,
            &pso,
            &Dispatch {
                roots: vec![(0, Root::Constants(ud))],
                views: &[View::RawUav(64)],
                samplers: &[],
                groups: [1, 1, 1],
            },
        )
        .unwrap_or_else(|e| panic!("dispatch failed: {e}"));
    Some(out[0][..64].to_vec())
}

fn check(body: &[u32], cases: &[(u32, u32)], what: &str) {
    let shader = lowered(body);
    let _one = super::valu_tests::ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    for &(s0, want) in cases {
        let Some(got) = on_the_card(&shader, s0) else { return };
        assert_eq!(got, vec![want; 64], "{what}, s0 = {s0}");
    }
    eprintln!("ran on the hardware device: {what}");
}

/// `v_mbcnt` gives every thread its console lane; `v_readlane` of lane s0
/// gives every thread that lane's number, whichever host wave holds it.
#[test]
fn every_thread_reads_the_lane_s0_names_in_either_half() {
    let body = [
        0xD765_0001, 0x0001_00C1, // v_mbcnt_lo_u32_b32 v1, -1, 0
        0xD766_0001, 0x0002_02C1, // v_mbcnt_hi_u32_b32 v1, -1, v1
        0xD760_0007, 0x0000_0101, // v_readlane_b32 s7, v1, s0
    ];
    let cases: Vec<(u32, u32)> = [0, 5, 31, 32, 40, 63, 64 + 7].iter().map(|&l| (l, l & 63)).collect();
    check(&body, &cases, "lane reads");
}

/// The ballot of the lanes below s0, read as numbers: both dwords hold
/// every console lane's bit.
#[test]
fn a_ballot_read_as_numbers_holds_both_halves_bits() {
    let low = [
        0x7D88_0000, // v_cmp_gt_u32 vcc, s0, v0
        0xBE87_036A, // s_mov_b32 s7, vcc_lo
    ];
    check(&low, &[(10, 0x3FF), (40, u32::MAX), (0, 0)], "the ballot's low dword");
    let high = [
        0x7D88_0000, // v_cmp_gt_u32 vcc, s0, v0
        0xBE87_036B, // s_mov_b32 s7, vcc_hi
    ];
    check(&high, &[(10, 0), (40, 0xFF), (64, u32::MAX)], "the ballot's high dword");
}

/// `v_readfirstlane` of the thread index with every lane in EXEC: lane 0's,
/// in the upper half too.
#[test]
fn the_first_lane_is_lane_0_in_both_halves() {
    let body = [
        0xD760_0006, 0x0000_0100, // v_readlane_b32 s6, v0, s0 (the program crosses halves)
        0x7E0E_0500, // v_readfirstlane_b32 s7, v0
    ];
    check(&body, &[(0, 0)], "first-lane reads");
}
