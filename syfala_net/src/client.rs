use super::*;

const DISCOVERY_MESSAGE: [u8; 21] = *b"SYFALASERVERDISCOVERY";

pub mod discovery {
    use super::*;

    #[inline(always)]
    pub fn send_discovery_packet(
        addr: impl std::net::ToSocketAddrs,
        _audio_addr: core::net::SocketAddrV4,
        socket: &std::net::UdpSocket,
    ) -> io::Result<()> {
        // TODO: send audio_addr
        if socket.send_to(&DISCOVERY_MESSAGE, addr)? != DISCOVERY_MESSAGE.len() {
            Err(io::ErrorKind::Other.into())
        } else {
            Ok(())
        }
    }

    #[inline(always)]
    pub fn accept_config(
        socket: &std::net::UdpSocket,
    ) -> io::Result<(core::net::SocketAddrV4, AudioConfig)> {
        // TODO: validate and parse message
        let _bytes_read = socket.recv(&mut [])?;

        Ok((
            "127.0.0.1:6910".parse().unwrap(),
            AudioConfig::new(num::NonZeroUsize::MIN, num::NonZeroUsize::MIN),
        ))
    }
}

const MAX_DATAGRAM_SIZE: num::NonZeroUsize = nz(1452);

const MAX_SAMPLE_DATA_PER_DATAGRAM: num::NonZeroUsize =
    nz(MAX_DATAGRAM_SIZE.get().strict_sub(size_of::<u64>()));

const MAX_SPLS_PER_DATAGRAM: num::NonZeroUsize =
    nz(MAX_SAMPLE_DATA_PER_DATAGRAM.get() / SAMPLE_SIZE.get());

pub struct Sender {
    current_timestamp_spls: u64,
    // TODO: this buffer is meant to be copied into another byte buffer,
    // (packed with some metadata) which is what gets sent over the network.
    //
    // Can we avoid that extra copy?
    scratch_buffer: arrayvec::ArrayVec<Sample, { MAX_SPLS_PER_DATAGRAM.get() }>,
}

impl Sender {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            current_timestamp_spls: 0,
            scratch_buffer: arrayvec::ArrayVec::new_const(),
        }
    }

    #[inline(always)]
    pub const fn current_timestamp_samples(&self) -> u64 {
        self.current_timestamp_spls
    }

    #[inline(always)]
    pub fn n_remaining_chunk_samples(
        &self,
        chunk_size_samples: num::NonZeroUsize,
    ) -> num::NonZeroUsize {
        let current_timestamp = self.current_timestamp_samples() as usize + self.n_stored_samples();
        // Never zero, we always flush at chunk boundaries, or as soon as the buffer is full
        num::NonZeroUsize::new(
            chunk_size_samples
                .get()
                .strict_sub(current_timestamp % chunk_size_samples)
                .min(self.scratch_buffer.remaining_capacity()),
        )
        .unwrap()
    }

    #[inline(always)]
    pub fn n_stored_samples(&self) -> usize {
        self.scratch_buffer.len()
    }

    #[inline]
    pub fn flush(
        &mut self,
        socket: &std::net::UdpSocket,
        addr: core::net::SocketAddr,
    ) -> io::Result<()> {
        if let Some(n_samples) = num::NonZeroUsize::new(self.scratch_buffer.len()) {
            let mut bytes = arrayvec::ArrayVec::<u8, { MAX_DATAGRAM_SIZE.get() }>::new_const();

            bytes.extend(self.current_timestamp_samples().to_le_bytes());

            self.current_timestamp_spls += u64::try_from(n_samples.get()).unwrap();

            // Hopefully the optimizer understands this is just a memcpy on LE platforms
            bytes.extend(
                self.scratch_buffer
                    .drain(..)
                    .map(Sample::to_bits)
                    .flat_map(u32::to_le_bytes),
            );

            let _ = socket.send_to(bytes.as_slice(), addr)?;
        }

        Ok(())
    }

    #[inline]
    pub fn send(
        &mut self,
        chunk_size_samples: num::NonZeroUsize,
        socket: &std::net::UdpSocket,
        addr: core::net::SocketAddr,
        samples: impl Iterator<Item = Sample>,
    ) -> io::Result<bool> {
        let mut rem = self.n_remaining_chunk_samples(chunk_size_samples);
        let mut used_network = false;

        for sample in samples {
            self.scratch_buffer.push(sample);
            rem = if let Some(next) = num::NonZeroUsize::new(rem.get() - 1) {
                next
            } else {
                self.flush(socket, addr)?;
                used_network = true;
                self.n_remaining_chunk_samples(chunk_size_samples)
            };
        }

        Ok(used_network)
    }

    #[inline]
    pub fn send_sample(
        &mut self,
        chunk_size_samples: num::NonZeroUsize,
        socket: &std::net::UdpSocket,
        addr: core::net::SocketAddr,
        sample: Sample,
    ) -> io::Result<bool> {
        self.send(chunk_size_samples, socket, addr, iter::once(sample))
    }
}
