//! Which adapter egui draws the window with.
//!
//! wgpu's own choice looks only at the power preference within the enabled
//! backends, so a machine whose GPU wgpu reaches only through OpenGL (older
//! Intel GPUs its DX12 backend rejects, virtual machines with a GL driver)
//! ends up on the software rasterizer of DX12 or Vulkan. On WARP that costs
//! about half a CPU-second per frame (docs/adr/0011-gui-adapter.md), so pubrs
//! enumerates every backend and takes hardware first.

use eframe::wgpu;

/// Renderer names of software rasterizers that wgpu does not report as
/// [`wgpu::DeviceType::Cpu`], e.g. OpenGL-on-D3D12 running on WARP.
const SOFTWARE_NAMES: &[&str] = &[
    "basic render driver",
    "llvmpipe",
    "lavapipe",
    "softpipe",
    "swiftshader",
    "gdi generic",
];

/// True for adapters that rasterise on the CPU.
pub fn is_software(info: &wgpu::AdapterInfo) -> bool {
    info.device_type == wgpu::DeviceType::Cpu || is_software_name(&info.name)
}

/// True for adapter or OpenGL renderer names of known software rasterizers.
pub fn is_software_name(name: &str) -> bool {
    let name = name.to_lowercase();
    SOFTWARE_NAMES.iter().any(|s| name.contains(s))
}

/// Sort key, lower is better: hardware before software, then Vulkan, Metal
/// and DX12 before OpenGL, then discrete, integrated, virtual, unknown.
fn rank(info: &wgpu::AdapterInfo) -> (bool, bool, u8) {
    let class = match info.device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
    };
    (is_software(info), info.backend == wgpu::Backend::Gl, class)
}

/// The best adapter that can present to `surface`.
pub fn select(
    adapters: &[wgpu::Adapter],
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    for a in adapters {
        let i = a.get_info();
        tracing::debug!(name = %i.name, api = ?i.backend, kind = ?i.device_type, "adapter found");
    }
    adapters
        .iter()
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .min_by_key(|a| rank(&a.get_info()))
        .cloned()
        .ok_or_else(|| {
            format!(
                "none of the {} adapters can draw this window",
                adapters.len()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(
        name: &str,
        backend: wgpu::Backend,
        device_type: wgpu::DeviceType,
    ) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: name.into(),
            vendor: 0,
            device: 0,
            device_type,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: None,
            limit_bucket: None,
        }
    }

    fn best(list: &[wgpu::AdapterInfo]) -> wgpu::AdapterInfo {
        list.iter().min_by_key(|i| rank(i)).unwrap().clone()
    }

    #[test]
    fn hardware_opengl_beats_warp() {
        use wgpu::{Backend::*, DeviceType::*};
        let found = [
            info("Microsoft Basic Render Driver", Dx12, Cpu),
            info("Intel(R) HD Graphics 4600", Gl, IntegratedGpu),
        ];
        assert_eq!(best(&found).name, "Intel(R) HD Graphics 4600");
    }

    #[test]
    fn native_api_beats_opengl_on_the_same_gpu() {
        use wgpu::{Backend::*, DeviceType::*};
        let found = [
            info("NVIDIA GeForce RTX 3060", Gl, Other),
            info("NVIDIA GeForce RTX 3060", Vulkan, DiscreteGpu),
            info("Microsoft Basic Render Driver", Dx12, Cpu),
        ];
        assert_eq!(best(&found).backend, Vulkan);
    }

    #[test]
    fn opengl_on_warp_counts_as_software() {
        use wgpu::{Backend::*, DeviceType::*};
        let glon12 = info("D3D12 (Microsoft Basic Render Driver)", Gl, Other);
        assert!(is_software(&glon12));
        let found = [
            glon12.clone(),
            info("Microsoft Basic Render Driver", Dx12, Cpu),
        ];
        let b = found.iter().min_by_key(|i| rank(i)).unwrap();
        assert_eq!(
            b.backend, Dx12,
            "between two software paths, prefer the native API"
        );
    }

    #[test]
    fn discrete_beats_integrated() {
        use wgpu::{Backend::*, DeviceType::*};
        let found = [
            info("Intel(R) UHD Graphics 630", Dx12, IntegratedGpu),
            info("AMD Radeon RX 6600", Dx12, DiscreteGpu),
        ];
        assert_eq!(best(&found).name, "AMD Radeon RX 6600");
    }

    #[test]
    fn virtual_machine_gl_driver_beats_warp() {
        use wgpu::{Backend::*, DeviceType::*};
        let found = [
            info("Microsoft Basic Render Driver", Dx12, Cpu),
            info("SVGA3D; build: RELEASE; LLVM;", Gl, Other),
        ];
        assert_eq!(best(&found).name, "SVGA3D; build: RELEASE; LLVM;");
    }
}
