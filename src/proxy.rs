use hyper::{
    client::HttpConnector,
    server::conn::AddrStream,
    service::{make_service_fn, service_fn},
    Body, Client, Method, Request, Response, Server,
};
use rand::Rng;
use std::net::{IpAddr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpSocket,
};
use crate::auth::Auth;


pub async fn start_proxy(
    listen_addr: SocketAddr,
    (ipv6, prefix_len): (Ipv6Addr, u8),
    auth: Auth, // Dies ist jetzt eine Auth-Instanz
) -> Result<(), Box<dyn std::error::Error>> {
    let auth_arc = Arc::new(auth); // Erstellen einer Arc-Instanz für auth

    let make_service = make_service_fn(move |_: &AddrStream| {
        let auth_arc = Arc::clone(&auth_arc); // Klonen der Arc-Instanz
        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                Proxy {
                    ipv6: ipv6.into(),
                    prefix_len,
                    auth: Arc::clone(&auth_arc), // Verwenden Sie die geklonte Arc-Instanz
                }
                    .proxy(req)
            }))
        }
    });

    Server::bind(&listen_addr)
        .http1_preserve_header_case(true)
        .http1_title_case_headers(true)
        .serve(make_service)
        .await
        .map_err(|err| err.into())
}


#[derive(Clone)]
pub(crate) struct Proxy {
    pub ipv6: u128,
    pub prefix_len: u8,
    pub auth: Arc<Auth>,
}


impl Proxy {
    pub(crate) async fn proxy(self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        if req.method() == Method::CONNECT {
            self.process_connect(req).await
        } else {
            self.process_request(req).await
        }
    }

    async fn process_connect(self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        tokio::task::spawn(async move {
            let remote_addr = req.uri().authority().map(|auth| auth.to_string()).unwrap();
            let mut upgraded = hyper::upgrade::on(req).await.unwrap();
            self.tunnel(&mut upgraded, remote_addr).await
        });
        Ok(Response::new(Body::empty()))
    }


    async fn process_request(self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        if let Some(auth_header) = req.headers().get(hyper::header::AUTHORIZATION) {
            let encoded = auth_header.to_str().unwrap_or("").trim_start_matches("Basic ");
            let decoded = base64::decode(encoded).unwrap_or_else(|_| Vec::new());
            let decoded_str = String::from_utf8(decoded).unwrap_or_default();
            let parts: Vec<&str> = decoded_str.splitn(2, ':').collect();

            if parts.len() != 2 {
                return Ok(Response::builder()
                    .status(hyper::StatusCode::UNAUTHORIZED)
                    .body(Body::from("Unauthorized"))
                    .unwrap());
            }

            let validation = self.auth.validate(parts[0], parts[1]);
            match validation {
                Ok(is_valid) => {
                    if !is_valid {
                        return Ok(Response::builder()
                            .status(hyper::StatusCode::UNAUTHORIZED)
                            .body(Body::from("Unauthorized"))
                            .unwrap());
                    }
                }
                Err(_) => {
                    // Handle authentication error appropriately
                    return Ok(Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Internal Server Error"))
                        .unwrap());
                }
            }
        } else {
            return Ok(Response::builder()
                .status(hyper::StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap());
        }



        let bind_addr = get_rand_ipv6(self.ipv6, self.prefix_len);
        let mut http = HttpConnector::new();
        http.set_local_address(Some(bind_addr));
        println!("{} via {bind_addr}", req.uri().host().unwrap_or_default());

        let client = Client::builder()
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build(http);
        let res = client.request(req).await?;
        Ok(res)
    }


    async fn tunnel<A>(self, upgraded: &mut A, addr_str: String) -> std::io::Result<()>
    where
        A: AsyncRead + AsyncWrite + Unpin + ?Sized,
    {
        if let Ok(addrs) = addr_str.to_socket_addrs() {
            for addr in addrs {
                let socket = TcpSocket::new_v6()?;
                let bind_addr = get_rand_ipv6_socket_addr(self.ipv6, self.prefix_len);
                if socket.bind(bind_addr).is_ok() {
                    println!("{addr_str} via {bind_addr}");
                    if let Ok(mut server) = socket.connect(addr).await {
                        tokio::io::copy_bidirectional(upgraded, &mut server).await?;
                        return Ok(());
                    }
                }
            }
        } else {
            println!("error: {addr_str}");
        }

        Ok(())
    }




}

fn get_rand_ipv6_socket_addr(ipv6: u128, prefix_len: u8) -> SocketAddr {
    let mut rng = rand::thread_rng();
    SocketAddr::new(get_rand_ipv6(ipv6, prefix_len), rng.gen::<u16>())
}

fn get_rand_ipv6(mut ipv6: u128, prefix_len: u8) -> IpAddr {
    let rand: u128 = rand::thread_rng().gen();
    let net_part = (ipv6 >> (128 - prefix_len)) << (128 - prefix_len);
    let host_part = (rand << prefix_len) >> prefix_len;
    ipv6 = net_part | host_part;
    IpAddr::V6(ipv6.into())
}
