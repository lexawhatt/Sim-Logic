#![cfg(feature = "text")]

#[path = "../examples/voxel_sandbox/app.rs"]
mod app;
#[path = "../examples/voxel_sandbox/materials.rs"]
mod materials;
#[path = "../examples/voxel_sandbox/model/mod.rs"]
mod model;
#[path = "../examples/voxel_sandbox/presentation.rs"]
mod presentation;
#[path = "../examples/voxel_sandbox/projection.rs"]
mod projection;
#[path = "../examples/voxel_sandbox/scene.rs"]
mod scene;
#[path = "../examples/voxel_sandbox/showcase.rs"]
mod showcase;
#[path = "../examples/voxel_sandbox/view.rs"]
mod view;

#[path = "voxel_sandbox/runtime.rs"]
mod runtime;

#[path = "voxel_sandbox/interface.rs"]
mod interface;

#[path = "voxel_sandbox/ui_retry.rs"]
mod ui_retry;
