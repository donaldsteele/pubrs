# 0008 Golden images compare with a tolerance

- Context: goldens should be byte-identical across OSes, but tiny-skia's SIMD paths are chosen per CPU (enabled transitively by resvg/krilla-svg), so x86_64 and aarch64 can differ by a few anti-aliased pixels.
- Decision: goldens are rendered by tiny-skia from `tests/fonts/` only; tests pass if ≤0.2% of pixels differ by more than 8/255 per channel and log whether the PNG is byte-identical; JPEG (decoder SIMD) is kept out of goldens.
- Consequence: real regressions fail; CI logs show exact-match status per OS for the Definition of Done.
