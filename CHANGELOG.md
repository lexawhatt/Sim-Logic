# Changelog

User-visible changes to Sim;Logic. This project is pre-1.0; minor releases may
contain documented breaking changes.

## 0.2.0 - 2026-11-25

Uses published Sim;Engine 0.5.0. Adds bounded semantic text input, exact text
geometry and two-resolution rendering through the standard desktop host.

### Migration from 0.1

- Update consumer version requirements to `0.2.0`; a `0.1` requirement will not
  match this minor release. Remove development path patches when adopting the
  registry packages.
- Account for the new `InputEvent::Text` marker in exhaustive matches and use
  ordered semantic text events for character input. `BeginFrameRejection::Text`
  and new public desktop error variants also affect exhaustive matches.
- Use the same registry Engine 0.5.0 source identity across direct and Logic-mediated
  Engine dependencies. Engine 0.4 types are not interchangeable with 0.5 types.

### Added and changed

- Add opt-in Base/Detail rendering in the standard desktop host, per-layer
  resolution/effects, cuboid fog participation, transactional targets and recovery.
  Frozen pass/attachment limits, typed ContinueUi feedback, frame provenance and
  per-role GPU diagnostics preserve the existing input and screen composition path.

- Integrate published Sim;Engine 0.5.0 with an exact registry dependency.
  The earlier development path dependency is removed; no Engine checkout is required.
- Add bounded, ordered UTF-8/IME input, logical editor commands, explicit paste
  injection, frame-only delivery and modal keyboard ownership with cancellation.
  InputEvent gains a Text marker; exhaustive consumer matches must account for it.
- Expose managed-font CPU shaping/rasterization and bounded LTR surface layout
  with exact Engine pen carets, cluster-boundary errors and selection spans for application-owned pages.
- Document bounded background IO handoff, generation filtering and shutdown.

## 0.1.0 - 2026-09-26

Initial version, using the official Sim;Engine 0.4.2 release for rendering.

### Added

- A shared headless/desktop application runtime: one active World, ordered
  Startup, FixedUpdate and FrameUpdate Systems, bounded fixed-step catch-up,
  interpolation, pause, time scale and explicit exit requests.
- Approved components, provenance-checked entity handles, typed queries,
  deferred structural Commands, World/Application Resources and stage-local
  events. World factories prepare isolated candidates; replacement requests
  use runtime-issued intents and explicit arbitration.
- Typed keyboard and pointer input, click-time geometry, scrolling, focus-loss
  cancellation, cursor capture and window requests. Headless callers inject
  the same portable input events and inspect extraction without a GPU.
- Opt-in movement, velocity, acceleration and camera-follow helpers, plus
  circle/rectangle overlap queries independent of visual appearance.
- 2D world visuals, screen shapes and open styled paths, images, ordering,
  clipping, pointer buttons and a standalone keyboard-focus helper.
- Optional CPU fonts and headless text preparation, mixed-font labels and
  retained desktop glyph rendering. Exact unchanged screen snapshots and
  geometry runs can reuse CPU/GPU work, with explicit limits and diagnostics.
- A retained 3D bridge for host meshes and attributes, Opaque/Mask/Blend
  materials, lighting, fog, texture revisions, mipmaps, native rasterization,
  scene-owned dynamic updates and device recovery through Sim;Engine.
- Opt-in presentation-budget feedback, renderer timing/update reports,
  single-source audio output and the optional Ferris image helper.
- Small feature examples and larger voxel, territory, shooter and piano
  applications, with headless tests sharing their application setup.

### Documentation and packaging

- Short product README, focused usage guides and a repeatable package check.
- Explicit crate contents and docs.rs feature configuration; private working
  notes, saves, IDE settings and build products are excluded.
- Example entry points grouped under their own directories. Cargo example
  names and launch commands are unchanged.

### Known boundaries

- Sequential Systems, one active World/window, synchronous factories without
  runtime payloads; no general Faulted schedule or Behavior facade.
- No automatic physics, full UI layout system, model importer or audio voice
  manager. Domain rules stay in the consuming application.
- Arbitrary closed styled screen paths remain unsupported by Engine 0.4.2;
  circle and rectangle outlines are supported.
- Transparent 3D objects are sorted, but intersecting surfaces and triangle
  order within a transparent mesh are not solved by that sorting.
- Desktop verification covers Linux/Vulkan. Upstream Windows support is not
  a claim that Sim;Logic's host and examples have been qualified on Windows.
- Reuse counters and workload measurements are not a whole-application
  zero-allocation or universal frame-rate guarantee.
