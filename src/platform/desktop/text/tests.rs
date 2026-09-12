//! Actual cache bookkeeping with mock resources, not simulated GPU handles.

use std::{cell::RefCell, rc::Rc};

use bevy_ecs::entity::Entity;

use super::*;
use crate::{
    desktop::{DesktopConfig, FrameCacheBudget},
    identity::ApplicationId,
};

fn generation(sequence: u64) -> WorldGeneration {
    WorldGeneration::new(ApplicationId::from_raw(9), sequence)
}

fn source(sequence: u64, row: u32) -> LogicEntity {
    LogicEntity::new(
        ApplicationId::from_raw(9),
        generation(sequence),
        Entity::from_raw_u32(row).unwrap(),
    )
}

struct TrackedResource {
    name: &'static str,
    drops: Rc<RefCell<Vec<&'static str>>>,
}

impl Drop for TrackedResource {
    fn drop(&mut self) {
        self.drops.borrow_mut().push(self.name);
    }
}

#[test]
fn dpi_change_releases_bindings_before_old_runs_and_atlases() {
    let drops = Rc::new(RefCell::new(Vec::new()));
    let mut cache = RetainedTextCache::<TrackedResource, TrackedResource>::default();
    cache.begin_frame(generation(1), 1.0, || {
        panic!("new cache has no old bindings")
    });
    cache.fonts.push(TrackedResource {
        name: "atlas",
        drops: Rc::clone(&drops),
    });
    cache.runs.push(TrackedResource {
        name: "run",
        drops: Rc::clone(&drops),
    });
    cache.live.push(3);

    cache.begin_frame(generation(1), 1.5, || drops.borrow_mut().push("bindings"));

    assert_eq!(*drops.borrow(), ["bindings", "run", "atlas"]);
    assert!(cache.fonts.is_empty());
    assert!(cache.runs.is_empty());
    assert!(cache.live.is_empty());
    assert_eq!(cache.scale_bits, Some(1.5_f32.to_bits()));
    assert_eq!(cache.generation, Some(generation(1)));
    cache.begin_frame(generation(1), 1.5, || panic!("same DPI keeps bindings"));
    assert_eq!(drops.borrow().len(), 3);
}

#[test]
fn world_replacement_retires_old_runs_even_when_raw_entity_bits_match() {
    let mut cache = RetainedTextCache::<u32, LogicEntity>::default();
    cache.begin_frame(generation(1), 1.25, || {});
    cache.fonts.push(7);
    cache.runs.push(source(1, 0));
    assert_eq!(source(1, 0).stable_bits(), source(2, 0).stable_bits());

    cache.begin_frame(generation(2), 1.25, || {
        panic!("World switch keeps same-DPI fonts")
    });
    cache
        .retain_visible_runs([(source(2, 0), true)].into_iter(), |run| *run)
        .unwrap();

    assert!(cache.runs.is_empty());
    assert_eq!(cache.fonts, [7]);
    assert_eq!(cache.generation, Some(generation(2)));
}

#[test]
fn missing_disabled_and_empty_labels_retire_only_their_own_runs() {
    let mut cache = RetainedTextCache::<(), LogicEntity>::default();
    cache.begin_frame(generation(1), 1.0, || {});
    cache
        .runs
        .extend([source(1, 0), source(1, 1), source(1, 2), source(1, 3)]);
    // Disabled/despawned sources 0 and 3 are absent from the published snapshot;
    // source 1 is still a component but its replacement string is empty.
    cache
        .retain_visible_runs(
            [(source(1, 2), true), (source(1, 1), false)].into_iter(),
            |run| *run,
        )
        .unwrap();
    assert_eq!(cache.runs, [source(1, 2)]);
    assert_eq!(cache.live, [source(1, 2).stable_bits()]);

    cache
        .retain_visible_runs(std::iter::empty(), |run| *run)
        .unwrap();
    assert!(cache.runs.is_empty());
    assert!(cache.live.is_empty());
}

#[test]
fn no_live_labels_keep_only_application_font_resources() {
    let drops = Rc::new(RefCell::new(Vec::new()));
    let mut cache = RetainedTextCache::<TrackedResource, (LogicEntity, TrackedResource)>::default();
    cache.begin_frame(generation(1), 1.0, || {});
    cache.fonts.push(TrackedResource {
        name: "atlas",
        drops: Rc::clone(&drops),
    });
    cache.runs.push((
        source(1, 0),
        TrackedResource {
            name: "run",
            drops: Rc::clone(&drops),
        },
    ));

    cache
        .retain_visible_runs(std::iter::empty(), |run| run.0)
        .unwrap();

    assert_eq!(*drops.borrow(), ["run"]);
    assert_eq!(cache.fonts.len(), 1);
    assert!(cache.runs.is_empty());
    // Font caching is application-scoped; this test does not claim that Engine
    // idle bindings or submitted GPU work release every external reference now.
}

#[test]
fn warmed_bookkeeping_reuses_vector_storage_for_unchanged_labels() {
    let mut cache = RetainedTextCache::<u32, LogicEntity>::default();
    cache.begin_frame(generation(1), 2.0, || {});
    cache.fonts.push(7);
    cache.runs.extend((0..64).map(|row| source(1, row)));
    let labels = (0..64)
        .rev()
        .map(|row| (source(1, row), true))
        .collect::<Vec<_>>();
    cache
        .retain_visible_runs(labels.iter().copied(), |run| *run)
        .unwrap();
    let storage = (
        cache.fonts.as_ptr(),
        cache.runs.as_ptr(),
        cache.live.as_ptr(),
    );
    let capacities = (
        cache.fonts.capacity(),
        cache.runs.capacity(),
        cache.live.capacity(),
    );

    for _ in 0..360 {
        cache.begin_frame(generation(1), 2.0, || {
            panic!("unchanged epoch must retain bindings")
        });
        cache
            .retain_visible_runs(labels.iter().copied(), |run| *run)
            .unwrap();
        assert_eq!(
            (
                cache.fonts.as_ptr(),
                cache.runs.as_ptr(),
                cache.live.as_ptr()
            ),
            storage
        );
        assert_eq!(
            (
                cache.fonts.capacity(),
                cache.runs.capacity(),
                cache.live.capacity()
            ),
            capacities
        );
        assert_eq!(cache.runs.len(), 64);
    }
}

#[test]
fn failed_live_metadata_reservation_preserves_runs_and_allows_retry() {
    let mut cache = RetainedTextCache::<u32, LogicEntity>::default();
    cache.begin_frame(generation(1), 1.0, || {});
    cache.fonts.push(7);
    cache.runs.push(source(1, 0));
    // u64 metadata for usize::MAX entries overflows Vec's addressable capacity
    // before allocation or iterator traversal, independently of available RAM.
    let result = cache.retain_visible_runs(
        std::iter::repeat_n((source(1, 0), true), usize::MAX),
        |run| *run,
    );
    assert!(matches!(
        result,
        Err(DesktopTextError::Allocation {
            requested_bytes: usize::MAX
        })
    ));
    assert_eq!(cache.fonts, [7]);
    assert_eq!(cache.runs, [source(1, 0)]);

    cache
        .retain_visible_runs([(source(1, 0), true)].into_iter(), |run| *run)
        .unwrap();
    assert_eq!(cache.runs, [source(1, 0)]);
}

#[test]
fn explicit_recovery_clear_drops_resources_and_resets_both_epochs() {
    let mut cache = RetainedTextCache::<u32, LogicEntity>::default();
    cache.begin_frame(generation(1), 1.0, || {});
    cache.fonts.push(7);
    cache.runs.push(source(1, 0));
    cache
        .retain_visible_runs([(source(1, 0), true)].into_iter(), |run| *run)
        .unwrap();
    let capacities = (
        cache.fonts.capacity(),
        cache.runs.capacity(),
        cache.live.capacity(),
    );

    cache.clear();

    assert!(cache.fonts.is_empty());
    assert!(cache.runs.is_empty());
    assert!(cache.live.is_empty());
    assert_eq!(cache.generation, None);
    assert_eq!(cache.scale_bits, None);
    assert_eq!(
        (
            cache.fonts.capacity(),
            cache.runs.capacity(),
            cache.live.capacity()
        ),
        capacities
    );
    cache.begin_frame(generation(1), 1.0, || {
        panic!("replacement renderer already cleared bindings")
    });
    assert_eq!(cache.generation, Some(generation(1)));
}

#[test]
fn desktop_idle_cache_configuration_preserves_window_settings_and_allows_zero() {
    let mut config = DesktopConfig::new("Text cache", 960.0, 540.0).unwrap();
    assert_eq!(config.frame_cache_budget(), FrameCacheBudget::default());
    for budget in [
        FrameCacheBudget::new(0, 0, 0, 0),
        FrameCacheBudget::new(8192, 512, 16384, 8),
    ] {
        config.set_frame_cache_budget(budget);
        assert_eq!(config.frame_cache_budget(), budget);
        assert_eq!(config.title(), "Text cache");
        assert_eq!(config.logical_width(), 960.0);
        assert_eq!(config.logical_height(), 540.0);
    }
}

#[test]
fn dpi_layout_selection_borrows_matching_line_and_rebuilds_only_an_exact_new_style() {
    use crate::text::{ScreenTextVisual, TextLimits, TextRegistry, TextSettings};

    let mut fonts = TextRegistry::new(ApplicationId::from_raw(9), TextLimits::default());
    let font = fonts
        .register(
            include_bytes!("../../../../tests/assets/text/DejaVuSans.ttf").to_vec(),
            TextSettings::new(20.0).unwrap(),
        )
        .unwrap();
    let visual = ScreenTextVisual::new(
        font.clone(),
        "AV ffi Привет",
        sim_engine::LogicalScreenPosition::new(0.0, 40.0),
    )
    .unwrap();
    let matching = DesktopPreparedLine::new(&visual, 1.0).unwrap();
    assert!(std::ptr::eq(matching.line(), visual.shaped_line()));
    for scale in [1.25, 1.5, 2.0] {
        let prepared = DesktopPreparedLine::new(&visual, scale).unwrap();
        assert!(!std::ptr::eq(prepared.line(), visual.shaped_line()));
        assert_eq!(prepared.line().text(), visual.text());
        assert_eq!(prepared.line().glyphs(), visual.shaped_line().glyphs());
        assert_eq!(prepared.line().advance(), visual.metrics().advance());
        prepared
            .line()
            .validate_for(
                font.face(),
                &font.settings().style(scale).unwrap(),
                &font.settings().layout_budget(),
            )
            .unwrap();
    }
    assert!(DesktopPreparedLine::new(&visual, f32::NAN).is_err());
    assert_eq!(
        visual.shaped_line().style(),
        &font.settings().style(1.0).unwrap()
    );
}
