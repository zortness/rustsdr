fn main() {
    #[cfg(feature = "midi")]
    match sdr_app::practice_midi::ports() {
        Ok(ports) => {
            println!("{} MIDI input ports", ports.len());
            for port in &ports {
                println!("{}", port.name);
            }
            if std::env::args().any(|arg| arg == "--connect-test") {
                let Some(port) = ports.first() else {
                    eprintln!("No input to connect");
                    std::process::exit(1);
                };
                let input = sdr_app::practice_midi::MidiPractice::connect(port.clone()).unwrap();
                std::thread::sleep(std::time::Duration::from_secs(2));
                let status = input.status();
                assert!(
                    status.connected,
                    "MIDI connection failed: {:?}",
                    status.error
                );
                println!(
                    "Connected to {} and checked continued port availability; last event {:?}",
                    port.name, status.last
                );
            }
        }
        Err(error) => {
            eprintln!("MIDI enumeration failed: {error}");
            std::process::exit(1);
        }
    }
    #[cfg(not(feature = "midi"))]
    eprintln!("Build with --features midi to enumerate MIDI inputs");
}
