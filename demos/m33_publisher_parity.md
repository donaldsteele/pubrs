# M33: publisher parity, integration and release readiness (0.2.0)

The plan (M9 to M33) took pubrs from a four-template flyer maker to a small desktop publisher: faster (incremental
layout, tiled off-thread drawing), more robust (rollback, property tests), and with the features people expect
from Publisher: hyphenation and justification limits, keeps and drop caps, tab stops and lists, character formatting,
tight wrap, tables, WordArt and shadows, swatches with CMYK and spot colour, PDF/X-4, label sheets and booklets,
mail merge, spelling, a design checker, clipboard and input-method support, `.pub` import and thirteen templates.
M33 checks the whole against the Definition of Done and prepares the release.

Definition of Done, and where each item is shown:

1. **Newsletter in the GUI, close, reopen, identical.** The last steps of the GUI self-test (steps 235 to 265, "DoD1:"
   in the log):

   ```
   xvfb-run -a -s "-screen 0 1600x1000x24" cargo run -p pubrs-app -- --self-test target/gui-selftest
   ```

   New from the Newsletter template; Paragraph dialog: Hyphenate on (the lead story is justified; 2 lines end in a
   hyphen); Drop Cap on the second story; Bullets on the sidebar; Cmd+F, Cmd+H, Replace All (3 matches); Insert >
   Table and typing in a cell; F7 stepping through the spelling issues; Export PDF with the Commercial preset (the
   checker passes); save, close, reopen: identical JSON and byte-identical page renders. Screenshots are in
   `target/gui-selftest/`.
2. **30 labels on a 5160 sheet from CSV and from MCP.**

   ```
   cargo test -p pubrs-api --test templates address_labels
   cargo test -p pubrs-cli --test cli merge_address
   cargo test -p pubrs-mcp --test acceptance address_labels
   ```

3. **An 8-page booklet to PDF.**

   ```
   cargo test -p pubrs-io --test pdf_export eight_page_booklet
   ```

   Four sheet sides of 1224 x 792 pt, pages 8|1, 2|7, 6|3, 4|5, and the PDF/X-4 checker passes.
4. **Import a corpus `.pub` and export it.**

   ```
   cargo run -p pubrs-cli -- import tests/corpus/pub/poi/SampleNewsletter.pub -o target/newsletter.pubrs
   cargo run -p pubrs-cli -- export target/newsletter.pubrs -o target/newsletter.pdf
   cargo test -p pubrs-io --test pubfile
   ```

5. **Performance.**

   ```
   cargo run --release -p pubrs-cli -- perf --pages 200 --runs 7 --check
   cargo run --release -p pubrs-cli -- perf --pages 50 --runs 5 --check     # the CI command
   ```

   On the development machine (200 pages): keystroke total 9.8 ms against a 30 ms budget, relayout 8.8 ms (20), viewport
   render 7.7 ms (16), PDF export of all pages 0.8 s.
6. **CI green on all four OSes, goldens byte-identical.** Checked by the orchestrator after the merge; no golden file
   changed in M33.
7. **The release workflow builds every installer from a manual dispatch on the branch.** Checked by the orchestrator
   after the merge (`gh workflow run release.yml --ref plan/publisher-parity`); RELEASE.md says how and what to
   check by hand (labels, booklets, PDF/X-4 in a preflight tool).

What changed in M33: the end-to-end self-test flow; an MCP test for the label merge and a PDF test for the booklet; the
version is 0.2.0 everywhere (workspace, macOS bundle, `Cargo.lock`, AppStream); USER_GUIDE.md, ARCHITECTURE.md (with an
index of the 28 ADRs), FILE_FORMAT.md, DEPENDENCIES.md (with a licence check), ASSETS.md (sheet presets and which ones
need checking against datasheets), RELEASE.md (print checks and the 0.2.0 notes) and MCP.md are current; TODO.md lists
only what is left; the MCP.md "Frames" section is no longer split in two (`frame_copy_to_doc` moved up in the registry).

Next: merge `plan/publisher-parity` into `main`, then `git tag v0.2.0 && git push origin v0.2.0`.
