# 0005 Ids are allocated before commands are built

- Context: replaying a command log must produce identical documents, and `pubrs-core` must build without an RNG.
- Decision: commands carry the ids of objects they create; callers allocate them from an `IdSource` (`RandomIds` with the default feature, deterministic `SeqIds` for tests and templates).
- Consequence: `apply` is a pure function of (document, command); ids are stable across undo/redo.
