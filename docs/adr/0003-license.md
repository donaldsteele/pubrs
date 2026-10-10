# 0003 Project licence

- Context: installers (cargo-wix, cargo-bundle, cargo-deb) need a licence and the spec does not name one.
- Decision: MIT for pubrs code (placeholder for the owner to change); bundled Liberation fonts stay SIL OFL 1.1 with their licence text shipped.
- Consequence: changing the licence later is a one-line edit in the workspace Cargo.toml plus LICENSE.
