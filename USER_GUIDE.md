# pubrs user guide

pubrs makes print documents: flyers, brochures, business cards, newsletters
and other small print jobs. Start from a template, type your text, add
pictures, and export a print-ready PDF.

## Installing

### Windows 10/11 (x64 or ARM)

1. Download `pubrs-<version>-x86_64-pc-windows-msvc.msi` (or the
   `aarch64` one for ARM PCs).
2. Double-click it and follow the installer. If Windows SmartScreen warns
   about an unknown publisher (unsigned builds), choose **More info → Run anyway**.
3. Start **pubrs** from the Start menu. `.pubrs` files open in pubrs when
   double-clicked.

The installer puts everything in `C:\Program Files\pubrs`, adds its `bin`
folder to `PATH` (so `pubrs` and `pubrs-mcp` work in a terminal) and
installs a sample in `C:\Program Files\pubrs\samples`. Uninstall from
**Settings → Apps**.

### macOS 12 or later (Apple Silicon and Intel)

1. Download `pubrs-<version>-macos-universal.dmg` and open it.
2. Drag **pubrs** into **Applications**.
3. The first time, Control-click pubrs in Applications and choose **Open**
   (needed for builds that are not notarized).

To use the command-line tools from a terminal:
`/Applications/pubrs.app/Contents/MacOS/pubrs` and `.../pubrs-mcp`.
A sample is in `pubrs.app/Contents/Resources/samples`. Uninstall by
dragging pubrs to the Bin.

On macOS, double-clicking a `.pubrs` file starts pubrs but does not open it yet:
use **File → Open**, or drag the file onto the pubrs window.

### Linux (x64 or ARM; X11 and Wayland)

- **Debian/Ubuntu (.deb):** `sudo apt install ./pubrs_<version>_amd64.deb`,
  then start pubrs from your applications menu. The sample is in
  `/usr/share/doc/pubrs/examples`. Uninstall with `sudo apt remove pubrs`.
- **AppImage (any distribution):** `chmod +x pubrs-<version>-x86_64.AppImage`
  and run it. `pubrs-<version>-x86_64.AppImage --cli …` runs the command-line
  tool and `--mcp` the MCP server.
- **Flatpak:** build from `packaging/flatpak/` with `flatpak-builder` (see
  the comments in the manifest).

Printing on Linux uses CUPS; install it if you want to print.

## Getting started

1. **New from template** (Ctrl+N / ⌘N) shows the template gallery. Pick one,
   or a blank publication. Every template uses one of the built-in colour
   schemes; **Colour scheme…** in the Swatches panel recolours it in one
   step. The templates:
   - **Flyer**: Letter flyer with a colour header, photo and call to action.
   - **Trifold brochure**: two-sided, three-panel brochure.
   - **Business card**: one 3.5 x 2 in card with bleed.
   - **Newsletter**: four Letter pages with a masthead, a lead story that
     flows from page 1 to page 2, photos with captions, a pull quote, an
     events table, bulleted and numbered lists, an opening-hours table and
     a mailing panel on the back.
   - **Greeting card**: a quarter-fold card on one Letter page. The bottom
     half is the back (left) and front (right). The inside spread is in the
     top half, printed upside down on purpose so it reads correctly once
     folded. Fold guides mark the centre lines.
   - **Postcard**: 6 x 4 in, picture front and a back with message,
     address block and stamp box.
   - **Certificate**: Letter landscape with a decorative border, title,
     recipient, date and signature lines, and a seal.
   - **Restaurant menu**: two pages, prices lined up on a tab with dot
     leaders, and a bulleted list of specials.
   - **Letterhead**: logo, name and address, a thin rule, an empty body for
     your letter and a footer.
   - **#10 envelope**: return address at top left, recipient block just
     right of centre.
   - **Annual calendar**: twelve Letter landscape pages, one per month,
     with a picture and a table of days (weeks start on Sunday; the
     neighbouring months' days are grey). It opens on the current year; in
     scripts and through MCP the `year` option picks 1600 to 9999 (default
     2027).
   - **Address labels**: Avery 5160 (30 per sheet). The label holds the
     merge fields Name, Address, City, State and Zip: load a list with
     **Mailings → Mail Merge…** and each record becomes one label (see
     *Mail merge* below).
   - **Business cards (sheet)**: Avery 5371, ten cards per sheet.

   The nine templates after Newsletter (greeting card, postcard, certificate,
   menu, letterhead, envelope, calendar, address labels and business cards)
   are new in 0.2.0. A built-in template always wins over one of your own with
   the same name. If an earlier version saved `menu` or `postcard` in your
   templates folder, you get the built-in one and yours is hidden. The file is
   still there; save yours under another name to use it. `Save as
   Template…` refuses these names.
2. **Change the text:** double-click a text box and type. Ctrl+A selects
   all the text in the box. Press Esc when you are done.
3. **Add pictures:** double-click an empty picture frame (grey box with a
   cross) and choose a PNG, JPEG or SVG file. Use the properties panel on
   the right to switch between **Fill**, **Fit** and **Stretch** and to crop.
4. **Check for problems** (toolbar **Check**, or **Review → Design Checker**):
   a panel lists text that doesn't fit, pictures that will print blurry
   (below 150 dpi), missing fonts, text that is too small or too pale, things
   off the page or too near its edge, empty table cells, pictures without alt
   text and template placeholders you haven't filled. See *Design Checker*
   below.
5. **Export PDF** (Ctrl+E / ⌘E) makes the file for a print shop: fonts are
   embedded and the bleed is included. **Print** (Ctrl+P / ⌘P) prints the
   same PDF.
6. **Save** (Ctrl+S / ⌘S) writes a `.pubrs` file.

You can also drag a `.pubrs` file onto the window to open it, or drag a picture
onto the window to place it in the selected picture frame.

### Importing Microsoft Publisher files

**File → Import Publisher File…** (or dragging a `.pub` file onto the window)
turns a Publisher 98, 2000, 2002 or later file into a new, unsaved publication.
From Publisher 2002 and later files, pages, text boxes with their formatting
and links, pictures (PNG, JPEG and GIF), rectangles, ellipses and lines with
their colours, tables, the colour scheme (as swatches) and the master page come
across. From Publisher 98 and 2000 files, pages, text boxes with their
formatting and links, and the colour scheme come across; their pictures and
shapes are not imported yet. A report then lists everything that did not: pictures drawn as Windows metafiles are left as empty picture
frames, shapes of other kinds are skipped, and fonts you do not have are drawn
with Liberation Sans. Save the result with **Save** to keep it as a `.pubrs`
file. On the command line: `pubrs import old.pub -o new.pubrs`.

### Working with the page

- Click to select, Shift+click to add to the selection, drag on empty space
  to select several items.
- Drag items to move them (they snap to margins, guides and other items; hold
  Alt to move freely). Drag the square handles to resize and the round handle
  to rotate (hold Shift for 15° steps). Arrow keys nudge by 1 pt
  (Shift: 10 pt).
- Use the toolbar tools (Text, Picture, Rectangle, Ellipse, Line) and drag on
  the page to add items.
- The properties panel sets exact position and size (inches, millimetres or
  points), fill and line colours, opacity, text wrap, fonts, alignment and
  spacing. With nothing selected it shows the page size, margins and bleed.
- **Text that doesn't fit** shows a red **+** at the bottom-right of the box.
  Select the box, Shift+click an empty text box, and choose
  **Text → Link to Next Text Box** so the text continues there.
- The **Pages** panel on the left adds, deletes and reorders pages.

### Rulers and guides

- **View → Rulers** shows a ruler along the top and left of the page, in the
  units chosen in the properties panel (inches, millimetres or points). Zero is
  the page's top-left corner. Tick spacing follows the zoom, and a blue band on
  each ruler marks the selected items. The setting is remembered.
- **Make a guide** by dragging from the top ruler (a horizontal guide) or the
  left ruler (a vertical one) onto the page. A preview follows the pointer.
- **Move a guide** by dragging it. **Delete a guide** by dragging it off the
  page or back onto a ruler; it turns red while it will be deleted. Press Esc
  during a drag to cancel. Each of these is one undo step (Ctrl+Z / ⌘Z).
- Items snap to guides when **View → Snap to Guides** is on. **View → Show
  Guides** hides or shows guides. Guides that come from a master page are drawn
  on the pages using it and can't be dragged there.

### Find and replace

| Action | Windows / Linux | macOS |
|---|---|---|
| Find (opens the find bar) | Ctrl+F | ⌘F |
| Next / previous match | Enter / Shift+Enter in the bar, F3 / Shift+F3 | Enter / Shift+Enter, F3 / ⇧F3 |
| Find and Replace window | Ctrl+H | ⌘H |
| Close the bar or window | Esc | Esc |

- Type in the find bar. The count shows how many matches there are in the whole
  publication. **Match case** and **Whole word** narrow the search. Next and
  Previous go to the match's page, scroll to it and select it, ready to retype.
- If text is selected when you press Ctrl+F or Ctrl+H, it becomes the search.
- In the **Find and Replace** window, **Replace** swaps the selected match and
  moves to the next; **Replace All** changes every match in one step, so one
  undo restores them all. **Selection only** limits it to the text that was
  selected when you ticked it. Buttons that can't be used say why when you point
  at them.
- Matches never cross a paragraph break. Text hidden by overflow is searched
  and replaced, but can't be selected until it fits in a box.

### Spelling

- Misspelt words get a red wavy underline as you type. Right-click one (or put
  the cursor in it and press Shift+F10) for suggestions: pick one to replace
  the word (one undo step), **Add to Dictionary** to accept it from now on, or
  **Ignore** to accept it until you quit.
- **Review → Spelling…** (F7) goes through the misspelt words from the cursor
  on, selecting each in the text. **Change** uses the highlighted suggestion
  (Up/Down choose, Enter changes), **Change All** fixes every copy in one step,
  **Ignore**, **Add** and **Next** skip it, Esc closes.
- Each paragraph is checked in its own language (Text → Paragraph… sets it).
  American English is built in. For other languages, put a Hunspell dictionary
  pair (for example `de_DE.dic` and `de_DE.aff`, as used by LibreOffice) in the
  `dictionaries` folder of the pubrs settings folder: `%APPDATA%\pubrs` on
  Windows, `~/Library/Application Support/pubrs` on macOS, `~/.config/pubrs` on
  Linux. Paragraphs in a language without a dictionary are not checked.
- Words you add are kept in `user-words.txt` in that folder, one per line.
- Words with digits, web and e-mail addresses, and words in capitals only
  (acronyms) are not checked.

### Design Checker

- **Review → Design Checker** (also the toolbar **Check** button and
  **View → Check for Problems…**) shows a panel under the page. It lists what
  could go wrong in print, grouped by page (**Page 3 (2)**, then
  **Publication** for things that belong to no page). Each issue says what is
  wrong and how to fix it. **Click an issue** to go to its page and select the
  object. **Close** hides the panel; choosing the menu item again does too.
- The list updates by itself: about half a second after you stop typing or
  moving things, only the pages you changed are checked again. It does no work
  while the panel is closed.
- What it looks for. *Problems*: text that doesn't fit its frame; a font that
  isn't installed; characters no font has (named in the message); pictures
  under 150 dpi; objects off the page or past the bleed; RGB colour in a
  commercial job. *Worth a look*: text under 6 pt; text with a contrast ratio
  below 3:1 against its background (a frame's own fill, or the fill of a
  frame underneath it; pale text over a picture is not judged); objects within
  0.25 in of the page edge, which many desktop printers cannot print (not for
  publications with bleed, label sheets or commercial jobs); table cells with
  nothing in them; pictures without alt text (Alt text in the properties
  panel); unfilled template placeholders; merge fields that name a column the
  loaded list does not have.
- **Commercial printing checks** (a box in the panel, off by default) also
  flags every frame that uses an RGB colour (a plain colour, an RGB swatch, a
  gradient stop, text colour; pure black text is not counted) in a job meant
  for a CMYK press, and stops warning about the desktop margin. RGB colour is
  reported only with this box ticked, with `pubrs validate --commercial`, or at
  a commercial export; otherwise it is never mentioned. Use CMYK or
  spot swatches (**Design → Swatches**) instead. Pictures are not judged.
- **Before a commercial export.** With **Commercial printing (PDF/X-4)** chosen,
  **Export…** and **Save for a commercial printer…** first check for missing
  fonts, RGB colour and low-resolution pictures. If there are any they are
  listed with three buttons: **Export anyway**, **Fix first** (opens the
  panel with the commercial checks on and selects the first problem) and
  **Cancel**. The other presets never stop you.
- Not checked yet: whether a frame is hidden behind a picture, and text on a
  picture.

### Tables

- **Insert → Table…** opens a grid: point at it and click to choose the rows
  and columns (or type them; up to 200 rows and 40 columns) and press
  **Insert**. The table appears centred on the page, about two thirds of the
  margin width, with 24 pt rows and thin black borders, and its first cell is
  ready for typing. Inserting is one undo step.
- **Typing.** Click a table once to select it, then click a cell (or
  double-click a cell at any time) to type in it. **Tab** moves to the next
  cell and **Shift+Tab** to the previous one, with the cursor at the end of
  its text. **Tab in the last cell adds a row** (one undo step). **Control+Tab**
  types a tab character (the Control key on macOS too, not Command). Typing in a cell is undone like other text: five
  characters typed in a row are one step. A row grows with its text, and the
  table's frame grows with the rows.
- **Selecting cells.** With the table selected, drag from one cell to another
  to select a rectangle of cells (shown in blue; merged cells are always
  included whole), or Shift+click a second cell while editing. **Esc** clears
  the range. Hold **Alt** while dragging to move a table that is already
  selected.
- **Resizing.** With the table selected or a cell being edited, point at a
  column border or the bottom of a row: the pointer changes, and dragging moves
  the border (a line shows where it will land). Dragging a column border
  changes the width of the column on its left, so the table gets wider or
  narrower; dragging a row border sets that row's minimum height. Each drag is
  one undo step; columns and rows stay at least 6 pt.
- **Right-click** in a table for **Insert Row Above/Below**, **Insert Column
  Left/Right**, **Delete Row(s)** and **Delete Column(s)** (for the selected
  cells, or the cell you clicked), **Merge Cells** (when more than one cell is
  selected), **Split Cell** (for a merged cell) and **Cell Properties…**. A
  table always keeps at least one row and one column. On a misspelt word the
  spelling suggestions come first. Every item is one undo step.
- **Cell properties.** The properties panel has a Cell section for the cell
  being edited, the selected cells, or every cell when only the table is
  selected: **Fill**, **Borders** (All, Top, Right, Bottom or Left, with colour,
  width and None), **Vertical** alignment, **Insets** (one value, or each
  side), and the width of the current column and the minimum height of the
  current row. A change to several cells is one undo step.

### Text: hyphenation, justification and paragraph controls

**Text → Paragraph…** (Ctrl+Alt+P / ⌥⌘P) works on the paragraph at the cursor,
the selected paragraphs, or every paragraph of a selected text box. It has two
tabs and one OK for the whole dialog (one undo step).

- **Indents and Spacing** sets left, right and first-line indent, space before
  and after, and line spacing. Below them:
  - **Hyphenate** breaks long words at the end of a line with a hyphen. It is
    off until you turn it on, templates included. **Language** picks the
    pattern: English (US), German, Spanish, French, Polish or Portuguese. Other
    languages are not hyphenated, and the dialog says so.
  - **Limit word and letter spacing** sets how far a justified line may
    stretch before it is left short instead. **Word space up to** defaults to
    133% of a normal space and **Letter spacing up to** to 5% of the font size.
    The built-in templates have it ticked; unticked, justified lines stretch as
    far as needed. Justify itself is the **Justify** button in the Text section
    of the properties panel.
- **Keeps** controls how a paragraph breaks across columns, linked frames and pages:
  **Keep with next** (the last line stays with the next paragraph), **Keep
  lines together** (the paragraph is not split if it fits in one column), and
  **Widow and orphan control** with a minimum of 2 to 10 lines. A paragraph
  that contains a column, frame or page break is not kept.

**Text → Drop Cap…** sinks the first letters of a paragraph into the first
lines: tick **Drop cap** and choose how many **lines** it spans and how many
**characters** it takes. The dialog previews the paragraph's first words.

### Tab stops, leaders, bullets and numbering

- **Text → Tabs…** shows a ruler as wide as the text box. Click it to add a
  stop, drag a stop to move it (drag it off the ruler to delete it), and choose
  its alignment (left, centre, right or decimal) and **leader** (none, dots,
  dashes or a line) before adding. The position field gives an exact value in
  points. Stops are measured from the text edge. Without stops a tab jumps to
  the next 36 pt. A paragraph with a tab is set flush left whatever its
  alignment. The menu template uses a right tab with dot leaders for prices.
- **Text → Bullets and Numbering…** turns paragraphs into list items:
  **Bullets** (pick the character) or **Numbers** (1. 2. 3., a. b. c.,
  A. B. C., i. ii. iii. or I. II. III.), the nesting **Level**, the hanging
  **Indent**, and **Restart numbering** with **Start at**. **None** turns items
  back into paragraphs. In a list, **Tab** at the start of an item nests it
  one level and **Shift+Tab** brings it back.

### Character formatting

**Text → Character…** (Ctrl+Alt+C / ⌥⌘C) sets font, size, **Bold**,
**Italic**, **Underline**, **Strikethrough**, **Baseline** (normal,
superscript, subscript, or a **Shift** in points: positive raises), **Capitals**
(none, small caps or all caps; the text is stored as typed) and **Ligatures**
and **Kerning** (both on by default). **Superscript** and **Subscript** also
have shortcuts (Ctrl+Shift+= and Ctrl+=; ⇧⌘= and ⌘= on macOS) and buttons in
the text panel, which also has colour, size, alignment and spacing. With
several cells or a whole table selected, the Text and character commands are
unavailable; place the cursor in one cell.

### Colour

- **Design → Swatches** shows the Swatches panel. **+ RGB**, **+ CMYK** and
  **+ Spot** add a swatch (a spot colour has a name and a CMYK stand-in shown on
  screen). Use a swatch for a fill, line or text colour and change it once to
  change every place that uses it.
- **Design → Colour Scheme…** (or **Colour scheme…** in the panel) picks one of
  twelve built-in schemes with seven slots (Main, Accent 1 to 4, Hyperlink,
  Followed Hyperlink) and recolours everything that uses them in one step.
- A fill can be a **solid** colour or swatch with a **tint**, a **linear
  gradient** (angle) or a **radial gradient**. The panel edits the angle and
  the first and last colour.
- CMYK and spot swatches print as CMYK and as a separate ink in a commercial
  PDF; RGB colours print as RGB (see *Commercial printing* below).

### WordArt, shadows and BorderArt

- **Insert → WordArt…** opens a gallery of twelve styles. Type your text,
  choose a style and press **Insert**. The properties panel then edits the
  text, font, size, **Shape** (arch up or down, circle, wave, slant up or down,
  inflate), letter fill (solid or gradient), outline and shadow. WordArt text
  is edited in the panel, not on the page, and is not spell checked.
- **Format → Shadow** adds a soft drop shadow to the selected frames; the
  **Shadow** controls in the panel set offset, blur, colour and opacity. In a
  PDF the shadow is a 150 dpi picture.
- **Format → Border Art…** picks one of twelve decorative borders and its width
  for the selected frame, with previews. The border colours are fixed.

### Text wrap

Select a picture or shape and choose **Text wrap** in the properties panel:
**None**, **Square** (around the frame's box, on the wider side), **Tight**
(around the shape's outline, which for an ellipse follows the curve and for a
picture follows its non-transparent pixels), **Through** (like Tight, but the
text also fills notches and holes in the outline) or **Top and bottom** (no text
beside it). **Margin** sets the clearance in points. **Text on both sides**
fills every gap beside the frame that is wider than three em. Only frames in
front of the text frame on the same page make text wrap.

### Labels, cards and booklets

**File → Page Setup…** (Ctrl+Shift+P / ⇧⌘P) has a **Page** tab (it shows the
size, margins and bleed, which you change in the properties panel with nothing
selected) and a **Labels, Cards, Booklet** tab:

- **Labels or cards on a sheet.** Choose a **Sheet** from 26 presets: Avery US
  address, shipping, return-address, business-card, postcard, note-card and square labels
  (5160, 5161, 5162, 5163, 5164, 5167, 5371, 5376, 5377, 8160, 8163, 22805,
  5305, 8315), A4 labels and cards (L7160, L7161, L7162, L7163, L7173, L7651,
  C32011, J8160) and generic 2-up and 4-up Letter and A4 sheets. Each page of
  the publication is then one label, so tick the box that sets the page size to
  the label size. **Fill** is **Same page on every label** (design one label,
  it repeats across the sheet) or **One page per label** (each page fills the
  next label; used by mail merge). Export PDF and Print then place the labels
  on the sheet at its exact positions; the status bar says how many sheets
  there are. Avery and the other product numbers are trademarks of their
  owners, used only to name a sheet; pubrs is not affiliated with them. Check a
  test print against a plain sheet held to the light before using real labels.
- **Booklet (saddle stitch, 2-up).** The pages are padded to a multiple of 4 and
  imposed two to a sheet side in booklet order, so an 8-page booklet prints as
  four sides: 8|1, 2|7, 6|3 and 4|5. Print double-sided, flipping on the short
  edge, then fold the stack in half. Imposed output has no bleed, and crop
  marks mark the whole sheet. **Export PDF** writes the same imposed pages (the
  MCP `export_pdf` tool takes `impose: false` for the plain pages).

### Mail merge

Mail merge prints one copy of your design for each row of a list: letters,
postcards, name badges or address labels.

1. **Mailings → Mail Merge…** opens a four-step window. On **1 Data** type the
   path of a `.csv` or `.xlsx` file, or **Choose file…**, then **Load**. The
   first row names the fields. CSV must be UTF-8; the separator (comma,
   semicolon or tab) is detected automatically. For a workbook pick the **Sheet**
   (default the first). The data is stored in the publication, so it reopens
   without the file.
2. **2 Fields** lists each field with the first record's value. Click in a text
   box first, then **Insert at cursor** (or use **Insert → Merge Field…**,
   which picks a field with the arrow keys and Enter). A field sits in the text
   as one character and looks like «Name»; it takes the formatting of the text
   around it.
3. **3 Preview** shows the data in the text: tick **Show the data in the text**
   and step through the records with ◀ ▶ or the Left and Right arrow keys.
   A field with no value takes one neighbouring space with it, so a missing
   town does not leave a gap.
4. **4 Finish** makes the output: **Merge to PDF…**, **Merge to a new
   publication** (a new, unsaved tab with the values typed in) or **Print…**
   (the merged PDF goes to the printer). The Design Checker warns if a field
   names a column the list does not have.

**Mailings → Refresh Data** re-reads the file for the current session. On a
label sheet set to **One page per label**, 30 records fill the 30 labels of a
5160 sheet and record 31 starts the next sheet; on a **Same page on every
label** sheet each record gets a sheet of its own. Limits: files up to 64 MiB,
100,000 records, 512 fields, 5,000,000 cells and 20,000 output pages. A
field's value is one line (a line break in a cell becomes a space), so an
address that spans lines needs one field per line. Fields on a master page
merge in the PDF but stay «Name» in a merged publication. Copying text that
holds a field to another program gives plain text. On the command line:
`pubrs merge labels.pubrs --data list.csv -o labels.pdf`.

For address labels choose **File → New from Template… → Address labels**: it is
a 5160 sheet with Name, Address, City, State and Zip already in place.

### Commercial printing (PDF/X-4)

**File → Export PDF…** (Ctrl+E / ⌘E) asks what the PDF is for:

- **Screen**: for reading and sharing by e-mail; tagged for screen readers.
- **Desktop printing**: an ordinary PDF with fonts embedded, for an office or
  home printer. Bleed and crop marks are there to tick if you want them.
- **Commercial printing (PDF/X-4)**: PDF 1.6 with a CMYK output intent (a
  bundled generic CMYK, SWOP-like, profile), the bleed (if the publication has
  one), crop marks and embedded fonts, checked before it is written. CMYK colours are written as CMYK and spot
  colours as separate inks. Use CMYK or spot swatches: the check lists RGB
  colours (see *Design Checker*), and they are written with the profile's RGB
  equivalent.
- **Archive (PDF/A-2b)**: for long-term storage.

**Include bleed**, **Crop marks** and **Tagged PDF (accessibility)** (text as
paragraphs, pictures as figures with their alt text) can be changed after
choosing a preset (bleed and marks only for the two printing presets). **Save for a commercial printer…** writes one zip with the
PDF/X-4 file, the editable publication with its pictures, and a list of the
fonts and colours used: send that to the print shop. Give the bleed a value
first in the properties panel with nothing selected (0.125 in is common).
pubrs checks its own PDF/X-4 output for the markers and structure the standard
needs, but does not check trapping, overprint or total ink coverage: ask your
printer to run it through a preflight tool before the job. The same options are
in the `export_pdf` MCP tool (`standard: "pdfx4"`).

### Printing on each system

**Print…** (Ctrl+P / ⌘P) exports the PDF and sends it to a printer. It uses
what each system provides:

| | Windows | macOS | Linux |
|---|---|---|---|
| Dialog | The Windows print dialog | The macOS print panel | pubrs's own: choose the printer and copies |
| Scaling | Actual size, centred; shrunk only if the page is bigger than the printer's printable area | Page scaling is off: actual size | The PDF is sent as it is with `lpr`; if the page comes out smaller, turn off "fit to page" in the printer's options |
| Needs | A printer | A printer | CUPS (`lpr`, `lpstat`) |

For labels and booklets, print at **actual size** (no "fit to page") and, for
booklets, **double-sided, flip on the short edge**. Printers cannot print to the
very edge of the paper; the Design Checker mentions objects within 0.25 in of
it. On Windows a label sheet on a printer whose printable area is smaller than
the paper is shrunk slightly, which moves the labels: print the exported PDF
from a PDF viewer at 100% instead, and test on plain paper first.

### Notes for each system

- **Command and Control.** Everything written as Ctrl+X in this guide is ⌘X on
  macOS, and Alt is ⌥ (Option). In a table cell, **Control+Tab** types a tab
  character on macOS too (Control, not Command). Zoom in is Ctrl+Alt+= (⌥⌘=)
  because Ctrl+= and Ctrl+Shift+= are subscript and superscript.
- **Paste a picture.** On Windows and Linux, Ctrl+V pastes text when the
  clipboard holds text and does nothing when it holds only a picture: use
  **Edit → Paste** for a picture. On macOS ⌘V works for both.
- **Chinese, Japanese and Korean input.** The text you are composing is drawn
  in the window's own font, not your document's, so it may show boxes on a
  system without a CJK font until you confirm it; once confirmed it is set in
  your document's font like any other text. Input methods have been tested with
  synthetic input events only, not with a real input method on each system.
- **Opening files.** Windows and Linux open `.pubrs` and `.pub` files you
  double-click. On macOS use **File → Open** or drag the file onto the window.
- **Fonts.** Each system's own fonts are available; Liberation Sans and Serif
  are always there.

### Copying, pasting and international text

- **Copy and paste frames.** Copy puts the selected frames on the system clipboard. Paste puts them on the
  current page, in the same publication (nudged 18 pt) or another open one, with their text,
  linked frames, groups and pictures. Styles are matched by name (missing ones are added); colours from a swatch the
  other publication lacks are kept as plain colours. One undo step.
- **Paste from other programs.** A picture on the clipboard becomes a picture frame. One larger than 8,000 pixels a side
  is scaled down, and the status bar says so. Text pasted while editing keeps bold, italic, underline, size and
  colour from web pages and rich text; with no text being edited it goes into a new text frame. Pasting copied frames while editing text inserts their text. For a
  picture use Edit > Paste (on Windows and Linux Ctrl+V pastes text only).
- **Input methods.** Chinese, Japanese and Korean input works while editing text: the text you are composing shows
  at the cursor with an underline and the candidate window opens beside it; when you confirm it, it becomes part of the text, in one undo
  step with the typing around it.
- **Right-to-left text.** Hebrew and Arabic read right to left. The arrow keys move the cursor the way they point on
  the screen (in a Hebrew word, Right goes one letter back in the text), Home and End go to the start and end of the
  line (the right end, for a right-to-left line), and clicking and selecting follow the letters you see. Tab stops
  still count from the left edge.

### Keyboard shortcuts

| Action | Windows / Linux | macOS |
|---|---|---|
| New from template / blank | Ctrl+N / Ctrl+Shift+N | ⌘N / ⇧⌘N |
| Open, Save, Save As | Ctrl+O, Ctrl+S, Ctrl+Shift+S | ⌘O, ⌘S, ⇧⌘S |
| Export PDF, Print | Ctrl+E, Ctrl+P | ⌘E, ⌘P |
| Page Setup | Ctrl+Shift+P | ⇧⌘P |
| Page break, column break | Ctrl+Enter, Ctrl+Shift+Enter | ⌘↩, ⇧⌘↩ |
| Undo, Redo | Ctrl+Z, Ctrl+Shift+Z | ⌘Z, ⇧⌘Z |
| Cut, Copy, Paste, Duplicate | Ctrl+X/C/V, Ctrl+D | ⌘X/C/V, ⌘D |
| Group, Ungroup | Ctrl+G, Ctrl+Shift+G | ⌘G, ⇧⌘G |
| Bring forward / to front | Ctrl+], Ctrl+Shift+] | ⌘], ⇧⌘] |
| Send backward / to back | Ctrl+[, Ctrl+Shift+[ | ⌘[, ⇧⌘[ |
| Bold, Italic, Underline | Ctrl+B/I/U | ⌘B/I/U |
| Superscript, Subscript | Ctrl+Shift+=, Ctrl+= | ⇧⌘=, ⌘= |
| Character…, Paragraph… | Ctrl+Alt+C, Ctrl+Alt+P | ⌥⌘C, ⌥⌘P |
| Zoom in / out / fit / 100% | Ctrl+Alt+= / Ctrl+- / Ctrl+0 / Ctrl+1 | ⌥⌘= / ⌘- / ⌘0 / ⌘1 |
| Next / previous page | Page Down / Page Up | Page Down / Page Up |
| Lock / unlock | Ctrl+L | ⌘L |
| Find, Replace | Ctrl+F, Ctrl+H | ⌘F, ⌘H |
| Spelling, spelling menu at the cursor | F7, Shift+F10 | F7, Shift+F10 |

### Saving, autosave and recovery

pubrs keeps a recovery copy of unsaved work every minute. If it closes
unexpectedly, the next start offers to recover your publications. Recovery
copies, templates you save and settings live in:

| | Windows | macOS | Linux |
|---|---|---|---|
| Recovery copies | `%LOCALAPPDATA%\pubrs\recovery` | `~/Library/Application Support/pubrs/recovery` | `~/.local/share/pubrs/recovery` |
| Your templates | `%APPDATA%\pubrs\templates` | `~/Library/Application Support/pubrs/templates` | `~/.local/share/pubrs/templates` |
| Settings | `%APPDATA%\pubrs` | `~/Library/Application Support/pubrs` | `~/.config/pubrs` |

## Using pubrs with AI assistants

pubrs includes an MCP server, `pubrs-mcp`, so assistants such as Claude can
make publications for you. In Claude Desktop add this to
`claude_desktop_config.json` (**Help → Use with AI Assistants** shows it too):

```json
{ "mcpServers": { "pubrs": { "command": "pubrs-mcp", "args": [] } } }
```

Then ask, for example, *"Make me a trifold brochure for a bakery."* Files the
assistant saves go to `Documents/pubrs`. See `examples/mcp/README.md` and
`MCP.md` for details.

## Command line

```
pubrs export brochure.pubrs -o brochure.pdf [--crop-marks] [--no-bleed] [--outlines]
pubrs render brochure.pubrs --dpi 150 [--page 2] [-o out.png]
pubrs validate brochure.pubrs [--json] [--commercial]   # exit code 1 if there are problems
pubrs import old.pub -o new.pubrs [--json]    # import a Publisher file; prints what was dropped
pubrs merge labels.pubrs --data list.csv -o labels.pdf [--as publication] [--sheet Sheet1]
                                              # mail merge: one page set per record
pubrs perf --pages 200 [--runs 7] [--check]   # time editing and rendering on a synthetic publication
pubrs script steps.json --out out/            # replay MCP-style tool calls
pubrs tools                                   # list the tools
```

Add `--no-system-fonts` (before the command) to use only the bundled fonts, for
the same output on every computer. `pubrs merge` reads the list stored in the
publication when `--data` is left out. `pubrs perf --check` exits 1 if a
budget is exceeded.

`pubrs validate` exits 1 for any warning, including missing alt text and empty
table cells (a table you have just inserted counts as having empty cells).
`--commercial` also reports RGB colour.

## Fonts

pubrs always has Liberation Sans and Liberation Serif (same widths as Arial
and Times New Roman), and uses the fonts installed on your computer. If a
publication uses a font your computer doesn't have, pubrs substitutes a
bundled font and **Check** reports it.

## Troubleshooting

**pubrs is slow and uses a lot of CPU when you move the mouse.** Look at
the bottom-right corner of the window. **⚠ Drawing without a GPU** means no
graphics driver is available to pubrs, so the window is drawn on the CPU.
This is common in virtual machines and remote desktop sessions. Install the
graphics driver, or turn on 3D acceleration in the virtual machine's
settings. **Help → About** shows what draws the window ("Window drawing")
and the pages ("Page rendering").

On Windows (x64), the installer includes Mesa, a faster software renderer
that pubrs uses by itself when there is no graphics driver: About then
shows "llvmpipe" for window drawing. It still uses more CPU than a graphics
card. To turn it off, set `PUBRS_NO_MESA=1`.

To measure, start pubrs from a terminal with `PUBRS_PERF=1` set. It then
prints frames per second and drawing times once a second. On Windows, in a
Command Prompt: `set PUBRS_PERF=1`, then
`start /wait "" "C:\Program Files\pubrs\bin\pubrs-app.exe"`. pubrs opens no
console window of its own, and `start /wait` keeps the prompt from returning
while pubrs runs.
