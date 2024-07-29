// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! An implementation of Oblivious RAM (ORAM) for the secure enclave setting.
//!
//! # Overview
//!
//! This crate implements an oblivious RAM protocol (ORAM) for (secure) enclave applications.
//!
//! This crate assumes that ORAM clients are running inside a secure enclave architecture that provides memory encryption.
//! It does not perform encryption-on-write and thus is **not** secure without memory encryption.
//!
//! # Design
//!
//! This crate implements the Path ORAM protocol, with oblivious
//! client data structures based on the [Oblix paper](https://people.eecs.berkeley.edu/~raluca/oblix.pdf).
//! See the [Path ORAM retrospective paper](http://elaineshi.com/docs/pathoram-retro.pdf)
//! for a high-level introduction to ORAM and Path ORAM, and for more detailed references.
//!
//! # Example
//!
//! The below example reads a database from memory into an ORAM to permit secret-dependent access to it.
//!
//! ```
//! use oram::{Address, BlockSize, BlockValue, Oram, DefaultOram};
//! # use oram::OramError;
//!
//! const BLOCK_SIZE: BlockSize = 64;
//! const DB_SIZE: Address = 64;
//! # const DATABASE: [[u8; BLOCK_SIZE as usize]; DB_SIZE as usize] =
//! # [[0; BLOCK_SIZE as usize]; DB_SIZE as usize];
//! let mut rng = rand::rngs::OsRng;
//!
//! // Initialize `oram` to store 64 blocks of 64 bytes each.
//! let mut oram = DefaultOram::<BlockValue<BLOCK_SIZE>>::new(DB_SIZE, &mut rng)?;
//!
//! // Read DATABASE into oram.
//! for (i, bytes) in DATABASE.iter().enumerate() {
//!     oram.write(i as Address, BlockValue::new(*bytes), &mut rng)?;
//! }
//!
//! // Now you can safely make secret-dependent accesses to your database.
//! let secret = 42;
//! let _ = oram.read(secret, &mut rng)?;
//! # Ok::<(), OramError>(())
//! ```

#![warn(clippy::cargo, clippy::doc_markdown, missing_docs, rustdoc::all)]

use std::num::TryFromIntError;

use rand::{CryptoRng, RngCore};
use subtle::ConditionallySelectable;
use thiserror::Error;

pub(crate) mod bucket;
pub(crate) mod database;
pub(crate) mod linear_time_oram;
pub mod path_oram;
pub(crate) mod position_map;
pub(crate) mod stash;
#[cfg(test)]
mod test_utils;
pub(crate) mod utils;

pub use crate::bucket::BlockValue;
pub use crate::path_oram::DefaultOram;
pub use crate::path_oram::PathOram;

/// The numeric type used to specify the size of an ORAM block in bytes.
pub type BlockSize = usize;
/// The numeric type used to specify the size of an ORAM in blocks, and to index into the ORAM.
pub type Address = u64;
/// The numeric type used to specify the size of an ORAM bucket in blocks.
pub type BucketSize = usize;

/// A "trait alias" for ORAM blocks: the values read and written by ORAMs.
pub trait OramBlock:
    Copy + Clone + std::fmt::Debug + Default + PartialEq + ConditionallySelectable
{
}

impl OramBlock for u8 {}
impl OramBlock for u16 {}
impl OramBlock for u32 {}
impl OramBlock for u64 {}
impl OramBlock for i8 {}
impl OramBlock for i16 {}
impl OramBlock for i32 {}
impl OramBlock for i64 {}

/// A list of error types which are produced during ORAM protocol execution.
#[derive(Error, Debug)]
pub enum OramError {
    /// Errors arising from conversions between integer types.
    #[error("Arithmetic error encountered.")]
    IntegerConversionError(#[from] TryFromIntError),
    /// Errors arising from attempting to make an ORAM access to an invalid address.
    #[error("Attempted to access an out-of-bounds ORAM address.")]
    AddressOutOfBoundsError,
    /// Errors arising from invalid parameters or configuration.
    #[error("Invalid configuration.")]
    InvalidConfigurationError,
}

/// Represents an oblivious RAM (ORAM) mapping addresses of type `Address` to values of type `V: OramBlock`.
pub trait Oram<V: OramBlock>
where
    Self: Sized,
{
    /// Returns a new ORAM mapping addresses `0 <= address < block_capacity` to default `V` values.
    ///
    /// # Errors
    ///
    /// If `block_capacity` is not a power of two, returns an `InvalidConfigurationError`.
    fn new<R: RngCore + CryptoRng>(block_capacity: Address, rng: &mut R)
        -> Result<Self, OramError>;

    /// Returns the capacity in blocks of this ORAM.
    fn block_capacity(&self) -> Result<Address, OramError>;

    /// Performs a (oblivious) ORAM access.
    /// Returns the value `v` previously stored at `index`, and writes `callback(v)` to `index`.
    ///
    /// For updating a block in place, using `access` is expected to be about
    /// twice as fast as performing a `read` followed by a `write`.
    fn access<R: RngCore + CryptoRng, F: Fn(&V) -> V>(
        &mut self,
        index: Address,
        callback: F,
        rng: &mut R,
    ) -> Result<V, OramError>;

    /// Obliviously reads the value stored at `index`.
    fn read<R: RngCore + CryptoRng>(
        &mut self,
        index: Address,
        rng: &mut R,
    ) -> Result<V, OramError> {
        log::debug!("ORAM read: {}", index);
        let callback = |x: &V| *x;
        self.access(index, callback, rng)
    }

    /// Obliviously writes the value stored at `index`. Returns the value previously stored at `index`.
    fn write<R: RngCore + CryptoRng>(
        &mut self,
        index: Address,
        new_value: V,
        rng: &mut R,
    ) -> Result<V, OramError> {
        log::debug!("ORAM write: {}", index);
        let callback = |_: &V| new_value;
        self.access(index, callback, rng)
    }
}
