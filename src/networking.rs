use std::net::{ UdpSocket, Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream };
use std::str::FromStr;
use std::error::Error;
use std::thread::{spawn, JoinHandle};
use std::io::{Read, Write};
use std::time::Duration;

use ext_time::{Duration as ext_Duration, PreciseTime};

const ANNOUNCE_MULTICAST: &'static str = "224.0.1.0";
pub const BASE_PORT: u16 = 8888;

pub fn start_ping_server() -> JoinHandle<()> {
    spawn(|| {
        let tcp_sock = TcpListener::bind(("0.0.0.0", BASE_PORT + 1)).unwrap();
        for stream in tcp_sock.incoming() {
            let mut stream = stream.unwrap();
            let mut buf = Vec::new();
            stream.read(&mut [0]).unwrap();
            stream.write_all(&mut buf).unwrap();
        }
    })
}

pub fn ping(mut target: SocketAddr) -> Option<ext_Duration> {
    target.set_port(BASE_PORT + 1);
    match TcpStream::connect(target) {
        Ok(mut stream) => {
            stream.set_read_timeout(Some(Duration::from_millis(5000))).unwrap();
            let start = PreciseTime::now();
            match stream.write(&[1]) {
                Ok(_) => {
                    match stream.read(&mut [0]) {
                        Ok(_) => Some(start.to(PreciseTime::now())),
                        Err(_) => None
                    }
                },
                Err(_) => None
            }
        },
        Err(_) => None
    }
}

/// Builder struct for `UDPSocketHandle`
#[derive(Debug)]
pub struct UDPSocket {
    local_addr: Ipv4Addr,
    multicast_addr: Ipv4Addr,
    /// The base port on which the sockets are based on
    pub port: u16
}

/// A handle for communication via UDP multicast
#[derive(Debug)]
pub struct UDPSocketHandle {
    /// The `std::net::UdpSocket` that is used for communication
    pub socket: UdpSocket,
    multicast_addr: SocketAddr
}

impl UDPSocket {
    /// Creates a new `UDPSocketHandle` builder
    pub fn new() -> UDPSocket {
        UDPSocket {
            local_addr: Ipv4Addr::new(0, 0, 0, 0),
            multicast_addr: Ipv4Addr::from_str(ANNOUNCE_MULTICAST).expect("Failed to convert MULTICAST const to IP."),
            port: BASE_PORT
        }
    }

    /// Change the port of the resulting socket
    pub fn port(mut self, port: u16) -> UDPSocket {
        self.port = port;
        self
    }

    /// Change the local address on which the socket will bind to
    pub fn local_addr(mut self, ip: &'static str) -> UDPSocket {
        self.local_addr = FromStr::from_str(&ip).ok().expect("Failed to resolve IP.");
        self
    }

    /// Change the multicast group the socket will attempt to join
    pub fn multicast_addr(mut self, ip: &'static str) -> UDPSocket {
        self.multicast_addr = FromStr::from_str(&ip).ok().expect("Failed to resolve IP.");
        self
    }

    /// Assemble a `std::net::UdpSocket` with the previously defined parameters and a port delta. `None` results in it binding to a random free port
    fn assemble_socket(&self, delta_opt: Option<u16>) -> UdpSocket {
        let port = match delta_opt {
            Some(delta) => self.port+delta,
            None => 0
        };
        let sock = match UdpSocket::bind(SocketAddrV4::new(self.local_addr, port)) {
            Ok(s) => s, Err(e) => {exit!(8, "Error binding UDP socket: {}", e.description());}
        };
        match sock.join_multicast_v4(&self.multicast_addr, &self.local_addr) {
            Ok(_) => sock,
            Err(_) => { exit!(1, "Multicast support not available. (NET_ERR)"); }
        }
    }

    /// Create a handle that binds to a random port
    pub fn create_handle(&mut self) -> UDPSocketHandle {
        UDPSocketHandle {
            socket: self.assemble_socket(None),
            multicast_addr: SocketAddr::V4(SocketAddrV4::new(self.multicast_addr, self.port))
        }
    }

    pub fn create_listener(&mut self) -> UDPSocketHandle {
        UDPSocketHandle {
            socket: self.assemble_socket(Some(0)),
            multicast_addr: SocketAddr::V4(SocketAddrV4::new(self.multicast_addr, self.port))
        }
    }
}

impl UDPSocketHandle {
    /// Send a datagram `data` to the `target` address
    pub fn send(&self, data: &[u8], target: SocketAddr) -> usize {
        trace!("UDP SEND {:?} -> {:?}", data, target);
        self.socket.send_to(data, target).ok().expect("Failed to send transmission")
    }

    /// Broadcast a datagram `data` to the previously joined multicast group
    pub fn send_to_multicast(&self, data: &[u8]) -> usize {
        self.send(data, self.multicast_addr)
    }

    /// Receive a datagram from any sender
    pub fn receive(&self) -> (Vec<u8>, SocketAddr) {
        let mut buf = vec![0; 1000000];//2048];
        let (len, src) = self.socket.recv_from(&mut buf).ok().expect("Failed to receive package.");
        buf.truncate(len);
        trace!("UDP RECV {:?} <- {:?}", buf, src);
        (buf, src)
    }

    pub fn try_clone(&self) -> Result<UDPSocketHandle, ()> {
        match self.socket.try_clone() {
            Ok(sock) => Ok(UDPSocketHandle {
                socket: sock,
                multicast_addr: self.multicast_addr
            }),
            Err(_) => Err(())
        }
    }
}
