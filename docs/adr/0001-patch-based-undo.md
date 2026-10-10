# 0001 Patch-based undo

- Context: every command must be invertible; hand-writing an inverse for ~50 commands is error-prone.
- Decision: the Editor snapshots, applies, and diffs before/after into forward/backward `Patch`es (themselves a serializable `patch` command); undo/redo apply patches, never re-run command logic.
- Consequence: inverses are exact and atomic by construction; cost is one document clone + diff per command (sub-millisecond at publication sizes).
