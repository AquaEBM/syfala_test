#![cfg_attr(not(feature = "std"), no_std)]

pub mod queue;

mod sample_type;

pub use sample_type::*;

mod byte_consumer;
pub use byte_consumer::*;

mod byte_producer;
pub use byte_producer::*;

// TODO: This crate is in desperate need of tests

extern crate alloc;