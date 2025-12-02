use core::{convert::Infallible, iter, num, mem};
use std::{
    // TODO: choose a better hasher
    collections::hash_map::{Entry, HashMap},
    io,
    thread,
};
use syfala_net::{AudioConfig, network, queue, rtrb};

pub mod client;
mod interleaver;
pub mod server;
