# Changelog

User-visible changes to Sim;Logic. This project is pre-1.0; minor releases may
contain documented breaking changes.

## Unreleased - 0.1.0 preparation

This is the first registry release candidate, not a published-version claim.
The renderer dependency is the official Sim;Engine 0.4.2 release.

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

### Release preparation

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
