# 0004 GUI renderer fallback

- Context: CI machines, VMs and remote desktops may have no GPU adapter.
- Decision: eframe starts with its wgpu renderer and, if that fails, retries with glow (OpenGL) in the same process (eframe reuses its event loop); the active path is logged.
- Consequence: the GUI launches wherever either backend works; document rendering has its own tiny-skia fallback (render crate).
- Addendum (M8): page rendering uses vello only on hardware adapters. On software adapters (WARP, llvmpipe, SwiftShader) it uses tiny-skia, which is faster there and is the reference renderer. vello crashed the process with an access violation on WARP (Windows CI runner without a GPU), which no fallback can catch.
