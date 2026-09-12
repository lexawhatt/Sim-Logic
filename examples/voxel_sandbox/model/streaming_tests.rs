use super::*;

fn fill(region: &mut Region, position: [f32; 3]) {
    for _ in 0..40 {
        let before = region.generated_chunks();
        region.stream_around(position).unwrap();
        assert!(region.generated_chunks() - before <= terrain::GENERATIONS_PER_FRAME as u64);
        assert!(region.resident_count() <= MAX_RESIDENT_CHUNKS);
        if region.pending_chunks() == 0 {
            return;
        }
    }
    panic!("bounded resident loading did not complete");
}

#[test]
fn signed_chunk_ids_roundtrip_and_keep_original_bootstrap_coordinates() {
    for id in 0..CHUNK_COUNT {
        let origin = chunk_origin(id).unwrap();
        assert_eq!(terrain::chunk_index(origin), id);
    }
    for x in [-4096, -33, -9, -8, -1, 0, 7, 8, 23, 24, 127, 4095] {
        for z in [-4096, -9, -1, 0, 8, 24, 4095] {
            for y in [0, 7, 8, 15] {
                let id = terrain::chunk_index([x, y, z]);
                assert_eq!(
                    chunk_origin(id),
                    Some([
                        x.div_euclid(8) * 8,
                        y.div_euclid(8) * 8,
                        z.div_euclid(8) * 8
                    ])
                );
            }
        }
    }
    assert_eq!(chunk_origin(usize::MAX), None);
}

#[test]
fn streaming_is_bounded_and_negative_edits_survive_eviction_and_revisit() {
    let mut region = Region::generated(RegionId::Meadow, 42);
    let cell = [-17, 12, -9];
    let original = region.get(cell);
    assert_eq!(original, Block::Air);
    region.try_set(cell, Block::Glass).unwrap();
    fill(&mut region, [-16.5, 10.0, -8.5]);
    assert_eq!(region.resident_count(), MAX_RESIDENT_CHUNKS);
    let id = terrain::chunk_index(cell);
    let first_revision = region.chunk_revision(id).unwrap();
    assert_eq!(region.get(cell), Block::Glass);
    fill(&mut region, [500.5, 10.0, 500.5]);
    assert_eq!(region.chunk_revision(id), None);
    assert_eq!(
        region.get(cell),
        Block::Glass,
        "unloaded canonical state stays readable"
    );
    assert_eq!(region.edit_count(), 1);
    fill(&mut region, [-16.5, 10.0, -8.5]);
    assert!(region.chunk_revision(id).unwrap() > first_revision);
    assert_eq!(region.get(cell), Block::Glass);
    region.try_set(cell, original).unwrap();
    assert_eq!(
        region.edit_count(),
        0,
        "restoring generated value removes redundant edit"
    );
    let expected = region.get([-16, 1, -9]);
    let mut fresh = Region::generated(RegionId::Meadow, 42);
    fill(&mut fresh, [-16.5, 10.0, -8.5]);
    assert_eq!(fresh.get([-16, 1, -9]), expected);
}

#[test]
fn unloaded_collision_and_cross_chunk_face_sampling_match_loaded_state() {
    let mut region = Region::generated(RegionId::Canyon, 77);
    let cell = [-100, 0, 200];
    assert!(region.collision_cell(cell));
    let before = region.get([-100, 2, 200]);
    fill(&mut region, [-100.5, 12.0, 200.5]);
    assert_eq!(region.get([-100, 2, 200]), before);
    let selected = region.desired_chunks()[0];
    let first = region.chunk_meshes(selected).unwrap();
    assert!(!first.is_empty());
    for part in first {
        for vertex in part.mesh.vertices() {
            assert!(vertex.x() < 0.0);
            assert!(vertex.z() > 100.0);
        }
    }
    assert!(region.collision_cell([-WORLD_LIMIT - 1, 12, 0]));
    assert!(!Region::contains([0, HEIGHT, 0]));
    let count = region.resident_count();
    assert!(region.stream_around([f32::NAN, 0.0, 0.0]).is_err());
    assert!(region.stream_around([f32::MAX, 0.0, 0.0]).is_err());
    assert_eq!(region.resident_count(), count);
}

#[test]
fn sparse_edit_and_revision_limits_reject_before_mutation() {
    let mut region = Region::generated(RegionId::Meadow, 9);
    for z in 0..128 {
        for x in 0..128 {
            region.try_set([x, 12, z], Block::Bricks).unwrap();
        }
    }
    assert_eq!(region.edit_count(), terrain::MAX_EDITS);
    assert_eq!(
        region.try_set([-1, 12, 0], Block::Bricks),
        Err(EditError::EditLimit)
    );
    assert_eq!(region.get([-1, 12, 0]), Block::Air);
    region.try_set([0, 12, 0], Block::Air).unwrap();
    region.try_set([-1, 12, 0], Block::Bricks).unwrap();
    assert_eq!(region.edit_count(), terrain::MAX_EDITS);
}

#[test]
fn nine_hotbar_slots_accept_the_full_palette_and_invalid_values_are_atomic() {
    let mut inventory = Inventory::default();
    assert_eq!(Block::SOLID.len(), 32);
    assert_eq!(inventory.hotbar().len(), 9);
    for (index, block) in Block::SOLID.into_iter().enumerate() {
        assert!(inventory.assign_slot(index % 9, block));
        assert!(inventory.select_slot(index % 9));
        assert_eq!(inventory.selected(), block);
    }
    let original = inventory.clone();
    assert!(!inventory.assign_slot(9, Block::Glass));
    assert!(!inventory.assign_slot(0, Block::Air));
    assert!(!inventory.select_slot(9));
    assert_eq!(inventory, original);
}

#[test]
fn sparse_save_roundtrip_keeps_unloaded_edits_both_worlds_and_hotbar() {
    let mut game = SaveGame::new(5);
    game.regions[0]
        .try_set([-1000, 12, -1000], Block::GoldOre)
        .unwrap();
    game.regions[1]
        .try_set([999, 11, 888], Block::BlueGlass)
        .unwrap();
    game.inventory.assign_slot(8, Block::Obsidian);
    game.inventory.select_slot(8);
    game.player = Player::at([-600.5, 30.0, -600.5]);
    game.stream_active().unwrap();
    let bytes = game.encode().unwrap();
    assert_eq!(&bytes[..8], b"SVXLS002");
    assert!(bytes.len() <= storage::MAX_SAVE_BYTES);
    let restored = SaveGame::decode(&bytes).unwrap();
    assert_eq!(restored.encode().unwrap(), bytes);
    assert_eq!(restored.regions[0].get([-1000, 12, -1000]), Block::GoldOre);
    assert_eq!(restored.regions[1].get([999, 11, 888]), Block::BlueGlass);
    assert_eq!(restored.inventory, game.inventory);
    assert_eq!(restored.player, game.player);
    assert!(restored.creative);
    assert!(
        SaveGame::decode(b"SVXLS001")
            .unwrap_err()
            .to_string()
            .contains("legacy")
    );
}

#[test]
fn flight_is_creative_only_bounded_and_does_not_accelerate_diagonals() {
    let mut game = SaveGame::new(123);
    game.player = Player::at([12.5, 20.0, 12.5]);
    let start = game.player;
    game.creative = false;
    assert!(!game.fly_step(Movement::default(), 1.0, 0.1));
    assert_eq!(game.player, start);
    game.creative = true;
    assert!(!game.fly_step(Movement::default(), f32::NAN, 0.1));
    assert_eq!(game.player, start);
    assert!(game.fly_step(
        Movement {
            forward: 1.0,
            strafe: 1.0,
            jump: false
        },
        1.0,
        0.1
    ));
    let distance = game
        .player
        .position
        .into_iter()
        .zip(start.position)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt();
    assert!((distance - 0.7).abs() < 0.001);
    game.player = Player::at([12.5, 20.0, 12.5]);
    assert!(game.fly_step(Movement::default(), -1.0, 0.1));
    assert!((game.player.position[1] - 19.3).abs() < 0.001);
    for _ in 0..100 {
        game.fly_step(Movement::default(), 1.0, 0.1);
    }
    assert!(game.player.position[1] <= Player::MAX_ALTITUDE);
    assert!(game.player.position[1] > Player::MAX_ALTITUDE - 0.001);
    let before = game.player.position;
    game.fly_step(Movement::default(), 0.0, 0.1);
    assert_eq!(game.player.position, before, "hover does not apply gravity");
    assert!(
        game.encode().is_ok(),
        "airborne position is a supported save"
    );
}

#[test]
fn creative_flight_still_collides_with_buildings() {
    let mut game = SaveGame::new(42);
    game.player = Player::at([12.5, 11.0, 12.5]);
    game.regions[0].try_set([12, 11, 11], Block::Glass).unwrap();
    game.regions[0].try_set([12, 12, 11], Block::Glass).unwrap();
    for _ in 0..10 {
        game.fly_step(
            Movement {
                forward: 1.0,
                strafe: 0.0,
                jump: false,
            },
            0.0,
            0.1,
        );
    }
    assert!(game.player.position[2] >= 12.0 + Player::RADIUS - 0.001);
    assert!(!game.player.collides(game.active_region()));
}
