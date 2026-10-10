# 0004 GUI renderer fallback

- Context: CI machines, VMs and remote desktops may have no GPU adapter.
- Decision: eframe starts with its wgpu renderer and, if that fails, retries with glow (OpenGL) in the same process (eframe reuses its event loop); the active path is logged.
- Consequence: the GUI launches wherever either backend works; document rendering has its own tiny-skia fallback (render crate).
