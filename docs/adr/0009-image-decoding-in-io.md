# 0009 Image decoding lives in pubrs-io

- Context: the spec puts image decode/SVG in `pubrs-io`, but `pubrs-render` (below io) must draw images.
- Decision: render defines `ImageContent` (decoded raster + original bytes, or a usvg tree) and an `ImageResolver` trait; io's `ImageCache` decodes assets (EXIF orientation applied) and implements it.
- Consequence: render stays free of codecs; the PDF writer can pass original JPEG bytes through when no rotation was applied.
