use std::net::TcpListener;

fn port() -> u16 {
    8080
}

fn main() {
    let listener = TcpListener::bind(("127.0.0.1", port())).unwrap();
    println!("listening");
    for stream in listener.incoming() {
        handle(stream.unwrap());
    }
}

fn handle(_s: std::net::TcpStream) {
    println!("todo");
}
