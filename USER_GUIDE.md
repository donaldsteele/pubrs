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

1. **New from template** (Ctrl+N / ⌘N) shows the templates: flyer, trifold
   brochure, business card and newsletter. Pick one, or a blank publication.
2. **Change the text:** double-click a text box and type. Ctrl+A selects
   all the text in the box. Press Esc when you are done.
3. **Add pictures:** double-click an empty picture frame (grey box with a
   cross) and choose a PNG, JPEG or SVG file. Use the properties panel on
   the right to switch between **Fill**, **Fit** and **Stretch** and to crop.
4. **Check for problems** (toolbar **Check**): pubrs warns about text that
   doesn't fit, pictures that will print blurry (below 150 dpi), missing
   fonts, things off the page and template placeholders you haven't filled.
5. **Export PDF** (Ctrl+E / ⌘E) makes the file for a print shop: fonts are
   embedded and the bleed is included. **Print** (Ctrl+P / ⌘P) prints the
   same PDF.
6. **Save** (Ctrl+S / ⌘S) writes a `.pubrs` file.

You can also drag a `.pubrs` file onto the window to open it, or drag a picture
onto the window to place it in the selected picture frame.

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

### Keyboard shortcuts

| Action | Windows / Linux | macOS |
|---|---|---|
| New from template / blank | Ctrl+N / Ctrl+Shift+N | ⌘N / ⇧⌘N |
| Open, Save, Save As | Ctrl+O, Ctrl+S, Ctrl+Shift+S | ⌘O, ⌘S, ⇧⌘S |
| Export PDF, Print | Ctrl+E, Ctrl+P | ⌘E, ⌘P |
| Undo, Redo | Ctrl+Z, Ctrl+Shift+Z | ⌘Z, ⇧⌘Z |
| Cut, Copy, Paste, Duplicate | Ctrl+X/C/V, Ctrl+D | ⌘X/C/V, ⌘D |
| Group, Ungroup | Ctrl+G, Ctrl+Shift+G | ⌘G, ⇧⌘G |
| Bring forward / to front | Ctrl+], Ctrl+Shift+] | ⌘], ⇧⌘] |
| Send backward / to back | Ctrl+[, Ctrl+Shift+[ | ⌘[, ⇧⌘[ |
| Bold, Italic, Underline | Ctrl+B/I/U | ⌘B/I/U |
| Zoom in / out / fit / 100% | Ctrl+= / Ctrl+- / Ctrl+0 / Ctrl+1 | ⌘= / ⌘- / ⌘0 / ⌘1 |
| Next / previous page | Page Down / Page Up | Page Down / Page Up |
| Lock / unlock | Ctrl+L | ⌘L |

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
pubrs validate brochure.pubrs [--json]        # exit code 1 if there are problems
pubrs script steps.json --out out/            # replay MCP-style tool calls
pubrs tools                                   # list the tools
```

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

To measure, start pubrs from a terminal with `PUBRS_PERF=1` set (Windows:
`set PUBRS_PERF=1` then `"C:\Program Files\pubrs\bin\pubrs-app.exe"`). It
then prints frames per second and drawing times once a second.
