fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("agenda {}", agenda_core::VERSION);
        return;
    }
    let api = agenda_core::Api::new();
    println!("{}", api.call("version", "{}"));
}
