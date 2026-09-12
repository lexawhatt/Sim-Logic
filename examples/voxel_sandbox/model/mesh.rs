use sim_engine::{Color, Mesh3d, Mesh3dAttributes, Mesh3dError, TextureCoordinate2d, Vec3};

use super::{Block, CHUNK_COUNT, CHUNK_SIZE, HEIGHT, Region, terrain::chunk_origin};

/// One nonempty material/shade batch, containing exposed faces only.
/// shade: 0 bottom, 1 vertical side, 2 top. Coordinates are world-space blocks.
pub struct MeshPart {
    pub block: Block,
    pub shade: u8,
    pub mesh: Mesh3d,
}

#[derive(Default)]
struct Faces {
    vertices: Vec<Vec3>,
    indices: Vec<u32>,
    uvs: Vec<TextureCoordinate2d>,
    normals: Vec<Vec3>,
    colors: Vec<Color>,
}

impl Region {
    /// At most 15 meshes and 3,072 quads per 8-cubed chunk. Neighbors across
    /// chunk boundaries suppress internal faces. Invalid chunk IDs yield no meshes.
    pub fn chunk_meshes(&self, chunk: usize) -> Result<Vec<MeshPart>, Mesh3dError> {
        if chunk >= CHUNK_COUNT {
            return Ok(Vec::new());
        }
        let origin = chunk_origin(chunk);
        let mut batches: Vec<Faces> = (0..15).map(|_| Faces::default()).collect();
        for y in origin[1]..origin[1] + CHUNK_SIZE {
            for z in origin[2]..origin[2] + CHUNK_SIZE {
                for x in origin[0]..origin[0] + CHUNK_SIZE {
                    let block = self.get([x, y, z]);
                    if !block.solid() {
                        continue;
                    }
                    for face in FACES {
                        let neighbor = [x + face.normal[0], y + face.normal[1], z + face.normal[2]];
                        // Collision solidity is not render occlusion: leaf
                        // masks have holes, so geometry behind them must remain.
                        let neighbor = self.get(neighbor);
                        if neighbor.solid() && neighbor != Block::Leaves {
                            continue;
                        }
                        let batch =
                            &mut batches[(block as usize - 1) * 3 + usize::from(face.shade)];
                        let first = batch.vertices.len() as u32;
                        batch.vertices.extend(face.corners.map(|corner| {
                            // Coordinates are integer grid corners within [0, 24].
                            Vec3::new(
                                x as f32 + corner[0],
                                y as f32 + corner[1],
                                z as f32 + corner[2],
                            )
                            .expect("bounded voxel grid corner")
                        }));
                        let normal = Vec3::new(
                            face.normal[0] as f32,
                            face.normal[1] as f32,
                            face.normal[2] as f32,
                        )
                        .expect("constant unit face normal");
                        batch.normals.extend([normal; 4]);
                        batch
                            .uvs
                            .extend([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]].map(
                                |[u, v]| {
                                    TextureCoordinate2d::new(u, v).expect("normalized face UV")
                                },
                            ));
                        // A small host-owned height tint exercises interpolated
                        // vertex colors without baking a second lighting model.
                        // Block/material color is still chosen during projection.
                        batch.colors.extend(face.corners.map(|corner| {
                            let tint = 0.94 + (y as f32 + corner[1]) / HEIGHT as f32 * 0.06;
                            Color::rgb(tint, tint, tint)
                        }));
                        batch.indices.extend([
                            first,
                            first + 1,
                            first + 2,
                            first,
                            first + 2,
                            first + 3,
                        ]);
                    }
                }
            }
        }
        let mut parts = Vec::new();
        for (index, batch) in batches.into_iter().enumerate() {
            if !batch.indices.is_empty() {
                parts.push(MeshPart {
                    block: Block::SOLID[index / 3],
                    shade: (index % 3) as u8,
                    mesh: Mesh3d::with_attributes(
                        batch.vertices,
                        batch.indices,
                        Vec::new(),
                        Mesh3dAttributes::new()
                            .with_texture_coordinates(batch.uvs)
                            .with_vertex_colors(batch.colors)?
                            .with_normals(batch.normals)?,
                    )?,
                });
            }
        }
        Ok(parts)
    }
}

#[derive(Clone, Copy)]
struct Face {
    normal: [i32; 3],
    shade: u8,
    corners: [[f32; 3]; 4],
}

// Counter-clockwise viewed from outside. No duplicate interior triangles.
const FACES: [Face; 6] = [
    Face {
        normal: [0, -1, 0],
        shade: 0,
        corners: [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
    },
    Face {
        normal: [0, 1, 0],
        shade: 2,
        corners: [
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ],
    },
    Face {
        normal: [-1, 0, 0],
        shade: 1,
        corners: [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
        ],
    },
    Face {
        normal: [1, 0, 0],
        shade: 1,
        corners: [
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
        ],
    },
    Face {
        normal: [0, 0, -1],
        shade: 1,
        corners: [
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ],
    },
    Face {
        normal: [0, 0, 1],
        shade: 1,
        corners: [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
    },
];
