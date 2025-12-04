fn main() -> std::io::Result<core::convert::Infallible> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:6910")?;

    syfala_jack::server::start(
        &socket,
        syfala_net::AudioConfig::new(1.try_into().unwrap(), 16.try_into().unwrap()),
    )
}
