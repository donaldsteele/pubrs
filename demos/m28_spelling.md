# M28: Spell check

What works:

- **Engine** (`crates/pubrs-io/src/spell.rs`): `spellbook` 0.4.2 (MPL-2.0, Hunspell-compatible) with the SCOWL
  en_US Hunspell dictionary 2026.02.25 (size 60) compiled in with `include_str!` (`assets/dict/`, credited in
  ASSETS.md). The release `pubrs` CLI grows by 1.06 MB (25.34 to 26.40 MB unstripped), 0.51 MB of it the
  dictionary text. It is parsed once per process on first use, on the checking thread (about 7 ms in a release
  build), and shared by `Arc`. Dictionaries hash with a fixed seed, so suggestions come out in the same order in
  every run (MCP.md is stable).
- **More languages**: Hunspell `.dic`/`.aff` pairs in `<config dir>/pubrs/dictionaries/`, keyed by file stem
  (`de_DE`), parsed on first use; UTF-8 or ISO-8859-1. Added words go to `user-words.txt` there (written
  atomically, merged with words another window added meanwhile) and count in every language, capitalised and
  all-capitals forms included. Ignored words last for the session.
- **Languages per paragraph**: each paragraph is checked in its own `language` (BCP 47). `en`, `en-US`, `en_US`
  and `en-Latn-US` use en_US; a tag with a region uses `ll_RR`, then a bare `ll` dictionary; a tag without one
  uses the language's usual dictionary or the first `ll_*`. `en-GB` is not checked with en_US. A paragraph in a
  language without a dictionary is skipped, never flagged, and reported in `unchecked_languages`.
- **Words** (Unicode word boundaries, UAX #29): an apostrophe between letters stays in the word ("don't",
  "O'Neil", "can’t", checked with an ASCII apostrophe and reported as written); hyphenated words are checked part
  by part. Skipped: one-letter words, words with digits ("3rd", "A4"), words joined by other punctuation ("e.g",
  "example.com", "snake_case"), web and e-mail addresses (a chunk with "://", "@", or starting "www."), any chunk
  with a merge field (U+FFFC, M24), and words in capitals only (an option, on by default). White space and control
  characters, including the column, frame and page break characters, separate words.
- **Incremental**: results are cached per paragraph by (text, dictionary); after an edit only the changed paragraph
  is checked again (`CacheStats`, tested).
- **MCP**: `spell_check` (whole document in reading order, or one story; `{story_id, frame_id, start, end, word,
  language}` plus `unchecked_languages` and `dictionaries`), `spell_suggest` (word and language, best first, with
  `correct`), `spell_add_word`. `SessionConfig.dictionaries_dir` is new and optional: `None` keeps added words in
  memory; the app and the `pubrs` CLI pass `<config dir>/pubrs/dictionaries`.
- **GUI**: a spell worker thread (newest request wins, panics caught and reported, restarted if it dies, ignored
  words replayed) checks the page snapshot the canvas already builds; the UI thread never checks a word. Red wavy
  underlines (theme error colour, whole device pixels, constant screen size at any zoom) are drawn under misspelt
  words in the frames on the page, line by line, and only while the worker's result is for the story text on
  screen (`Arc` identity), so stale offsets are never drawn. Right-click a flagged word (or Shift+F10 at the caret)
  for up to six suggestions (one undo step each), Add to Dictionary and Ignore. Review → Spelling… (F7) opens a
  window that walks the misspelt words from the caret in reading order, selecting each in the text: Change (Enter;
  Up/Down choose), Change All (one undo step), Ignore, Add, Next, Close (Esc). Help → About shows the dictionary's
  copyright and licence.
- **Speed** (LD3): `pubrs perf` has a `keystroke.spell` row, the worker's work per keystroke (the edited story again,
  one paragraph missing the cache). Release build, development machine: 0.07 ms at 50 pages and 0.10 ms at 200
  pages, one paragraph checked per keystroke; enforced at 2 ms for both page counts.

See it:

```
cargo run -p pubrs-app
xvfb-run -a -s "-screen 0 1600x1000x24" cargo run -p pubrs-app -- --self-test target/gui-selftest
cargo test -p pubrs-io --test spell
cargo run --release -p pubrs-cli -- perf --pages 50 --runs 5 --check
```

The self-test (steps 91 to 105) types " Delicous" into a new text frame one key at a time, waits for the worker and
asserts the squiggle is drawn, right-clicks the word, picks the first suggestion ("Delicious") and checks the text
changed in one undo step, undoes it, presses F7, checks the window shows and selects the word, presses Enter to
change it and Esc to close. Screenshots 22 to 25 in `target/gui-selftest/` show the squiggle, the menu and the
window.

Try the tools: the call shapes are in `docs/mcp-examples.json` (`spell_check`, `spell_suggest`, `spell_add_word`)
and in MCP.md.

Known limits (TODO.md, Spelling): only en_US is built in; `pubrs-mcp` passes no dictionaries folder yet; spellbook
has no phonetic suggestions; the word being typed is flagged at once; master-page text has no squiggles.
