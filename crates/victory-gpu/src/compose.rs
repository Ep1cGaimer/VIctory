//! Present planes drawn into one picture. On Series S the title presents
//! two planes each frame (measured: plane descriptions 0x48 bytes apart,
//! token, resource count, resources, then a pointer to the plane's source
//! rectangle): its 1920x1080 scene and a 3840x2160 interface plane drawn
//! over it. The window shows their composite: the scene scaled to the
//! picture, each later plane over it, premultiplied alpha.
//!
//! shortcut: no destination placement, colour space or plane flags; every
//! plane covers the whole picture.

use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{PCSTR, s};

const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=4, b0), DescriptorTable(SRV(t0)), StaticSampler(s0, filter=FILTER_MIN_MAG_MIP_LINEAR, addressU=TEXTURE_ADDRESS_CLAMP, addressV=TEXTURE_ADDRESS_CLAMP, addressW=TEXTURE_ADDRESS_CLAMP)"
cbuffer Rect : register(b0) { float2 origin; float2 extent; };
Texture2D<float4> plane : register(t0);
SamplerState linear_clamp : register(s0);
struct V { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
[RootSignature(RS)]
V vs(uint id : SV_VertexID) {
    V v;
    float2 t = float2((id << 1) & 2, id & 2);
    v.pos = float4(t * float2(2, -2) + float2(-1, 1), 0, 1);
    v.uv = origin + t * extent;
    return v;
}
[RootSignature(RS)]
float4 ps(V v) : SV_Target { return plane.Sample(linear_clamp, v.uv); }
"#;

/// A plane to show: its host texture, the host state it is in (and stays
/// in), and the part of it shown, in texels (left, top, right, bottom).
pub(crate) struct Shown<'a> {
    pub(crate) texture: &'a ID3D12Resource,
    pub(crate) state: u32,
    pub(crate) rect: Option<[u32; 4]>,
}

pub(crate) struct Compositor {
    root: ID3D12RootSignature,
    /// Plane 0, written as it is, and the planes over it.
    opaque: ID3D12PipelineState,
    over: ID3D12PipelineState,
    format: DXGI_FORMAT,
    views: ID3D12DescriptorHeap,
    targets: ID3D12DescriptorHeap,
}

// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Compositor {}

/// Most planes composited.
pub(crate) const PLANES: usize = 4;

fn compile(entry: PCSTR, target: PCSTR) -> windows::core::Result<ID3DBlob> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source and names live through the call; out pointers ours.
    let compiled = unsafe {
        D3DCompile(SOURCE.as_ptr().cast(), SOURCE.len(), s!("compose"), None, None, entry, target, 0, 0, &mut code, Some(&mut errors))
    };
    if let Err(e) = compiled {
        if let Some(errors) = errors {
            // SAFETY: the blob's own bytes.
            let text = unsafe { std::slice::from_raw_parts(errors.GetBufferPointer().cast::<u8>(), errors.GetBufferSize()) };
            tracing::warn!("plane compositor: {}", String::from_utf8_lossy(text));
        }
        return Err(e);
    }
    code.ok_or_else(windows::core::Error::empty)
}

fn bytecode(blob: &ID3DBlob) -> D3D12_SHADER_BYTECODE {
    // SAFETY: the blob's own bytes, alive while it is.
    unsafe { D3D12_SHADER_BYTECODE { pShaderBytecode: blob.GetBufferPointer(), BytecodeLength: blob.GetBufferSize() } }
}

/// The format a view of a presented texture reads it through.
fn view_format(format: DXGI_FORMAT) -> DXGI_FORMAT {
    match format {
        DXGI_FORMAT_R8G8B8A8_TYPELESS => DXGI_FORMAT_R8G8B8A8_UNORM,
        DXGI_FORMAT_B8G8R8A8_TYPELESS => DXGI_FORMAT_B8G8R8A8_UNORM,
        DXGI_FORMAT_R10G10B10A2_TYPELESS => DXGI_FORMAT_R10G10B10A2_UNORM,
        DXGI_FORMAT_R16G16B16A16_TYPELESS => DXGI_FORMAT_R16G16B16A16_FLOAT,
        f => f,
    }
}

impl Compositor {
    pub(crate) fn new(device: &ID3D12Device, format: DXGI_FORMAT) -> windows::core::Result<Self> {
        let vs = compile(s!("vs"), s!("vs_5_1"))?;
        let ps = compile(s!("ps"), s!("ps_5_1"))?;
        // SAFETY: plain object creation from our own shaders.
        unsafe {
            let code = std::slice::from_raw_parts(vs.GetBufferPointer().cast::<u8>(), vs.GetBufferSize());
            let root: ID3D12RootSignature = device.CreateRootSignature(0, code)?;
            let pso = |blend: bool| -> windows::core::Result<ID3D12PipelineState> {
                let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
                    pRootSignature: std::mem::transmute_copy(&root),
                    VS: bytecode(&vs),
                    PS: bytecode(&ps),
                    SampleMask: u32::MAX,
                    RasterizerState: D3D12_RASTERIZER_DESC {
                        FillMode: D3D12_FILL_MODE_SOLID,
                        CullMode: D3D12_CULL_MODE_NONE,
                        DepthClipEnable: true.into(),
                        ..Default::default()
                    },
                    PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
                    NumRenderTargets: 1,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    ..Default::default()
                };
                desc.RTVFormats[0] = format;
                desc.BlendState.RenderTarget[0] = D3D12_RENDER_TARGET_BLEND_DESC {
                    BlendEnable: blend.into(),
                    LogicOpEnable: false.into(),
                    SrcBlend: D3D12_BLEND_ONE,
                    DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
                    BlendOp: D3D12_BLEND_OP_ADD,
                    SrcBlendAlpha: D3D12_BLEND_ONE,
                    DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
                    BlendOpAlpha: D3D12_BLEND_OP_ADD,
                    LogicOp: D3D12_LOGIC_OP_NOOP,
                    RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
                };
                // The description borrows the root signature (ManuallyDrop:
                // never released through it).
                device.CreateGraphicsPipelineState(&desc)
            };
            let opaque = pso(false)?;
            let over = pso(true)?;
            let views = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: PLANES as u32,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                NodeMask: 0,
            })?;
            let targets = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 1,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
                NodeMask: 0,
            })?;
            Ok(Compositor { root, opaque, over, format, views, targets })
        }
    }

    pub(crate) fn format(&self) -> DXGI_FORMAT {
        self.format
    }

    /// Records, in `list`, the planes drawn into `back` (`width` x
    /// `height`, in the render target state): views, viewport and draws.
    /// The planes must be readable by pixel shaders.
    pub(crate) fn record(
        &self,
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        back: &ID3D12Resource,
        (width, height): (u32, u32),
        planes: &[Shown<'_>],
    ) {
        // SAFETY: an open list, our heaps and pipelines, live textures.
        unsafe {
            let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) as usize;
            let cpu = self.views.GetCPUDescriptorHandleForHeapStart();
            let gpu = self.views.GetGPUDescriptorHandleForHeapStart();
            let rtv = self.targets.GetCPUDescriptorHandleForHeapStart();
            device.CreateRenderTargetView(back, None, rtv);
            list.SetGraphicsRootSignature(&self.root);
            list.SetDescriptorHeaps(&[Some(self.views.clone())]);
            list.OMSetRenderTargets(1, Some(&rtv), false, None);
            list.RSSetViewports(&[D3D12_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: width as f32, Height: height as f32, MinDepth: 0.0, MaxDepth: 1.0 }]);
            list.RSSetScissorRects(&[windows::Win32::Foundation::RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 }]);
            list.IASetPrimitiveTopology(windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            for (i, plane) in planes.iter().take(PLANES).enumerate() {
                let desc = plane.texture.GetDesc();
                let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: view_format(desc.Format),
                    ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2D: D3D12_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1, PlaneSlice: 0, ResourceMinLODClamp: 0.0 },
                    },
                };
                device.CreateShaderResourceView(plane.texture, Some(&view), D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu.ptr + i * step });
                let (w, h) = (desc.Width as f32, desc.Height as f32);
                let [l, t, r, b] = plane.rect.unwrap_or([0, 0, desc.Width as u32, desc.Height]).map(|v| v as f32);
                let constants = [l / w, t / h, (r - l) / w, (b - t) / h];
                list.SetPipelineState(if i == 0 { &self.opaque } else { &self.over });
                list.SetGraphicsRoot32BitConstants(0, 4, constants.as_ptr().cast(), 0);
                list.SetGraphicsRootDescriptorTable(1, D3D12_GPU_DESCRIPTOR_HANDLE { ptr: gpu.ptr + (i * step) as u64 });
                list.DrawInstanced(3, 1, 0, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// The compositor compiles with the system's compiler and the host
    /// takes its pipelines, for the swap chain formats presented.
    #[test]
    fn compositor_builds() {
        let Some(host) = crate::host::host() else {
            return; // no host GPU here
        };
        for format in [
            super::DXGI_FORMAT_R8G8B8A8_UNORM,
            super::DXGI_FORMAT_B8G8R8A8_UNORM,
            super::DXGI_FORMAT_R10G10B10A2_UNORM,
            super::DXGI_FORMAT_R16G16B16A16_FLOAT,
        ] {
            assert!(super::Compositor::new(&host.device, format).is_ok(), "{format:?}");
        }
    }
}
