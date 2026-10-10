# 0002 macOS Intel CI runner

- Context: the spec names `macos-13` for Intel macOS, but GitHub retired that image in late 2025.
- Decision: the Intel job runs on `macos-15-intel`, GitHub's replacement Intel image.
- Consequence: Intel coverage stays until GitHub drops Intel images; revisit then.
