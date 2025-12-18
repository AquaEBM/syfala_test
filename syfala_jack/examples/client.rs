fn main() -> std::io::Result<core::convert::Infallible> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:6910")?;
    socket.set_broadcast(true)?;

    const RB_LEN: core::time::Duration = core::time::Duration::from_secs(4);
    const DELAY: core::time::Duration = core::time::Duration::from_millis(5);

    syfala_jack::client::start(
        &socket,
        "255.255.255.255:6911".parse().unwrap(),
        core::time::Duration::from_millis(250),
        |_, _| RB_LEN,
        |_, _| DELAY,
    )
}
