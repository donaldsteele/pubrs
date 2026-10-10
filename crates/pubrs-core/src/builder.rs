//! Convenience builder that creates documents by issuing ordinary commands.
//! Used by templates and tests.

use crate::command::{CmdResult, Command};
use crate::geom::Rect;
use crate::history::Editor;
use crate::id::{FrameId, IdSource, PageId, StoryId};
use crate::model::*;

/// Builds a document through commands (so everything it makes is undoable
/// and valid).
pub struct DocBuilder<I: IdSource> {
    pub editor: Editor,
    pub ids: I,
}

impl<I: IdSource> DocBuilder<I> {
    pub fn new(mut ids: I, setup: PageSetup) -> Self {
        let doc = Document::new(&mut ids, setup);
        Self {
            editor: Editor::new(doc),
            ids,
        }
    }

    pub fn doc(&self) -> &Document {
        self.editor.doc()
    }

    pub fn finish(self) -> Document {
        self.editor.doc().clone()
    }

    pub fn exec(&mut self, c: Command) -> CmdResult<&mut Self> {
        self.editor.execute(c)?;
        Ok(self)
    }

    /// Id of page `index` (0-based).
    pub fn page(&self, index: usize) -> PageId {
        self.doc().pages[index].id
    }

    pub fn add_page(&mut self) -> CmdResult<PageId> {
        let id = PageId::new(&mut self.ids);
        self.exec(Command::AddPage {
            page: Page::new(id),
            index: None,
        })?;
        Ok(id)
    }

    /// Add a frame on `parent` and return its id.
    pub fn frame(
        &mut self,
        parent: Parent,
        frame: Frame,
        story: Option<Story>,
    ) -> CmdResult<FrameId> {
        let id = frame.id;
        self.exec(Command::AddFrame {
            parent,
            index: None,
            frame,
            story,
        })?;
        Ok(id)
    }

    /// A text frame with plain text (`\n` separates paragraphs).
    pub fn text(&mut self, page: PageId, rect: Rect, text: &str) -> CmdResult<FrameId> {
        let fid = FrameId::new(&mut self.ids);
        let sid = StoryId::new(&mut self.ids);
        let mut story = Story::new(sid, fid);
        story.insert_text(0, text, None);
        let frame = Frame::new(fid, rect, FrameContent::Text(TextFrame::new(sid)));
        self.frame(Parent::Page(page), frame, Some(story))
    }

    /// A text frame with explicit paragraphs.
    pub fn text_paragraphs(
        &mut self,
        parent: Parent,
        rect: Rect,
        paragraphs: Vec<Paragraph>,
    ) -> CmdResult<FrameId> {
        let fid = FrameId::new(&mut self.ids);
        let sid = StoryId::new(&mut self.ids);
        let story = Story {
            id: sid,
            frames: vec![fid],
            paragraphs,
        };
        let frame = Frame::new(fid, rect, FrameContent::Text(TextFrame::new(sid)));
        self.frame(parent, frame, Some(story))
    }

    pub fn shape(&mut self, page: PageId, rect: Rect, shape: Shape) -> CmdResult<FrameId> {
        let fid = FrameId::new(&mut self.ids);
        let frame = Frame::new(fid, rect, FrameContent::Shape(ShapeFrame { shape }));
        self.frame(Parent::Page(page), frame, None)
    }

    pub fn image(
        &mut self,
        page: PageId,
        rect: Rect,
        asset: Option<crate::id::AssetId>,
    ) -> CmdResult<FrameId> {
        let fid = FrameId::new(&mut self.ids);
        let frame = Frame::new(fid, rect, FrameContent::Image(ImageFrame::new(asset)));
        self.frame(Parent::Page(page), frame, None)
    }

    pub fn link(&mut self, from: FrameId, to: FrameId) -> CmdResult<&mut Self> {
        self.exec(Command::LinkFrames { from, to })
    }

    /// Story shown by a text frame.
    pub fn story_of(&self, frame: FrameId) -> StoryId {
        self.doc().story_of(frame).expect("text frame")
    }

    /// Mutate a frame's non-geometry properties through commands.
    pub fn style(&mut self, frame: FrameId, style: FrameStyle) -> CmdResult<&mut Self> {
        self.exec(Command::SetFrameStyle { frame, style })
    }

    pub fn wrap(&mut self, frame: FrameId, wrap: TextWrap) -> CmdResult<&mut Self> {
        self.exec(Command::SetWrap { frame, wrap })
    }
}
