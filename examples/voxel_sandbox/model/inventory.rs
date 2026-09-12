use super::Block;

/// Nine assignable hotbar slots plus optional finite counts for the full palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Inventory {
    pub(super) counts: [u16; 32],
    pub(super) hotbar: [Block; 9],
    pub(super) selected_slot: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            counts: [32; 32],
            hotbar: [
                Block::Grass,
                Block::Stone,
                Block::Wood,
                Block::Sand,
                Block::Leaves,
                Block::Glass,
                Block::Bricks,
                Block::OakPlanks,
                Block::Lamp,
            ],
            selected_slot: 0,
        }
    }
}

impl Inventory {
    pub const CAPACITY: u16 = 999;
    pub fn count(&self, block: Block) -> u16 {
        if block.solid() {
            self.counts[block as usize - 1]
        } else {
            0
        }
    }
    pub fn hotbar(&self) -> [Block; 9] {
        self.hotbar
    }
    pub fn selected_slot(&self) -> usize {
        self.selected_slot
    }
    pub fn selected(&self) -> Block {
        self.hotbar[self.selected_slot]
    }
    pub fn select_slot(&mut self, slot: usize) -> bool {
        if slot >= self.hotbar.len() {
            return false;
        }
        self.selected_slot = slot;
        true
    }
    pub fn assign_slot(&mut self, slot: usize, block: Block) -> bool {
        if slot >= self.hotbar.len() || !block.solid() {
            return false;
        }
        self.hotbar[slot] = block;
        true
    }
    /// Select an existing slot or assign the requested block to the active slot.
    #[cfg(test)]
    pub fn select(&mut self, block: Block) -> bool {
        if !block.solid() {
            return false;
        }
        if let Some(slot) = self.hotbar.iter().position(|candidate| *candidate == block) {
            self.selected_slot = slot;
        } else {
            self.hotbar[self.selected_slot] = block;
        }
        true
    }
}
