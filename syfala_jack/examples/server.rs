fn main() -> std::io::Result<core::convert::Infallible> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:6911")?;
    socket.set_read_timeout(Some(core::time::Duration::from_millis(100)))?;

    const RB_LEN: core::time::Duration = core::time::Duration::from_secs(4);
    const DELAY: core::time::Duration = core::time::Duration::ZERO;

    syfala_jack::server::start(
        &socket,
        8.try_into().unwrap(),
        |_, _| RB_LEN,
        |_, _| DELAY
    )
}