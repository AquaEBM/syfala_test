use super::*;

pub mod discovery {
    use super::*;

    const DISCOVERY_MESSAGE: &[u8] = b"SYFALACLIENTADDR";
    const DISCOVERY_PACKET_LEN: usize =
        // magic message (little endian)
        DISCOVERY_MESSAGE.len()
        // Audio socket IP address (v4) (little_endian)
        + size_of::<u32>()
        // Audio socket port (little endian)
        + size_of::<u16>();

    #[inline]
    pub fn send_discovery(
        socket: &std::net::UdpSocket,
        dest_addr: core::net::SocketAddr,
        audio_addr: core::net::SocketAddrV4,
    ) -> io::Result<()> {
        // // figure out the actual address the receiver will see when
        // // sent data from a socket bound to audio_addr

        // let temp_socket = std::net::UdpSocket::bind(audio_addr)?;
        // temp_socket.connect(addr)?;
        // let core::net::SocketAddr::V4(audio_addr) = temp_socket.local_addr()? else {
        //     return Err(io::Error::from(io::ErrorKind::Other));
        // };

        // build the packet

        let mut packet_buf = arrayvec::ArrayVec::<_, DISCOVERY_PACKET_LEN>::new_const();

        packet_buf.try_extend_from_slice(DISCOVERY_MESSAGE).unwrap();
        packet_buf
            .try_extend_from_slice(&audio_addr.ip().to_bits().to_le_bytes())
            .unwrap();
        packet_buf
            .try_extend_from_slice(&audio_addr.port().to_le_bytes())
            .unwrap();

        assert_eq!(
            packet_buf.len(),
            packet_buf.capacity(),
            "ERROR: missing fields"
        );

        let err = socket.send_to(&packet_buf, dest_addr);
        

        if err? != DISCOVERY_PACKET_LEN {
            Err(io::ErrorKind::Other.into())
        } else {
            Ok(())
        }
    }

    #[inline(always)]
    fn parse_discovery_packet(packet: &[u8]) -> Option<core::net::SocketAddrV4> {
        let (_message, rem) = packet
            .split_at_checked(DISCOVERY_MESSAGE.len())
            .filter(|&(message, _)| message == DISCOVERY_MESSAGE)?;

        let (&ip, rem) = rem.split_first_chunk()?;
        let ip = u32::from_le_bytes(ip);

        let (&port, _rem) = rem.split_first_chunk()?;
        let port = u16::from_le_bytes(port);

        Some(core::net::SocketAddrV4::new(
            core::net::Ipv4Addr::from_bits(ip),
            port,
        ))
    }

    #[inline]
    pub fn accept_discovery(
        socket: &std::net::UdpSocket,
    ) -> io::Result<(core::net::SocketAddr, Option<core::net::SocketAddrV4>)> {
        let mut packet_buf = [0u8; DISCOVERY_PACKET_LEN];

        let (bytes_read, source_addr) = socket.recv_from(&mut packet_buf)?;

        Ok((source_addr, parse_discovery_packet(&packet_buf[..bytes_read])))
    }

    const SERVER_CONFIG_MESSAGE: &[u8] = b"SYFALASERVERCONF";
    const SERVER_CONFIG_PACKET_LEN: usize =
        // magic message (little endian)
        SERVER_CONFIG_MESSAGE.len()
        // Audio socket IP address (v4) (little_endian)
        + size_of::<u32>()
        // Audio socket port (little endian)
        + size_of::<u16>()
        // channel count (little endian) (must be non-zero)
        + size_of::<u32>()
        // buffer size (litte endian) (must be non-zero)
        + size_of::<u32>();

    #[inline]
    pub fn send_config(
        socket: &std::net::UdpSocket,
        dest_addr: core::net::SocketAddr,
        audio_addr: core::net::SocketAddrV4,
        config: AudioConfig,
    ) -> io::Result<()> {
        let mut packet_buf = arrayvec::ArrayVec::<_, SERVER_CONFIG_PACKET_LEN>::new_const();

        packet_buf
            .try_extend_from_slice(SERVER_CONFIG_MESSAGE)
            .unwrap();
        packet_buf
            .try_extend_from_slice(&audio_addr.ip().to_bits().to_le_bytes())
            .unwrap();
        packet_buf
            .try_extend_from_slice(&audio_addr.port().to_le_bytes())
            .unwrap();
        packet_buf
            .try_extend_from_slice(&config.n_channels().get().to_le_bytes())
            .unwrap();
        packet_buf
            .try_extend_from_slice(&config.chunk_size_frames().get().to_le_bytes())
            .unwrap();

        assert_eq!(
            packet_buf.len(),
            packet_buf.capacity(),
            "ERROR: missing fields"
        );

        if socket.send_to(&packet_buf, dest_addr)? != DISCOVERY_PACKET_LEN {
            Err(io::ErrorKind::Other.into())
        } else {
            Ok(())
        }
    }

    #[inline(always)]
    fn parse_config(packet: &[u8]) -> Option<(core::net::SocketAddrV4, AudioConfig)> {
        let (_message, rem) = packet
            .split_at_checked(SERVER_CONFIG_MESSAGE.len())
            .filter(|&(message, _)| message == SERVER_CONFIG_MESSAGE)?;

        let (&ip, rem) = rem.split_first_chunk()?;
        let ip = u32::from_le_bytes(ip);

        let (&port, rem) = rem.split_first_chunk()?;
        let port = u16::from_le_bytes(port);

        let (&n_channels, rem) = rem.split_first_chunk()?;
        let n_channels = u32::from_be_bytes(n_channels).try_into().unwrap();

        let (&buffer_size_frames, _rem) = rem.split_first_chunk()?;
        let buffer_size_frames = u32::from_be_bytes(buffer_size_frames).try_into().unwrap();

        Some((
            core::net::SocketAddrV4::new(core::net::Ipv4Addr::from_bits(ip), port),
            AudioConfig::new(n_channels, buffer_size_frames),
        ))
    }

    #[inline(always)]
    pub fn accept_config(
        socket: &std::net::UdpSocket,
    ) -> io::Result<Option<(core::net::SocketAddrV4, AudioConfig)>> {
        let mut packet_buf = [0u8; SERVER_CONFIG_PACKET_LEN];

        let bytes_read = socket.recv(&mut packet_buf)?;

        Ok(parse_config(&packet_buf[..bytes_read]))
    }
}

const MAX_DATAGRAM_SIZE: num::NonZeroUsize = nz(1452);

pub struct Sender {
    chunk_size_spls: num::NonZeroUsize,
    // hehehe zero copy yoohoo
    scratch_buffer: arrayvec::ArrayVec<u8, { MAX_DATAGRAM_SIZE.get() }>,
}

impl Sender {
    const TIMESTAMP_SIZE_BYTES: usize = size_of::<u64>();

    #[inline(always)]
    pub fn new(chunk_size_spls: num::NonZeroUsize) -> Self {
        let mut scratch_buffer = arrayvec::ArrayVec::new_const();
        scratch_buffer.extend(0u64.to_le_bytes());

        Self {
            scratch_buffer,
            chunk_size_spls,
        }
    }

    #[inline(always)]
    fn split(&self) -> (u64, &[u8]) {
        // the buffer always contains at least 8 bytes (the packet timestamp)
        let (timestamp, sample_data) = self.scratch_buffer.split_at(Self::TIMESTAMP_SIZE_BYTES);
        (
            u64::from_le_bytes(timestamp.try_into().unwrap()),
            sample_data,
        )
    }

    #[inline(always)]
    pub const fn chunk_size_samples(&self) -> num::NonZeroUsize {
        self.chunk_size_spls
    }

    #[inline(always)]
    pub const fn set_chunk_size_samples(&mut self, size: num::NonZeroUsize) {
        self.chunk_size_spls = size;
    }

    #[inline(always)]
    fn n_stored_samples(&self) -> usize {
        self.split().1.len() / SAMPLE_SIZE
    }

    #[inline(always)]
    pub fn current_timestamp_samples(&self) -> u64 {
        self.split()
            .0
            .strict_add(self.n_stored_samples().try_into().unwrap())
    }

    #[inline(always)]
    fn n_remaining_chunk_samples(&self) -> num::NonZeroUsize {
        let chunk_size_samples = num::NonZeroU64::try_from(self.chunk_size_samples()).unwrap();
        // Never zero, we always flush at least as soon as the buffer is full
        let max_samples_left = num::NonZeroU64::new(
            (self.scratch_buffer.remaining_capacity() / SAMPLE_SIZE)
                .try_into()
                .unwrap(),
        )
        .unwrap();

        // Never zero, we always flush at least at chunk boundaries
        let chunk_samples_left = num::NonZeroU64::new(
            chunk_size_samples
                .get()
                .strict_sub(self.current_timestamp_samples() % chunk_size_samples),
        )
        .unwrap();

        max_samples_left.min(chunk_samples_left).try_into().unwrap()
    }

    #[inline]
    pub fn flush(
        &mut self,
        socket: &std::net::UdpSocket,
        addr: core::net::SocketAddr,
    ) -> io::Result<()> {
        let (timestamp, sample_data) = self.split();

        let n_samples = u64::try_from(sample_data.len() / SAMPLE_SIZE).unwrap();

        socket.send_to(self.scratch_buffer.as_slice(), addr)?;

        self.scratch_buffer.clear();

        self.scratch_buffer
            .extend(u64::to_le_bytes(timestamp + n_samples));
        Ok(())
    }

    #[inline]
    pub fn send(
        &mut self,
        socket: &std::net::UdpSocket,
        addr: core::net::SocketAddr,
        samples: impl Iterator<Item = Sample>,
    ) -> io::Result<bool> {
        let mut rem = self.n_remaining_chunk_samples();
        let mut used_network = false;

        for sample in samples {
            self.scratch_buffer.extend(sample.to_le_bytes());

            rem = if let Some(next) = num::NonZeroUsize::new(rem.get() - 1) {
                next
            } else {
                self.flush(socket, addr)?;
                used_network = true;
                self.n_remaining_chunk_samples()
            };
        }

        Ok(used_network)
    }
}

#[inline]
pub fn recv_audio_packet(
    socket: &std::net::UdpSocket,
) -> io::Result<(
    core::net::SocketAddr,
    u64,
    impl Iterator<Item = Sample> + 'static,
)> {
    // parse the next packet and return an interator of the samples it contains
    let mut buf = [0u8; MAX_DATAGRAM_SIZE.get()];

    let (bytes_read, peer_addr) = socket.recv_from(&mut buf)?;

    let timestamp = buf[..bytes_read]
        .split_first_chunk()
        .map(|(&chunk, _rem)| u64::from_le_bytes(chunk))
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;

    // At this point, we are sure that we have read at least size_of::<u64>() bytes.
    let mut sample_byte_iter = buf.into_iter().skip(size_of::<u64>()).take(bytes_read);

    // NIGHTLY: #[feature(iter_array_chunks)] use array_chunks
    // instead of whatever this is
    let sample_iter = iter::from_fn(move || {
        let mut sample_buf = [0u8; _];

        for byte in &mut sample_buf {
            *byte = sample_byte_iter.next()?;
        }

        Some(Sample::from_bits(u32::from_le_bytes(sample_buf)))
    });

    return Ok((peer_addr, timestamp, sample_iter));
}
