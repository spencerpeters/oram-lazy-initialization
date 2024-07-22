// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! A trait representing a Path ORAM stash.

use super::{Bucket, PathOramBlock};
use crate::{
    database::Database,
    utils::{bitonic_sort_by_keys, CompleteBinaryTreeIndex, TreeIndex},
    Address, BucketSize, OramBlock, OramError,
};

/// Numeric type used to represent the size of a Path ORAM stash in blocks.
pub type StashSize = u64;

/// A Path ORAM stash.
pub trait Stash<V: OramBlock>
where
    Self: Sized,
{
    /// Creates a new stash capable of holding `capacity` blocks.
    fn new(path_size: StashSize, overflow_size: StashSize) -> Result<Self, OramError>;
    /// Read blocks from the path specified by the leaf `position` in the tree `physical_memory` of height `height`
    fn read_from_path<const Z: BucketSize, T: Database<Bucket<V, Z>>>(
        &mut self,
        physical_memory: &mut T,
        position: TreeIndex,
    ) -> Result<(), OramError>;
    /// Evict blocks from the stash to the path specified by the leaf `position` in the tree `physical_memory` of height `height`.
    fn write_to_path<const Z: BucketSize, T: Database<Bucket<V, Z>>>(
        &mut self,
        physical_memory: &mut T,
        position: TreeIndex,
    ) -> Result<(), OramError>;
    /// Obliviously scans the stash for a block `b` with address `address`; if found, replaces that block with `callback(b)`.
    fn access<F: Fn(&V) -> V>(
        &mut self,
        address: Address,
        new_position: TreeIndex,
        value_callback: F,
    ) -> Result<V, OramError>;

    #[cfg(test)]
    /// The number of real (non-dummy) blocks in the stash.
    fn occupancy(&self) -> StashSize;
}

use subtle::{Choice, ConditionallySelectable, ConstantTimeEq};

#[derive(Debug)]
/// A fixed-size, obliviously accessed Path ORAM stash data structure implemented using oblivious sorting.
pub struct BitonicStash<V: OramBlock> {
    blocks: Vec<PathOramBlock<V>>,
    path_size: StashSize,
}

impl<V: OramBlock> BitonicStash<V> {
    fn len(&self) -> usize {
        self.blocks.len()
    }
}

impl<V: OramBlock> Stash<V> for BitonicStash<V> {
    fn new(path_size: StashSize, overflow_size: StashSize) -> Result<Self, OramError> {
        let num_stash_blocks: usize = (path_size + overflow_size).try_into()?;
        if !(num_stash_blocks.is_power_of_two()) {
            return Err(OramError::InvalidConfigurationError);
        }

        Ok(Self {
            blocks: vec![PathOramBlock::<V>::dummy(); num_stash_blocks],
            path_size,
        })
    }

    fn write_to_path<const Z: BucketSize, T: Database<Bucket<V, Z>>>(
        &mut self,
        physical_memory: &mut T,
        position: TreeIndex,
    ) -> Result<(), OramError> {
        let height = position.ct_depth();

        let mut level_assignments = vec![TreeIndex::MAX; self.len()];
        let mut level_counts = vec![0; usize::try_from(height)? + 1];

        for (i, block) in self.blocks.iter().enumerate() {
            // If `block` is a dummy, the rest of this loop iteration will be a no-op, and the values don't matter.
            let block_is_dummy = block.ct_is_dummy();

            // Set up valid but meaningless input to the computation in case `block` is a dummy.
            let an_arbitrary_leaf: TreeIndex = 1 << height;
            let block_position =
                TreeIndex::conditional_select(&block.position, &an_arbitrary_leaf, block_is_dummy);

            let block_level: u64 = block_position
                .ct_common_ancestor_of_two_leaves(position)
                .ct_depth();

            // Obliviously scan through the buckets, assigning the block to the correct one, or to the overflow.
            for (level, count) in level_counts.iter_mut().enumerate() {
                let block_level_bucket_full: Choice = count.ct_eq(&(u64::try_from(Z)?));
                let correct_level = level.ct_eq(&(usize::try_from(block_level))?);

                // If the bucket `block` should go in is full, assign the block to the overflow.
                let should_overflow = correct_level & block_level_bucket_full & (!block_is_dummy);
                level_assignments[i].conditional_assign(&(TreeIndex::MAX - 1), should_overflow);

                // Otherwise, assign
                let should_assign = correct_level & (!block_level_bucket_full) & (!block_is_dummy);
                let level_count_incremented = *count + 1;
                count.conditional_assign(&level_count_incremented, should_assign);
                level_assignments[i].conditional_assign(&block_level, should_assign);
            }
        }

        // Assign dummy blocks to the remaining non-full buckets until all buckets are full.
        for (i, block) in self.blocks.iter().enumerate() {
            let block_free = block.ct_is_dummy();

            let mut assigned: Choice = 0.into();
            for (level, count) in level_counts.iter_mut().enumerate() {
                let full = count.ct_eq(&(u64::try_from(Z)?));
                let no_op = assigned | full | !block_free;

                level_assignments[i].conditional_assign(&(u64::try_from(level))?, !no_op);
                count.conditional_assign(&(*count + 1), !no_op);
                assigned |= !no_op;
            }
        }

        bitonic_sort_by_keys(&mut self.blocks, &mut level_assignments);

        // Write the first Z * height blocks into slots in the tree
        for depth in 0..=height {
            let mut new_bucket: Bucket<V, Z> = Bucket::default();

            for slot_number in 0..Z {
                let stash_index = (usize::try_from(depth)?) * Z + slot_number;

                new_bucket.blocks[slot_number] = self.blocks[stash_index];
            }

            physical_memory.write_db(position.node_on_path(depth, height), new_bucket)?;
        }

        Ok(())
    }

    fn access<F: Fn(&V) -> V>(
        &mut self,
        address: Address,
        new_position: TreeIndex,
        value_callback: F,
    ) -> Result<V, OramError> {
        let mut result: V = V::default();

        for block in &mut self.blocks {
            let is_requested_index = block.address.ct_eq(&address);

            // Read current value of target block into `result`.
            result.conditional_assign(&block.value, is_requested_index);

            // Write new position into target block.
            block
                .position
                .conditional_assign(&new_position, is_requested_index);

            // If a write, write new value into target block.
            let value_to_write = value_callback(&result);

            block
                .value
                .conditional_assign(&value_to_write, is_requested_index);
        }
        Ok(result)
    }

    #[cfg(test)]
    fn occupancy(&self) -> StashSize {
        let mut result = 0;
        for i in self.path_size.try_into().unwrap()..(self.blocks.len()) {
            if !self.blocks[i].is_dummy() {
                result += 1;
            }
        }
        result
    }

    fn read_from_path<const Z: crate::BucketSize, T: crate::database::Database<Bucket<V, Z>>>(
        &mut self,
        physical_memory: &mut T,
        position: TreeIndex,
    ) -> Result<(), OramError> {
        let height = position.ct_depth();

        for i in (0..(self.path_size / u64::try_from(Z)?)).rev() {
            let bucket_index = position.node_on_path(i, height);
            let bucket = physical_memory.read_db(bucket_index)?;
            for slot_index in 0..Z {
                self.blocks[Z * (usize::try_from(i)?) + slot_index] = bucket.blocks[slot_index];
            }
        }

        Ok(())
    }
}
