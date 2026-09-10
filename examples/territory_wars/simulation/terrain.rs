//! Integer-only island generation. Keep the largest connected land component so
//! no surviving rival can become unreachable without a naval implementation.

use super::{CELLS, HEIGHT, NEUTRAL, WATER, WIDTH, neighbors};

pub(super) fn island(seed: u64) -> Vec<u8> {
    let mut owners = vec![WATER; CELLS];
    for y in 2..HEIGHT - 2 {
        for x in 2..WIDTH - 2 {
            let dx = x as i64 * 2 - WIDTH as i64;
            let dy = y as i64 * 2 - HEIGHT as i64;
            let radius = dx * dx * 10_000 / (WIDTH as i64 - 7).pow(2)
                + dy * dy * 10_000 / (HEIGHT as i64 - 7).pow(2);
            let broad = noise(seed, x, y, 15);
            let coast = noise(seed.wrapping_add(91), x, y, 5) / 3;
            if radius + broad + coast < 9_200 {
                owners[y * WIDTH + x] = NEUTRAL;
            }
        }
    }
    let mut visited = vec![false; CELLS];
    let mut queue = Vec::with_capacity(CELLS);
    let mut largest = Vec::with_capacity(CELLS);
    for start in 0..CELLS {
        if owners[start] != NEUTRAL || visited[start] {
            continue;
        }
        queue.clear();
        queue.push(start);
        visited[start] = true;
        let mut cursor = 0;
        while cursor < queue.len() {
            let cell = queue[cursor];
            cursor += 1;
            for adjacent in neighbors(cell).into_iter().flatten() {
                if owners[adjacent] == NEUTRAL && !visited[adjacent] {
                    visited[adjacent] = true;
                    queue.push(adjacent);
                }
            }
        }
        if queue.len() > largest.len() {
            largest.clear();
            largest.extend_from_slice(&queue);
        }
    }
    owners.fill(WATER);
    for cell in largest {
        owners[cell] = NEUTRAL;
    }
    owners
}

fn noise(seed: u64, x: usize, y: usize, scale: usize) -> i64 {
    let scale = scale as i64;
    let x = x as i64;
    let y = y as i64;
    let gx = x / scale;
    let gy = y / scale;
    let fx = x % scale;
    let fy = y % scale;
    let top = hash(seed, gx, gy) * (scale - fx) + hash(seed, gx + 1, gy) * fx;
    let bottom = hash(seed, gx, gy + 1) * (scale - fx) + hash(seed, gx + 1, gy + 1) * fx;
    (top * (scale - fy) + bottom * fy) / (scale * scale)
}

fn hash(seed: u64, x: i64, y: i64) -> i64 {
    let mut value = seed
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((value ^ (value >> 31)) % 5_001) as i64 - 2_500
}
