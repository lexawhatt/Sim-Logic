//! Original block palette; the first five saved discriminants remain unchanged.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Block {
    Air,
    Grass,
    Stone,
    Wood,
    Sand,
    Leaves,
    Dirt,
    Cobblestone,
    Bricks,
    OakPlanks,
    DarkPlanks,
    Glass,
    BlueGlass,
    Snow,
    Ice,
    Clay,
    RedClay,
    Gravel,
    CoalOre,
    IronOre,
    GoldOre,
    CopperOre,
    Slate,
    Marble,
    Basalt,
    Moss,
    Sandstone,
    Obsidian,
    Red,
    Blue,
    Yellow,
    White,
    Lamp,
}

impl Block {
    pub const SOLID: [Self; 32] = [
        Self::Grass,
        Self::Stone,
        Self::Wood,
        Self::Sand,
        Self::Leaves,
        Self::Dirt,
        Self::Cobblestone,
        Self::Bricks,
        Self::OakPlanks,
        Self::DarkPlanks,
        Self::Glass,
        Self::BlueGlass,
        Self::Snow,
        Self::Ice,
        Self::Clay,
        Self::RedClay,
        Self::Gravel,
        Self::CoalOre,
        Self::IronOre,
        Self::GoldOre,
        Self::CopperOre,
        Self::Slate,
        Self::Marble,
        Self::Basalt,
        Self::Moss,
        Self::Sandstone,
        Self::Obsidian,
        Self::Red,
        Self::Blue,
        Self::Yellow,
        Self::White,
        Self::Lamp,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Air => "Air",
            Self::Grass => "Grass",
            Self::Stone => "Stone",
            Self::Wood => "Wood",
            Self::Sand => "Sand",
            Self::Leaves => "Leaves",
            Self::Dirt => "Dirt",
            Self::Cobblestone => "Cobble",
            Self::Bricks => "Bricks",
            Self::OakPlanks => "Oak planks",
            Self::DarkPlanks => "Dark planks",
            Self::Glass => "Glass",
            Self::BlueGlass => "Blue glass",
            Self::Snow => "Snow",
            Self::Ice => "Ice",
            Self::Clay => "Clay",
            Self::RedClay => "Red clay",
            Self::Gravel => "Gravel",
            Self::CoalOre => "Coal ore",
            Self::IronOre => "Iron ore",
            Self::GoldOre => "Gold ore",
            Self::CopperOre => "Copper ore",
            Self::Slate => "Slate",
            Self::Marble => "Marble",
            Self::Basalt => "Basalt",
            Self::Moss => "Moss",
            Self::Sandstone => "Sandstone",
            Self::Obsidian => "Obsidian",
            Self::Red => "Red block",
            Self::Blue => "Blue block",
            Self::Yellow => "Yellow block",
            Self::White => "White block",
            Self::Lamp => "Lamp",
        }
    }

    pub fn color(self, shade: u8) -> sim_engine::Color {
        let rgb = match self {
            Self::Air => [0.0, 0.0, 0.0],
            Self::Grass => [0.28, 0.64, 0.18],
            Self::Stone => [0.48, 0.53, 0.60],
            Self::Wood => [0.50, 0.27, 0.11],
            Self::Sand => [0.82, 0.60, 0.29],
            Self::Leaves => [0.14, 0.43, 0.16],
            Self::Dirt => [0.45, 0.27, 0.14],
            Self::Cobblestone => [0.42, 0.45, 0.49],
            Self::Bricks => [0.67, 0.24, 0.16],
            Self::OakPlanks => [0.73, 0.48, 0.23],
            Self::DarkPlanks => [0.26, 0.15, 0.09],
            Self::Glass => [0.78, 0.92, 0.97],
            Self::BlueGlass => [0.18, 0.42, 0.86],
            Self::Snow => [0.91, 0.95, 1.0],
            Self::Ice => [0.42, 0.73, 0.92],
            Self::Clay => [0.55, 0.61, 0.66],
            Self::RedClay => [0.75, 0.30, 0.16],
            Self::Gravel => [0.55, 0.51, 0.47],
            Self::CoalOre => [0.20, 0.22, 0.24],
            Self::IronOre => [0.70, 0.49, 0.35],
            Self::GoldOre => [0.92, 0.71, 0.17],
            Self::CopperOre => [0.72, 0.43, 0.24],
            Self::Slate => [0.27, 0.32, 0.40],
            Self::Marble => [0.86, 0.85, 0.81],
            Self::Basalt => [0.20, 0.21, 0.24],
            Self::Moss => [0.26, 0.39, 0.12],
            Self::Sandstone => [0.81, 0.68, 0.41],
            Self::Obsidian => [0.13, 0.08, 0.19],
            Self::Red => [0.85, 0.13, 0.13],
            Self::Blue => [0.13, 0.30, 0.84],
            Self::Yellow => [0.95, 0.81, 0.14],
            Self::White => [0.94, 0.94, 0.90],
            Self::Lamp => [1.0, 0.86, 0.41],
        };
        let shade = match shade {
            0 => 0.58,
            1 => 0.82,
            _ => 1.0,
        };
        sim_engine::Color::rgb(rgb[0] * shade, rgb[1] * shade, rgb[2] * shade)
    }

    pub const fn solid(self) -> bool {
        !matches!(self, Self::Air)
    }
    pub const fn translucent(self) -> bool {
        matches!(self, Self::Glass | Self::BlueGlass | Self::Ice)
    }
    pub const fn occludes(self) -> bool {
        !matches!(
            self,
            Self::Air | Self::Leaves | Self::Glass | Self::BlueGlass | Self::Ice
        )
    }

    pub(super) fn from_byte(value: u8) -> Option<Self> {
        if value == 0 {
            Some(Self::Air)
        } else {
            Self::SOLID.get(value as usize - 1).copied()
        }
    }
}
