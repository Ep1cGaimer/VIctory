//! Implicit derivatives in compute (`image_sample` with implicit level):
//! the console takes them over quads of lanes 4n..4n+3, which are four
//! consecutive flat thread ids, x across lanes 0-1 and y across lanes 0-2,
//! whatever the group's shape. DXIL (SM 6.6) has them in compute too, but
//! for a 2D `numthreads` its quads are 2x2 blocks of thread ids, so the
//! translation declares the group flat (1D quads: consecutive group
//! indices). Synthetic GFX10 programs, validated by `dxil.dll` when present
//! and run on every SM 6.6 host device: each lane samples a mip chain whose
//! level n holds n at coordinates whose x step is one texel per thread and
//! y step four, so the console's quads choose level 1 (the quad's y step is
//! two threads along x), where 2x2 quads would choose level 2.

use super::host::{Dispatch, Host, Root, View};
use super::{HostShader, validator};
use crate::dxbc::{Container, FourCC};
use crate::native::{
    ComputeIo, HwStage, NativeProgram, ResourceType, RootParam, RootType, StageIo, UsedResource,
};
use crate::{io, ir, resource, structure};

const S_ENDPGM: u32 = 0xBF81_0000;
/// SSRC literal: the next dword.
const LIT: u8 = 255;

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

/// VOP1 `v_cvt_f32_u32 vdst, vsrc`.
const fn v_cvt_f32_u32(vdst: u8, vsrc: u8) -> u32 {
    0x7E00_0000 | (vdst as u32) << 17 | 0x06 << 9 | (256 + vsrc as u32)
}

/// VOP2 `v_mul_f32 vdst, k, vdst` with a literal `k`.
const fn v_mul_f32_lit(vdst: u8, k: f32) -> [u32; 2] {
    [
        0x08 << 25 | (vdst as u32) << 17 | (vdst as u32) << 9 | LIT as u32,
        k.to_bits(),
    ]
}

/// MIMG `image_sample v[vdata:vdata+3], v[vaddr:vaddr+1], s[srsrc:+7],
/// s[ssamp:+3] dmask:0xf dim:2D`.
const fn image_sample_2d(vdata: u8, vaddr: u8, srsrc: u8, ssamp: u8) -> [u32; 2] {
    [
        0x3C << 26 | 0x20 << 18 | 0xF << 8 | 1 << 3,
        (ssamp as u32 >> 2) << 21 | (srsrc as u32 >> 2) << 16 | (vdata as u32) << 8 | vaddr as u32,
    ]
}

/// MIMG `image_store v[vdata:vdata+3], v[vaddr:vaddr+1], s[srsrc:+7]
/// dmask:0xf dim:2D unorm`.
const fn image_store_2d(vdata: u8, vaddr: u8, srsrc: u8) -> [u32; 2] {
    [
        0x3C << 26 | 0x08 << 18 | 1 << 12 | 0xF << 8 | 1 << 3,
        (srsrc as u32 >> 2) << 16 | (vdata as u32) << 8 | vaddr as u32,
    ]
}

/// Point sampling with point mips over every level, coordinates clamped.
const SAMPLER: [u32; 4] = [2 | 2 << 3 | 2 << 6, (15 * 256) << 12, 1 << 26, 0];

/// A wave32 program of this group shape: the texture table in s0, the UAV
/// table in s1, thread ids x and y in v0 and v1. Each lane samples at
/// `(x / 16, y / 4)` and stores what it read at texel `(x, y)`.
fn program(threads: [u32; 3]) -> NativeProgram {
    let mut words = vec![
        s_mov_b32(2, 1), // s[2:3]: the UAV table
        s_mov_b32(1, 0x80),
        s_mov_b32(3, 0x80),
    ];
    words.extend(s_load_dwordx8(8, 0));
    words.extend(s_load_dwordx8(16, 2));
    for (n, w) in SAMPLER.into_iter().enumerate() {
        words.extend([s_mov_b32(24 + n as u8, LIT), w]);
    }
    words.push(v_cvt_f32_u32(2, 0));
    words.push(v_cvt_f32_u32(3, 1));
    words.extend(v_mul_f32_lit(2, 1.0 / 16.0));
    words.extend(v_mul_f32_lit(3, 0.25));
    words.extend(image_sample_2d(4, 2, 8, 24));
    words.extend(image_store_2d(4, 0, 16));
    words.push(S_ENDPGM);
    let table = |kind, user_data, slot| {
        (
            RootParam {
                root: RootType::Table,
                table_offset_dwords: 0,
                sgprs: 1,
                base_register: 0,
                user_data,
                slot,
            },
            UsedResource {
                kind,
                root: RootType::Table,
                table_offset_dwords: 0,
                lower_bound: 0,
                stride: 0,
                dwords: 8,
                user_data,
                slot,
            },
        )
    };
    let (srv, srv_used) = table(ResourceType::Srv, 0, 1);
    let (uav, uav_used) = table(ResourceType::Uav, 1, 2);
    NativeProgram {
        stage: HwStage::Cs,
        wave64: false,
        vgprs: 8,
        layout: vec![srv, uav],
        used: vec![srv_used, uav_used],
        io: StageIo::Cs(ComputeIo {
            // 2 user SGPRs; thread ids x and y.
            rsrc2: 2 << 1 | 1 << 11,
            threads,
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    }
}

fn translate(native: &NativeProgram) -> Result<HostShader, crate::ShaderError> {
    let p = ir::build(&native.code, native.wave64, native.vgprs).expect("ir::build");
    let st = structure::structure(&p).expect("structure");
    let res =
        resource::track(&p, &native.layout, native.stage, Some(&st)).expect("resource::track");
    let io = io::raise(&p, native).expect("io::raise");
    // A compute program has no signatures: an empty container.
    let bytes = crate::dxbc::build(&[]);
    let c = Container::parse(&bytes)?;
    super::emit(&c, native, &p, &st, &io, &res)
}

/// The `numthreads` in PSV0 (runtime info bytes 36-47) and the runtime
/// info's size.
fn psv_threads(dxil: &[u8]) -> ([u32; 3], u32) {
    let c = Container::parse(dxil).unwrap();
    let psv = c.part(FourCC::PSV0).unwrap().data;
    let u = |at: usize| u32::from_le_bytes(psv[at..at + 4].try_into().unwrap());
    ([u(4 + 36), u(4 + 40), u(4 + 44)], u(0))
}

#[test]
fn compute_derivatives_take_the_consoles_quads() {
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for threads in [[8, 8, 1], [16, 4, 1], [32, 1, 1]] {
        let shader = translate(&program(threads)).expect("translate");
        // Declared flat (1D quads), with PSV0 runtime info version 3
        // (validator 1.8).
        let n = threads.iter().product();
        assert_eq!(psv_threads(&shader.dxil), ([n, 1, 1], 52), "{threads:?}");
        if let Ok(mut v) =
            validator::Validator::load(validator::Validator::default_path().as_deref())
        {
            v.validate(&mut shader.dxil.clone())
                .unwrap_or_else(|e| panic!("{threads:?}: {e}"));
        }
        let [w, h, _] = threads;
        for (label, warp) in [("hardware", false), ("warp", true)] {
            let Ok(host) = Host::new(warp) else {
                eprintln!("no SM 6.6 {label} device; skipped");
                continue;
            };
            let (root, pso) = host
                .compute_pipeline(&shader)
                .unwrap_or_else(|e| panic!("{label} {threads:?}: compute_pipeline: {e}"));
            let mut ud = vec![0u32; shader.user_data_dwords().max(1) as usize];
            for u in &shader.user_data {
                // Heap index 0 the texture, 1 the UAV (console units).
                ud[u.index as usize] = match (u.root, u.slot) {
                    (RootType::Table, 1) => 0,
                    (RootType::Table, 2) => 32,
                    other => panic!("unexpected user data {other:?}"),
                };
            }
            let out = host
                .dispatch(
                    &root,
                    &pso,
                    &Dispatch {
                        roots: vec![(0, Root::Constants(ud))],
                        views: &[View::Levels(16, 5), View::TextureUav(w, h)],
                        samplers: &[],
                        groups: [1, 1, 1],
                    },
                )
                .unwrap_or_else(|e| panic!("{label} {threads:?}: dispatch: {e}"));
            let levels: Vec<f32> = out[0].chunks(4).map(|t| f32::from_bits(t[0])).collect();
            assert!(
                levels.iter().all(|&l| l == 1.0),
                "{label} {threads:?}: levels {levels:?}"
            );
        }
    }
}

/// A mesh shader with derivatives declares them (ShaderFlags bit 29,
/// SFI0 0x100_0000) under validator 1.8, as DXC does for ms_6_6.
#[test]
fn mesh_derivatives_are_declared() {
    use super::shader::{Mesh, Shader};
    let mut s = Shader::new(false);
    s.threads = Some([32, 1, 1]);
    s.mesh = Some(Mesh {
        max_vertices: 32,
        max_primitives: 32,
        topology: 2,
    });
    s.derivatives = true;
    let b = s.entry;
    s.m.terminate(b, super::bitcode::Term::Ret);
    let dxil = s.finish();
    let c = Container::parse(&dxil).unwrap();
    let sfi0 = c.part(FourCC::SFI0).unwrap().data;
    let features = u64::from_le_bytes(sfi0.try_into().unwrap());
    assert_eq!(features & 0x100_0000, 0x100_0000, "SFI0 {features:#x}");
    assert_eq!(psv_threads(&dxil), ([32, 1, 1], 52));
}

#[test]
fn compute_derivatives_need_whole_quads() {
    let e = translate(&program([3, 3, 1])).expect_err("refused");
    assert!(e.to_string().contains("partial quads"), "{e}");
    // Without derivatives a 2D group stays 2D, under validator 1.6.
    let mut native = program([8, 8, 1]);
    let code: Vec<u32> = native
        .code
        .chunks(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let sample = image_sample_2d(4, 2, 8, 24);
    let at = code.windows(2).position(|w| w == sample).unwrap();
    // image_sample_lz (0x27): level 0, no derivatives.
    let mut lz = code.clone();
    lz[at] = lz[at] & !(0x7F << 18) | 0x27 << 18;
    native.code = lz.iter().flat_map(|w| w.to_le_bytes()).collect();
    let shader = translate(&native).expect("translate");
    assert_eq!(psv_threads(&shader.dxil), ([8, 8, 1], 48));
}
