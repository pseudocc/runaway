use runaway::app::App;

pub fn main() -> std::io::Result<()> {
    let socket_path = std::env::var("RUNAWAY_SOCKET").expect("RUNAWAY_SOCKET environment variable not set");

    let mut app = App::new(socket_path);
    app.run()
}
