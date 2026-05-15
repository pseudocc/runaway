use std::os::unix::net::UnixStream;

pub fn main() {
    let socket_path = std::env::var("RUNAWAY_SOCKET").expect("RUNAWAY_SOCKET environment variable not set");

    let stream = UnixStream::connect(socket_path).expect("Failed to connect to socket");

    use runaway::protocol::*;

    let mut handler = Client::new(stream).expect("Failed to initialize client");
    println!("Connected to runaway server");

    let mut send_counter_action = |action: CounterAction| {
        let request = Request::CounterAction(action);
        handler.send(&request).expect("Failed to send request");
        let response: Response = handler.receive().expect("Failed to receive response");
        match response {
            Response::CounterValue(value) => println!("Counter value: {}", value),
            _ => println!("Unexpected response"),
        }
    };

    send_counter_action(CounterAction::Increment);
    send_counter_action(CounterAction::Increment);
    send_counter_action(CounterAction::Get);
    send_counter_action(CounterAction::Decrement);
}
