//! MIDI input only. Own the native connection off the render/audio threads.
use crate::practice::{MidiBinding, MidiEvent, PracticeInput, midi_contacts, midi_event};
use midir::{Ignore, MidiInput};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct MidiPort {
    pub id: String,
    pub name: String,
}
pub fn ports() -> Result<Vec<MidiPort>, String> {
    let input = MidiInput::new("RustSDR practice ports").map_err(|e| e.to_string())?;
    input
        .ports()
        .iter()
        .map(|p| {
            input
                .port_name(p)
                .map(|name| MidiPort { id: p.id(), name })
                .map_err(|e| e.to_string())
        })
        .collect()
}
#[derive(Clone, Default)]
pub struct MidiStatus {
    pub connected: bool,
    pub error: Option<String>,
    pub last: Option<MidiEvent>,
    pub learned: Option<MidiBinding>,
    pub sequence: u64,
}
struct Route {
    input: PracticeInput,
    bindings: [Option<MidiBinding>; 3],
    held: [bool; 3],
}
pub struct MidiPractice {
    status: Arc<Mutex<MidiStatus>>,
    route: Arc<Mutex<Option<Route>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl MidiPractice {
    pub fn connect(selected: MidiPort) -> Result<Self, String> {
        let status = Arc::new(Mutex::new(MidiStatus::default()));
        let route = Arc::new(Mutex::new(None::<Route>));
        let stop = Arc::new(AtomicBool::new(false));
        let (state, routing, stopping) = (status.clone(), route.clone(), stop.clone());
        let worker = thread::Builder::new()
            .name("cw-midi-input".into())
            .spawn(move || {
                let run = || -> Result<(), String> {
                    let mut midi =
                        MidiInput::new("RustSDR CW practice").map_err(|e| e.to_string())?;
                    midi.ignore(Ignore::Time);
                    let available = midi.ports();
                    let port = available
                        .iter()
                        .find(|port| port.id() == selected.id)
                        .ok_or("MIDI port disappeared; refresh the port list")?;
                    if midi.port_name(port).map_err(|e| e.to_string())? != selected.name {
                        return Err("MIDI ports changed; refresh and select again".into());
                    }
                    let monitor = state.clone();
                    let callback_route = routing.clone();
                    let _connection = midi
                        .connect(
                            port,
                            "RustSDR CW input",
                            move |_, bytes, _| {
                                let Some(event) = midi_event(bytes) else {
                                    return;
                                };
                                let mut state = monitor.lock().unwrap();
                                state.last = Some(event);
                                if let MidiEvent::Contact(binding, true) = event {
                                    state.learned = Some(binding);
                                    state.sequence = state.sequence.wrapping_add(1);
                                }
                                drop(state);
                                if let Some(route) = callback_route.lock().unwrap().as_mut() {
                                    let before = route.held;
                                    midi_contacts(&route.bindings, &mut route.held, event);
                                    if before != route.held {
                                        route.input.set(1, route.held);
                                    }
                                }
                            },
                            (),
                        )
                        .map_err(|e| e.to_string())?;
                    state.lock().unwrap().connected = true;
                    let mut checked = Instant::now();
                    while !stopping.load(Ordering::Acquire) {
                        if checked.elapsed() >= Duration::from_millis(500) {
                            // midir exposes no portable disconnect callback. Re-enumerate
                            // and fail closed when the selected port is no longer present.
                            if !ports()?.iter().any(|port| port.id == selected.id) {
                                return Err(
                                    "MIDI input disconnected; reconnect before re-arming".into()
                                );
                            }
                            checked = Instant::now();
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Ok(())
                };
                let result = run();
                if let Some(route) = routing.lock().unwrap().take() {
                    route.input.stop();
                }
                let mut state = state.lock().unwrap();
                state.connected = false;
                if let Err(error) = result {
                    state.error = Some(error);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            status,
            route,
            stop,
            worker: Some(worker),
        })
    }
    pub fn status(&self) -> MidiStatus {
        self.status.lock().unwrap().clone()
    }
    /// Change routing only while disarmed or after release; never inherit held notes.
    pub fn target(&self, target: Option<(PracticeInput, [Option<MidiBinding>; 3])>) {
        let mut slot = self.route.lock().unwrap();
        if let Some(old) = slot.take() {
            old.input.set(1, [false; 3]);
        }
        *slot = target
            .filter(|(_, bindings)| bindings.iter().any(Option::is_some))
            .map(|(input, bindings)| Route {
                input,
                bindings,
                held: [false; 3],
            });
    }
}
impl Drop for MidiPractice {
    fn drop(&mut self) {
        self.target(None);
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
