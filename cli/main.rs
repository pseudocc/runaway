use std::os::unix::net::UnixStream;

pub fn main() {
    let socket_path = std::env::var("RUNAWAY_SOCKET").expect("RUNAWAY_SOCKET environment variable not set");

    let mut stream = UnixStream::connect(socket_path).expect("Failed to connect to socket");

    use std::io::{Read, Write};
    stream.write_all(b"counter").expect("Failed to write to socket");
    let mut response = [0u8; 64];
    let n = stream.read(&mut response).expect("Failed to read from socket");
    println!("Response: {}", String::from_utf8_lossy(&response[..n]));
}
