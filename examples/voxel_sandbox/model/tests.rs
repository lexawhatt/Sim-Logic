use super::*;

fn flat_region() -> Region {
    let mut region = Region::from_blocks(vec![Block::Air; terrain::CELL_COUNT]);
    for z in 0..WIDTH {
        for x in 0..WIDTH {
            region.set([x, 0, z], Block::Stone);
        }
    }
    region
}

#[test]
fn masked_leaves_do_not_hide_geometry_visible_through_their_holes() {
    let mut region = Region::from_blocks(vec![Block::Air; terrain::CELL_COUNT]);
    region.set([2, 2, 2], Block::Stone);
    region.set([3, 2, 2], Block::Leaves);
    let meshes = region.chunk_meshes(0).unwrap();
    let triangles = |block| {
        meshes
            .iter()
            .filter(|part| part.block == block)
            .map(|part| part.mesh.triangle_count())
            .sum::<usize>()
    };
    assert_eq!(
        triangles(Block::Stone),
        12,
        "stone behind a leaf mask stays visible"
    );
    assert_eq!(
        triangles(Block::Leaves),
        10,
        "opaque stone hides the neighboring leaf face"
    );
    region.set([2, 2, 2], Block::Leaves);
    assert_eq!(
        region
            .chunk_meshes(0)
            .unwrap()
            .iter()
            .map(|part| part.mesh.triangle_count())
            .sum::<usize>(),
        24
    );
}

fn flat_game() -> SaveGame {
    let region = flat_region();
    let player = Player::at([12.5, 1.02, 12.5]);
    SaveGame {
        active: RegionId::Meadow,
        player,
        inventory: Inventory::default(),
        creative: false,
        seed: 7,
        regions: [region.clone(), region],
        parked_players: [player; 2],
    }
}

#[test]
fn deterministic_regions_are_distinct_and_both_spawns_are_clear() {
    let first = SaveGame::new(42);
    let second = SaveGame::new(42);
    for id in RegionId::ALL {
        assert_eq!(
            first.region(id).bootstrap_blocks(),
            second.region(id).bootstrap_blocks()
        );
        assert!(first.region(id).spawn().valid(first.region(id)));
        assert!(first.region(id).solid_count() < terrain::CELL_COUNT);
    }
    assert_ne!(
        first.region(RegionId::Meadow).bootstrap_blocks(),
        first.region(RegionId::Canyon).bootstrap_blocks()
    );
}

#[test]
fn boundary_edit_invalidates_both_adjacent_chunks_not_unrelated_chunks() {
    let mut region = flat_region();
    let before: Vec<_> = (0..CHUNK_COUNT)
        .map(|chunk| region.chunk_revision(chunk))
        .collect();
    region.set([7, 2, 3], Block::Wood);
    assert_ne!(region.chunk_revision(0), before[0]);
    assert_ne!(region.chunk_revision(1), before[1]);
    for (chunk, revision) in before.iter().enumerate().skip(2) {
        assert_eq!(region.chunk_revision(chunk), *revision);
    }
    let revision = region.chunk_revision(0);
    region.set([7, 2, 3], Block::Wood);
    assert_eq!(region.chunk_revision(0), revision);
}

#[test]
fn isolated_and_adjacent_blocks_only_mesh_exposed_faces() {
    let mut region = Region::from_blocks(vec![Block::Air; terrain::CELL_COUNT]);
    region.set([2, 2, 2], Block::Stone);
    let meshes = region.chunk_meshes(0).unwrap();
    assert_eq!(
        meshes
            .iter()
            .map(|part| part.mesh.triangle_count())
            .sum::<usize>(),
        12
    );
    assert_eq!(meshes.len(), 3);
    region.set([3, 2, 2], Block::Grass);
    assert_eq!(
        region
            .chunk_meshes(0)
            .unwrap()
            .iter()
            .map(|part| part.mesh.triangle_count())
            .sum::<usize>(),
        20
    );
    assert!(region.chunk_meshes(CHUNK_COUNT).unwrap().is_empty());
}

#[test]
fn cross_chunk_neighbors_hide_the_shared_face() {
    let mut region = Region::from_blocks(vec![Block::Air; terrain::CELL_COUNT]);
    region.set([7, 2, 2], Block::Stone);
    region.set([8, 2, 2], Block::Wood);
    for chunk in [0, 1] {
        assert_eq!(
            region
                .chunk_meshes(chunk)
                .unwrap()
                .iter()
                .map(|part| part.mesh.triangle_count())
                .sum::<usize>(),
            10
        );
    }
}

#[test]
fn all_six_voxel_faces_have_outward_winding_matching_normals_and_complete_attributes() {
    let mut region = Region::from_blocks(vec![Block::Air; terrain::CELL_COUNT]);
    region.set([2, 2, 2], Block::Stone);
    let mut directions = Vec::new();
    let center = [2.5_f32; 3];
    for part in region.chunk_meshes(0).unwrap() {
        let mesh = &part.mesh;
        assert_eq!(mesh.normals().len(), mesh.vertices().len());
        assert_eq!(mesh.vertex_colors().len(), mesh.vertices().len());
        assert_eq!(mesh.texture_coordinates().len(), mesh.vertices().len());
        assert!(
            mesh.vertex_colors()
                .iter()
                .all(|color| color.is_normalized())
        );
        for uv in mesh.texture_coordinates() {
            assert!((0.0..=1.0).contains(&uv.u()));
            assert!((0.0..=1.0).contains(&uv.v()));
        }
        for indices in mesh.triangle_indices().chunks_exact(3) {
            let points: Vec<_> = indices
                .iter()
                .map(|index| mesh.vertices()[*index as usize])
                .collect();
            let a = points[0];
            let b = points[1];
            let c = points[2];
            let u = [b.x() - a.x(), b.y() - a.y(), b.z() - a.z()];
            let v = [c.x() - a.x(), c.y() - a.y(), c.z() - a.z()];
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let normal = mesh.normals()[indices[0] as usize];
            assert_eq!(cross, [normal.x(), normal.y(), normal.z()]);
            assert!(
                indices
                    .iter()
                    .all(|index| mesh.normals()[*index as usize] == normal)
            );
            let outward = [a.x() - center[0], a.y() - center[1], a.z() - center[2]];
            assert!(
                cross
                    .iter()
                    .zip(outward)
                    .map(|(normal, offset)| normal * offset)
                    .sum::<f32>()
                    > 0.0
            );
            if !directions.contains(&cross) {
                directions.push(cross);
            }
        }
    }
    assert_eq!(directions.len(), 6);
}

#[test]
fn generated_chunk_meshes_have_bounded_valid_topology() {
    let game = SaveGame::new(42);
    for id in RegionId::ALL {
        for chunk in 0..CHUNK_COUNT {
            let parts = game.region(id).chunk_meshes(chunk).unwrap();
            assert!(parts.len() <= Block::SOLID.len() * 3);
            assert!(
                parts
                    .iter()
                    .map(|part| part.mesh.triangle_count())
                    .sum::<usize>()
                    <= 6144
            );
            for part in parts {
                assert!(part.block.solid());
                assert!(part.shade <= 2);
                assert_eq!(part.mesh.normals().len(), part.mesh.vertices().len());
                assert_eq!(part.mesh.vertex_colors().len(), part.mesh.vertices().len());
                assert_eq!(
                    part.mesh.texture_coordinates().len(),
                    part.mesh.vertices().len()
                );
                for vertex in part.mesh.vertices() {
                    assert!(vertex.x() >= 0.0 && vertex.x() <= WIDTH as f32);
                    assert!(vertex.y() >= 0.0 && vertex.y() <= HEIGHT as f32);
                    assert!(vertex.z() >= 0.0 && vertex.z() <= WIDTH as f32);
                }
            }
        }
    }
}

#[test]
fn raycast_finds_entry_face_and_enforces_reach() {
    let mut region = flat_region();
    region.set([12, 2, 8], Block::Wood);
    let hit = raycast(&region, [12.5, 2.5, 12.5], [0.0, 0.0, -1.0], 6.0).unwrap();
    assert_eq!(hit.cell, [12, 2, 8]);
    assert_eq!(hit.adjacent, Some([12, 2, 9]));
    assert_eq!(hit.distance, 3.5);
    assert!(raycast(&region, [12.5, 2.5, 12.5], [0.0, 0.0, -1.0], 3.49).is_none());
    assert!(raycast(&region, [12.5, 2.5, 12.5], [0.0; 3], 6.0).is_none());
    assert!(raycast(&region, [f32::NAN; 3], [1.0, 0.0, 0.0], 6.0).is_none());
    assert!(raycast(&region, [12.5; 3], [f32::INFINITY, 0.0, 0.0], 6.0).is_none());
    assert!(raycast(&region, [12.5; 3], [1.0, 0.0, 0.0], 100.0).is_none());
}

#[test]
fn exact_corner_ray_skips_cells_touched_only_at_an_edge() {
    let mut region = flat_region();
    region.set([2, 2, 1], Block::Wood);
    region.set([3, 2, 3], Block::Grass);
    let hit = raycast(&region, [1.5, 2.5, 1.5], [1.0, 0.0, 1.0], 6.0).unwrap();
    assert_eq!(hit.cell, [3, 2, 3]);
}

#[test]
fn resting_zero_input_and_bad_steps_remain_finite() {
    let region = flat_region();
    let mut player = Player::at([12.5, 1.02, 12.5]);
    for _ in 0..120 {
        player.step(&region, Movement::default(), 1.0 / 60.0);
    }
    assert!(player.grounded);
    assert!(player.position.into_iter().all(f32::is_finite));
    assert!(!player.collides(&region));
    let before = player;
    for seconds in [-1.0, 1.0, f32::NAN, f32::INFINITY] {
        player.step(&region, Movement::default(), seconds);
        assert_eq!(player, before);
    }
}

#[test]
fn diagonal_speed_is_normalized_and_walls_stop_bounded_steps() {
    let mut region = flat_region();
    let start = [12.5, 1.02, 12.5];
    let mut straight = Player::at(start);
    let mut diagonal = straight;
    straight.step(
        &region,
        Movement {
            forward: 1.0,
            ..Movement::default()
        },
        0.1,
    );
    diagonal.step(
        &region,
        Movement {
            forward: 1.0,
            strafe: 1.0,
            jump: false,
        },
        0.1,
    );
    let horizontal_distance =
        |player: Player| (player.position[0] - start[0]).hypot(player.position[2] - start[2]);
    assert!((horizontal_distance(straight) - horizontal_distance(diagonal)).abs() < 0.0001);
    for y in 1..5 {
        region.set([12, y, 10], Block::Stone);
    }
    for _ in 0..100 {
        straight.step(
            &region,
            Movement {
                forward: 1.0,
                ..Movement::default()
            },
            0.1,
        );
    }
    assert!(straight.position[2] >= 11.0 + Player::RADIUS);
    assert!(!straight.collides(&region));
}

#[test]
fn jump_returns_to_ground_without_falling_through_floor() {
    let region = flat_region();
    let mut player = Player::at([12.5, 1.02, 12.5]);
    for _ in 0..10 {
        player.step(&region, Movement::default(), 1.0 / 60.0);
    }
    let resting = player.position[1];
    player.step(
        &region,
        Movement {
            jump: true,
            ..Movement::default()
        },
        1.0 / 60.0,
    );
    assert!(player.position[1] > resting);
    for _ in 0..120 {
        player.step(&region, Movement::default(), 1.0 / 60.0);
    }
    assert!(player.grounded);
    assert!((player.position[1] - resting).abs() < 0.03);
}

#[test]
fn break_place_and_two_way_travel_preserve_terrain_inventory_and_player() {
    let mut game = flat_game();
    game.player.pitch = 0.0;
    game.regions[0].set([12, 2, 9], Block::Wood);
    let before = game.inventory.count(Block::Wood);
    let edit = game.break_target().unwrap();
    assert_eq!(edit.cell, [12, 2, 9]);
    assert_eq!(game.inventory.count(Block::Wood), before + 1);
    let position = game.player.position;
    game.travel(RegionId::Canyon);
    game.player.position[0] += 2.0;
    game.travel(RegionId::Meadow);
    assert_eq!(game.player.position, position);
    assert_eq!(game.region(RegionId::Meadow).get(edit.cell), Block::Air);
    game.regions[0].set([12, 2, 8], Block::Stone);
    assert!(game.inventory.select(Block::Wood));
    let placed = game.place_target().unwrap();
    assert_eq!(placed.cell, [12, 2, 9]);
    assert_eq!(game.inventory.count(Block::Wood), before);
    game.travel(RegionId::Canyon);
    assert_eq!(game.player.position[0], position[0] + 2.0);
    game.travel(RegionId::Meadow);
    assert_eq!(game.active_region().get(placed.cell), Block::Wood);
}

#[test]
fn rejected_edits_preserve_inventory_and_terrain() {
    let mut game = flat_game();
    game.player.pitch = 0.0;
    game.regions[0].set([12, 2, 9], Block::Wood);
    game.inventory.counts[Block::Wood as usize - 1] = Inventory::CAPACITY;
    let inventory = game.inventory.clone();
    let blocks = game.active_region().bootstrap_blocks();
    assert_eq!(game.break_target(), Err(EditError::InventoryFull));
    assert_eq!(game.inventory, inventory);
    assert_eq!(game.active_region().bootstrap_blocks(), blocks);
    game.inventory.counts[0] = 0;
    assert_eq!(game.place_target(), Err(EditError::InventoryEmpty));
    assert_eq!(game.active_region().bootstrap_blocks(), blocks);
    game.player.pitch = -1.45;
    assert_eq!(game.break_target(), Err(EditError::Bedrock));
}

#[test]
fn cannot_place_inside_player_or_beyond_world_bounds() {
    let mut game = flat_game();
    game.player.pitch = 0.0;
    game.regions[0].set([12, 2, 11], Block::Stone);
    assert_eq!(game.place_target(), Err(EditError::PlayerOverlap));
    game.player = Player::at([0.3, 1.02, 12.5]);
    game.player.pitch = 0.0;
    game.player.yaw = -std::f32::consts::FRAC_PI_2;
    game.regions[0].set([0, 2, 12], Block::Stone);
    // An origin inside a solid cell has no legal entry face.
    assert_eq!(game.place_target(), Err(EditError::Occupied));
}

#[test]
fn save_roundtrip_keeps_both_regions_and_rejects_corruption() {
    let mut original = SaveGame::new(123);
    original.regions[0].set([2, 9, 2], Block::Wood);
    original.regions[1].set([3, 10, 3], Block::Grass);
    original.travel(RegionId::Canyon);
    original.inventory.select(Block::Sand);
    let bytes = original.encode().unwrap();
    assert_eq!(bytes.len(), storage::FIXED_BYTES + 26);
    let restored = SaveGame::decode(&bytes).unwrap();
    assert_eq!(restored.active, RegionId::Canyon);
    assert_eq!(restored.inventory, original.inventory);
    assert_eq!(restored.player.position, original.player.position);
    for id in RegionId::ALL {
        assert_eq!(
            restored.region(id).bootstrap_blocks(),
            original.region(id).bootstrap_blocks()
        );
    }
    let mut corrupt = bytes.clone();
    corrupt[100] ^= 0x80;
    assert!(SaveGame::decode(&corrupt).is_err());
    assert!(SaveGame::decode(&bytes[..bytes.len() - 1]).is_err());
    let mut oversized = bytes;
    oversized.push(0);
    assert!(SaveGame::decode(&oversized).is_err());
}

#[test]
fn valid_checksum_does_not_bypass_payload_validation() {
    let original = SaveGame::new(1).encode().unwrap();
    for (offset, replacement) in [(16, 2), (17, 2), (18, 9), (19, 31), (20, 0), (21, 250)] {
        let mut bytes = original.clone();
        bytes[offset] = replacement;
        let end = bytes.len() - 8;
        let checksum = storage::checksum(&bytes[..end]);
        bytes[end..].copy_from_slice(&checksum.to_le_bytes());
        assert!(SaveGame::decode(&bytes).is_err());
    }
    let mut bytes = original;
    bytes[93..97].copy_from_slice(&f32::NAN.to_le_bytes());
    let end = bytes.len() - 8;
    let checksum = storage::checksum(&bytes[..end]);
    bytes[end..].copy_from_slice(&checksum.to_le_bytes());
    assert!(SaveGame::decode(&bytes).is_err());
}

#[test]
fn file_save_is_explicit_atomic_and_does_not_delete_a_foreign_temp_file() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "sim-logic-voxel-test-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("world.svx");
    let game = SaveGame::new(77);
    game.save(&path).unwrap();
    assert_eq!(SaveGame::load(&path).unwrap().seed, 77);
    let original_bytes = std::fs::read(&path).unwrap();
    let temporary = directory.join(format!("world.svx.{}.tmp", std::process::id()));
    std::fs::write(&temporary, b"foreign file").unwrap();
    assert!(SaveGame::new(99).save(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
    assert_eq!(std::fs::read(&temporary).unwrap(), b"foreign file");
    std::fs::remove_file(&temporary).unwrap();
    let mut invalid = game;
    invalid.player.position[0] = f32::NAN;
    assert!(invalid.save(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
