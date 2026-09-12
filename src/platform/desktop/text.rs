//! Bounded retained Engine text, separate from canonical ECS state.

use std::{error::Error, fmt};

use sim_engine::{
    FrameComposer, FramePassOptions, ImageBatchPlacement, ImageSampling, LogicalScreenVector,
    TextAtlas2d, TextRun2d, WgpuRenderer,
};

use crate::{
    ExtractedFrame, ResolvedScreenText,
    identity::{LogicEntity, WorldGeneration},
    text::{TextError, TextFont},
};

/// A managed label failed desktop preparation after a valid CPU frame.
///
/// No partial surface frame is presented. Accepted earlier run updates or newly
/// cached glyphs may remain; this is not rollback of CPU state or GPU uploads.
#[derive(Debug)]
pub enum DesktopTextError {
    /// Font settings could not be represented at the current display DPI.
    Settings {
        /// Label requesting the font.
        source: LogicEntity,
        /// Concrete CPU text/settings failure.
        error: TextError,
    },
    /// Engine rejected an atlas, glyph raster, run update or device resource.
    Engine {
        /// Label whose preparation failed.
        source: LogicEntity,
        /// Underlying Engine error, including atlas-full and work-limit cases.
        error: sim_engine::TextError,
    },
    /// Bounded cache metadata could not be allocated.
    Allocation {
        /// Minimum additional metadata bytes requested.
        requested_bytes: usize,
    },
    /// The published draw plan and prepared cache disagree.
    InvalidDrawPlan,
}

impl fmt::Display for DesktopTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Settings { source, error } => write!(f, "text {source:?} settings: {error}"),
            Self::Engine { source, error } => write!(f, "text {source:?} preparation: {error}"),
            Self::Allocation { requested_bytes } => {
                write!(f, "text cache could not reserve {requested_bytes} bytes")
            }
            Self::InvalidDrawPlan => f.write_str("text snapshot and desktop cache disagree"),
        }
    }
}

impl Error for DesktopTextError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Settings { error, .. } => Some(error),
            Self::Engine { error, .. } => Some(error),
            _ => None,
        }
    }
}

struct FontEntry {
    font: TextFont,
    atlas: TextAtlas2d,
}

struct RunEntry {
    source: LogicEntity,
    font: TextFont,
    run: TextRun2d,
}

/// At most one atlas per used registration and one last-applied run per live
/// visible entity. Fonts have frozen per-atlas/run bounds; no historic strings
/// or DPI variants accumulate. Engine retains its own bounded frame bindings.
#[derive(Default)]
pub(super) struct DesktopText {
    cache: RetainedTextCache<FontEntry, RunEntry>,
}

/// Resource-free bookkeeping can be tested without fabricating Engine handles.
struct RetainedTextCache<Font, Run> {
    fonts: Vec<Font>,
    runs: Vec<Run>,
    live: Vec<u64>,
    generation: Option<WorldGeneration>,
    scale_bits: Option<u32>,
}

impl<Font, Run> Default for RetainedTextCache<Font, Run> {
    fn default() -> Self {
        Self {
            fonts: Vec::new(),
            runs: Vec::new(),
            live: Vec::new(),
            generation: None,
            scale_bits: None,
        }
    }
}

impl<Font, Run> RetainedTextCache<Font, Run> {
    fn clear(&mut self) {
        self.runs.clear();
        self.fonts.clear();
        self.live.clear();
        self.generation = None;
        self.scale_bits = None;
    }

    fn begin_frame(
        &mut self,
        generation: WorldGeneration,
        scale: f32,
        clear_frame_bindings: impl FnOnce(),
    ) {
        if self.scale_bits != Some(scale.to_bits()) {
            // Release binding-owned references before retiring old DPI atlases.
            // Canonical CPU metrics are unchanged; a failed rebuild presents nothing.
            if self.scale_bits.is_some() {
                clear_frame_bindings();
            }
            self.clear();
            self.scale_bits = Some(scale.to_bits());
        }
        if self.generation != Some(generation) {
            self.runs.clear();
            self.generation = Some(generation);
        }
    }

    fn retain_visible_runs(
        &mut self,
        labels: impl ExactSizeIterator<Item = (LogicEntity, bool)>,
        source: impl Fn(&Run) -> LogicEntity,
    ) -> Result<(), DesktopTextError> {
        self.live.clear();
        self.live
            .try_reserve(labels.len())
            .map_err(|_| DesktopTextError::Allocation {
                requested_bytes: labels.len().saturating_mul(size_of::<u64>()),
            })?;
        self.live.extend(
            labels.filter_map(|(source, nonempty)| nonempty.then_some(source.stable_bits())),
        );
        self.live.sort_unstable();
        self.runs
            .retain(|run| self.live.binary_search(&source(run).stable_bits()).is_ok());
        Ok(())
    }
}

impl DesktopText {
    pub(super) fn has_runs(&self) -> bool {
        !self.cache.runs.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.cache.clear();
    }

    pub(super) fn prepare(
        &mut self,
        renderer: &mut WgpuRenderer,
        extracted: &ExtractedFrame,
    ) -> Result<(), DesktopTextError> {
        let scale = renderer.scale_factor() as f32;
        self.cache
            .begin_frame(extracted.world_generation(), scale, || {
                renderer.clear_frame_cache()
            });
        self.cache.retain_visible_runs(
            extracted
                .resolved_screen_texts()
                .iter()
                .map(|text| (text.source(), !text.text().is_empty())),
            |run| run.source,
        )?;
        for visual in extracted.resolved_screen_texts() {
            if !visual.text().is_empty() {
                self.prepare_one(renderer, visual, scale)?;
            }
        }
        Ok(())
    }

    fn prepare_one(
        &mut self,
        renderer: &WgpuRenderer,
        visual: &ResolvedScreenText,
        scale: f32,
    ) -> Result<(), DesktopTextError> {
        let source = visual.source();
        let font = visual.font();
        let font_index = match self
            .cache
            .fonts
            .binary_search_by_key(&font.slot(), |entry| entry.font.slot())
        {
            Ok(index) => index,
            Err(index) => {
                self.cache
                    .fonts
                    .try_reserve(1)
                    .map_err(|_| DesktopTextError::Allocation {
                        requested_bytes: size_of::<FontEntry>(),
                    })?;
                let settings = font.settings();
                let style = settings
                    .style(scale)
                    .map_err(|error| DesktopTextError::Settings { source, error })?;
                let atlas = TextAtlas2d::new(
                    renderer,
                    font.face().clone(),
                    style,
                    settings.atlas_budget(),
                )
                .map_err(|error| DesktopTextError::Engine { source, error })?;
                self.cache.fonts.insert(
                    index,
                    FontEntry {
                        font: font.clone(),
                        atlas,
                    },
                );
                index
            }
        };
        let atlas = &mut self.cache.fonts[font_index].atlas;
        match self
            .cache
            .runs
            .binary_search_by_key(&source.stable_bits(), |entry| entry.source.stable_bits())
        {
            Ok(index) => {
                let entry = &mut self.cache.runs[index];
                if entry.font != *font {
                    // Font changes replace one run, never mutate another label.
                    let run = atlas
                        .prepare(renderer, visual.text(), font.settings().layout_budget())
                        .map_err(|error| DesktopTextError::Engine { source, error })?;
                    *entry = RunEntry {
                        source,
                        font: font.clone(),
                        run,
                    };
                } else if entry.run.text() != visual.text() {
                    atlas
                        .update(
                            renderer,
                            &mut entry.run,
                            visual.text(),
                            font.settings().layout_budget(),
                        )
                        .map_err(|error| DesktopTextError::Engine { source, error })?;
                }
            }
            Err(index) => {
                self.cache
                    .runs
                    .try_reserve(1)
                    .map_err(|_| DesktopTextError::Allocation {
                        requested_bytes: size_of::<RunEntry>(),
                    })?;
                let run = atlas
                    .prepare(renderer, visual.text(), font.settings().layout_budget())
                    .map_err(|error| DesktopTextError::Engine { source, error })?;
                self.cache.runs.insert(
                    index,
                    RunEntry {
                        source,
                        font: font.clone(),
                        run,
                    },
                );
            }
        }
        Ok(())
    }

    pub(super) fn draw<'a>(
        &'a self,
        frame: &mut FrameComposer<'a>,
        visual: &ResolvedScreenText,
        options: FramePassOptions,
    ) -> Result<(), super::images::ScreenPresentationError> {
        let font_index = self
            .cache
            .fonts
            .binary_search_by_key(&visual.font().slot(), |entry| entry.font.slot())
            .map_err(|_| DesktopTextError::InvalidDrawPlan)?;
        let run_index = self
            .cache
            .runs
            .binary_search_by_key(&visual.source().stable_bits(), |entry| {
                entry.source.stable_bits()
            })
            .map_err(|_| DesktopTextError::InvalidDrawPlan)?;
        let run = &self.cache.runs[run_index];
        if run.source != visual.source()
            || run.font != *visual.font()
            || run.run.text() != visual.text()
        {
            return Err(DesktopTextError::InvalidDrawPlan.into());
        }
        let position = visual.baseline_origin().to_vec2();
        let placement = ImageBatchPlacement::new(
            LogicalScreenVector::new(position.x(), position.y()),
            visual.tint(),
        )
        .map_err(|error| DesktopTextError::Engine {
            source: visual.source(),
            error: sim_engine::TextError::Glyph(sim_engine::GlyphError::Image(error)),
        })?;
        frame.draw_glyph_run_placed(
            self.cache.fonts[font_index].atlas.atlas(),
            run.run.glyph_run(),
            placement,
            ImageSampling::Linear,
            options,
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "text/tests.rs"]
mod tests;
