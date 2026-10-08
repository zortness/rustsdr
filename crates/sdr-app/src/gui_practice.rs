use super::*;
#[cfg(feature = "midi")]
use sdr_app::practice::MidiBinding;
use sdr_app::practice::{PracticeSession, PracticeStatus};
use sdr_dsp::keyer::{KeyerConfig, KeyerMode};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ControllerId {
    name: String,
    vendor: Option<u16>,
    product: Option<u16>,
}
#[derive(serde::Serialize, serde::Deserialize)]
enum SavedBinding {
    Key(KeyCode),
    Mouse(MouseButton),
    Gamepad(ControllerId, GamepadButton),
    Midi(sdr_app::practice::MidiBinding),
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Preferences {
    open: bool,
    mode: String,
    wpm: f32,
    pitch_hz: f32,
    volume: f32,
    midi_port_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
    Gamepad(Entity, GamepadButton),
    SavedGamepad(ControllerId, GamepadButton),
    #[cfg(feature = "midi")]
    Midi(MidiBinding),
    #[cfg(not(feature = "midi"))]
    UnavailableMidi(sdr_app::practice::MidiBinding),
}
impl Binding {
    fn label(&self) -> String {
        match self {
            Self::Key(key) => format!("Key {key:?}"),
            Self::Mouse(button) => format!("Mouse {button:?}"),
            Self::Gamepad(id, button) => format!("Controller {} {button:?}", id.index()),
            Self::SavedGamepad(id, button) => format!("{} {button:?} (disconnected)", id.name),
            #[cfg(feature = "midi")]
            Self::Midi(binding) => binding.label(),
            #[cfg(not(feature = "midi"))]
            Self::UnavailableMidi(_) => "MIDI (not enabled in this build)".into(),
        }
    }
}
pub(super) struct PracticePanel {
    pub open: bool,
    pub button: Option<egui::Pos2>,
    config: KeyerConfig,
    bindings: [Option<Binding>; 3],
    learning: Option<usize>,
    wait_release: bool,
    session: Option<PracticeSession>,
    status: PracticeStatus,
    previous: [bool; 3],
    volume: f32,
    message: String,
    controllers: std::collections::HashMap<Entity, ControllerId>,
    midi_port_id: String,
    arm_button: Option<egui::Pos2>,
    learn_buttons: [Option<egui::Pos2>; 3],
    #[cfg(feature = "midi")]
    midi: Option<sdr_app::practice_midi::MidiPractice>,
    #[cfg(feature = "midi")]
    ports: Vec<sdr_app::practice_midi::MidiPort>,
    #[cfg(feature = "midi")]
    port: usize,
    #[cfg(feature = "midi")]
    midi_sequence: u64,
}
impl Default for PracticePanel {
    fn default() -> Self {
        Self {
            open: false,
            button: None,
            config: KeyerConfig::default(),
            bindings: [
                Some(Binding::Key(KeyCode::Space)),
                Some(Binding::Key(KeyCode::KeyZ)),
                Some(Binding::Key(KeyCode::KeyX)),
            ],
            learning: None,
            wait_release: false,
            session: None,
            status: PracticeStatus::default(),
            previous: [false; 3],
            volume: 0.15,
            message: String::new(),
            controllers: default(),
            midi_port_id: String::new(),
            arm_button: None,
            learn_buttons: [None; 3],
            #[cfg(feature = "midi")]
            midi: None,
            #[cfg(feature = "midi")]
            ports: Vec::new(),
            #[cfg(feature = "midi")]
            port: 0,
            #[cfg(feature = "midi")]
            midi_sequence: 0,
        }
    }
}
impl PracticePanel {
    pub(super) fn restore_preferences(
        &mut self,
        doc: &sdr_app::preferences::Document,
        warnings: &mut Vec<String>,
    ) {
        if let Some(p) = settings::section::<Preferences>(doc, "practice", warnings) {
            let mode = match p.mode.as_str() {
                "straight" => Some(KeyerMode::Straight),
                "iambic_a" => Some(KeyerMode::IambicA),
                "iambic_b" => Some(KeyerMode::IambicB),
                _ => None,
            };
            if let Some(mode) = mode
                && (5.0..=60.0).contains(&p.wpm)
                && (200.0..=1200.0).contains(&p.pitch_hz)
                && (0.0..=1.0).contains(&p.volume)
            {
                self.config = KeyerConfig {
                    mode,
                    wpm: p.wpm,
                    pitch_hz: p.pitch_hz,
                };
                self.volume = p.volume;
                self.open = p.open;
                self.midi_port_id = p.midi_port_id;
            } else {
                warnings.push("Invalid practice settings; using defaults".into());
            }
        }
        if let Some(bindings) =
            settings::section::<[Option<SavedBinding>; 3]>(doc, "practice.bindings", warnings)
        {
            self.bindings = bindings.map(|b| {
                b.and_then(|b| match b {
                    SavedBinding::Key(k) => Some(Binding::Key(k)),
                    SavedBinding::Mouse(m) => Some(Binding::Mouse(m)),
                    SavedBinding::Gamepad(id, b) => Some(Binding::SavedGamepad(id, b)),
                    #[cfg(feature = "midi")]
                    SavedBinding::Midi(b) => match b {
                        MidiBinding::Note { channel, note } if channel < 16 && note < 128 => {
                            Some(Binding::Midi(b))
                        }
                        MidiBinding::Control { channel, control }
                            if channel < 16 && control < 128 =>
                        {
                            Some(Binding::Midi(b))
                        }
                        _ => None,
                    },
                    #[cfg(not(feature = "midi"))]
                    SavedBinding::Midi(b) => Some(Binding::UnavailableMidi(b)),
                })
            });
        }
    }
    pub(super) fn save_preferences(&self, doc: &mut sdr_app::preferences::Document) {
        doc.put(
            "practice",
            &Preferences {
                open: self.open,
                mode: match self.config.mode {
                    KeyerMode::Straight => "straight",
                    KeyerMode::IambicA => "iambic_a",
                    KeyerMode::IambicB => "iambic_b",
                }
                .into(),
                wpm: self.config.wpm,
                pitch_hz: self.config.pitch_hz,
                volume: self.volume,
                midi_port_id: self.midi_port_id.clone(),
            },
        );
        let bindings = self.bindings.each_ref().map(|b| {
            b.as_ref().and_then(|b| match b {
                Binding::Key(k) => Some(SavedBinding::Key(*k)),
                Binding::Mouse(m) => Some(SavedBinding::Mouse(*m)),
                Binding::Gamepad(entity, b) => self
                    .controllers
                    .get(entity)
                    .map(|id| SavedBinding::Gamepad(id.clone(), *b)),
                Binding::SavedGamepad(id, b) => Some(SavedBinding::Gamepad(id.clone(), *b)),
                #[cfg(feature = "midi")]
                Binding::Midi(b) => Some(SavedBinding::Midi(*b)),
                #[cfg(not(feature = "midi"))]
                Binding::UnavailableMidi(b) => Some(SavedBinding::Midi(*b)),
            })
        });
        doc.put("practice.bindings", &bindings);
    }
    fn stop(&mut self) {
        #[cfg(feature = "midi")]
        if let Some(midi) = &self.midi {
            midi.target(None);
        }
        if let Some(session) = self.session.take() {
            session.input.stop();
            self.status = session.status();
        }
        self.status.running = false;
        self.status.key_down = false;
        self.previous = [false; 3];
    }
    fn active(&self, index: usize) -> bool {
        if self.config.mode == KeyerMode::Straight {
            index == 0
        } else {
            index > 0
        }
    }
    fn arm(&mut self) {
        if self
            .bindings
            .iter()
            .enumerate()
            .any(|(i, b)| self.active(i) && matches!(b, Some(Binding::SavedGamepad(..))))
        {
            self.message =
                "Connect the saved controller or learn a new binding before arming.".into();
            return;
        }
        #[cfg(feature = "midi")]
        if self
            .bindings
            .iter()
            .enumerate()
            .any(|(i, b)| self.active(i) && matches!(b, Some(Binding::Midi(_))))
            && !self
                .midi
                .as_ref()
                .is_some_and(|midi| midi.status().connected)
        {
            self.message = "Connect the MIDI input before arming its bindings.".into();
            return;
        }
        self.learning = None;
        match PracticeSession::start(self.config, self.volume, true) {
            Ok(session) => {
                self.session = Some(session);
                self.wait_release = true;
                self.message.clear();
            }
            Err(error) => self.message = error,
        }
    }
}

/// Runs before egui consumes input: learned/armed controls do not also activate
/// radio widgets. Keyboard inputs are physical keys; OS key repeats are ignored.
pub(super) fn input(
    mut display: ResMut<Display>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<(Entity, &Gamepad)>,
    names: Query<&Name>,
    windows: Query<&Window>,
    mut egui_inputs: Query<&mut bevy_egui::EguiInput>,
) {
    let panel = &mut display.practice;
    for (entity, gamepad) in &gamepads {
        panel.controllers.insert(
            entity,
            ControllerId {
                name: names.get(entity).map_or("Controller", Name::as_str).into(),
                vendor: gamepad.vendor_id(),
                product: gamepad.product_id(),
            },
        );
    }
    // Entity IDs are session-local. Only reconnect an unambiguous device identity.
    for binding in &mut panel.bindings {
        if let Some(Binding::SavedGamepad(identity, button)) = binding {
            let mut matches = gamepads
                .iter()
                .filter(|(entity, _)| panel.controllers.get(entity) == Some(identity));
            if let Some((entity, _)) = matches.next()
                && matches.next().is_none()
            {
                *binding = Some(Binding::Gamepad(entity, *button));
            }
        }
    }
    if !panel.open || keys.just_pressed(KeyCode::Escape) || !windows.iter().any(|w| w.focused) {
        if panel.session.is_some() {
            panel.stop();
            panel.message = "Practice disarmed. Release controls before re-arming.".into();
        }
        panel.learning = None;
        return;
    }
    let any_down = keys.get_pressed().next().is_some()
        || mouse.get_pressed().next().is_some()
        || gamepads
            .iter()
            .any(|(_, g)| g.get_pressed().next().is_some());
    let capturing = panel.learning.is_some() || panel.session.is_some();
    let was_learning = panel.learning.is_some();
    if let Some(index) = panel.learning {
        if panel.wait_release {
            if !any_down {
                panel.wait_release = false;
            }
        } else {
            #[allow(unused_mut)]
            let mut learned = keys
                .get_just_pressed()
                .next()
                .map(|key| Binding::Key(*key))
                .or_else(|| {
                    mouse
                        .get_just_pressed()
                        .next()
                        .map(|button| Binding::Mouse(*button))
                })
                .or_else(|| {
                    gamepads.iter().find_map(|(id, g)| {
                        g.get_just_pressed()
                            .next()
                            .map(|button| Binding::Gamepad(id, *button))
                    })
                });
            #[cfg(feature = "midi")]
            if let Some(midi) = &panel.midi {
                let status = midi.status();
                if status.sequence != panel.midi_sequence {
                    learned = status.learned.map(Binding::Midi);
                    panel.midi_sequence = status.sequence;
                }
            }
            if let Some(binding) = learned {
                if panel
                    .bindings
                    .iter()
                    .enumerate()
                    .any(|(i, b)| i != index && b.as_ref() == Some(&binding))
                {
                    panel.message =
                        "That input is already bound to another contact. Clear it first.".into();
                } else {
                    panel.bindings[index] = Some(binding);
                    panel.message.clear();
                }
                panel.learning = None;
            }
        }
    }
    if panel.session.is_some() {
        let disconnected = panel.bindings.iter().enumerate().any(|(i, binding)| {
            panel.active(i)
                && matches!(binding, Some(Binding::Gamepad(id, _)) if gamepads.get(*id).is_err())
        });
        if disconnected {
            panel.stop();
            panel.message =
                "Controller disconnected. Reconnect and learn its bindings again.".into();
            return;
        }
        let status = panel.session.as_ref().unwrap().status();
        panel.status = status.clone();
        if !status.running {
            panel.stop();
            panel.message = status
                .error
                .unwrap_or_else(|| "Practice stopped; re-arm to continue".into());
            return;
        }
        panel.session.as_ref().unwrap().input.heartbeat();
        if panel.wait_release && !any_down && status.ready {
            panel.wait_release = false;
            #[cfg(feature = "midi")]
            if let Some(midi) = &panel.midi {
                midi.target(Some((
                    panel.session.as_ref().unwrap().input.clone(),
                    std::array::from_fn(|i| {
                        if !panel.active(i) {
                            return None;
                        }
                        match panel.bindings[i] {
                            Some(Binding::Midi(binding)) => Some(binding),
                            _ => None,
                        }
                    }),
                )));
            }
        }
        if !panel.wait_release && status.ready {
            let held = std::array::from_fn(|i| {
                panel.active(i)
                    && match &panel.bindings[i] {
                        Some(Binding::Key(key)) => keys.pressed(*key),
                        Some(Binding::Mouse(button)) => mouse.pressed(*button),
                        Some(Binding::Gamepad(id, button)) => {
                            gamepads.get(*id).is_ok_and(|(_, g)| g.pressed(*button))
                        }
                        _ => false,
                    }
            });
            if held != panel.previous {
                panel.session.as_ref().unwrap().input.set(0, held);
                panel.previous = held;
            }
        }
    }
    if capturing {
        for mut input in &mut egui_inputs {
            input.events.retain(|event| match event {
                egui::Event::Key { pressed, .. } => !pressed,
                egui::Event::Text(_) => false,
                egui::Event::PointerButton {
                    button, pressed, ..
                } => {
                    if !pressed {
                        return true;
                    }
                    if was_learning {
                        return false;
                    }
                    let native = match button {
                        egui::PointerButton::Primary => MouseButton::Left,
                        egui::PointerButton::Secondary => MouseButton::Right,
                        egui::PointerButton::Middle => MouseButton::Middle,
                        egui::PointerButton::Extra1 => MouseButton::Back,
                        egui::PointerButton::Extra2 => MouseButton::Forward,
                    };
                    !panel.bindings.iter().enumerate().any(|(i, binding)| {
                        panel.active(i) && binding == &Some(Binding::Mouse(native))
                    })
                }
                _ => true,
            });
        }
    }
}

pub(super) fn show(context: &egui::Context, panel: &mut PracticePanel) {
    let mut open = panel.open;
    egui::Window::new("CW practice · local audio only").open(&mut open).default_width(610.0).show(context, |ui| {
        let armed = panel.session.is_some();
        ui.horizontal(|ui| {
            let arm = ui.button(if armed { "Disarm / stop" } else { "Arm practice" });
            panel.arm_button = Some(arm.rect.center());
            if arm.clicked() { if armed { panel.stop(); } else { panel.arm(); } }
            ui.colored_label(if armed { AMBER } else { CYAN }, if armed {
                if panel.wait_release { "Release all controls…" } else if panel.status.key_down { "KEY DOWN" } else { "Ready" }
            } else { "Disarmed" });
            ui.small("Escape always stops");
        });
        ui.small("Practice produces sound and text only. Reception settings are independent.");
        ui.add_enabled_ui(!armed, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut panel.config.mode, KeyerMode::Straight, "Straight key");
                ui.selectable_value(&mut panel.config.mode, KeyerMode::IambicA, "Iambic A");
                ui.selectable_value(&mut panel.config.mode, KeyerMode::IambicB, "Iambic B");
            });
            ui.add(egui::Slider::new(&mut panel.config.wpm, 5.0..=60.0).text(if panel.config.mode == KeyerMode::Straight { "Decoder starting WPM" } else { "Paddle WPM" }));
            ui.add(egui::Slider::new(&mut panel.config.pitch_hz, 200.0..=1200.0).text("Sidetone Hz"));
            ui.small(match panel.config.mode {
                KeyerMode::Straight => "Press duration controls the mark. WPM only seeds the practice decoder.",
                KeyerMode::IambicA => "Hold either paddle to repeat; squeeze to alternate. Release both to finish the current element and stop.",
                KeyerMode::IambicB => "Squeeze to alternate. Release both to finish the current element and add one opposite element.",
            });
            ui.separator();
            egui::Grid::new("practice-bindings").num_columns(4).show(ui, |ui| {
                for (index, name) in ["Straight", "Dit paddle", "Dah paddle"].iter().enumerate() {
                    ui.label(*name);
                    ui.monospace(panel.bindings[index].as_ref().map_or_else(|| "Unbound".into(), Binding::label));
                    let learn = ui.button(if panel.learning == Some(index) { "Listening…" } else { "Learn input" });
                    panel.learn_buttons[index] = Some(learn.rect.center());
                    if learn.clicked() {
                        panel.learning = Some(index); panel.wait_release = true;
                        #[cfg(feature = "midi")]
                        { panel.midi_sequence = panel.midi.as_ref().map_or(0, |midi| midi.status().sequence); }
                    }
                    if ui.small_button("Clear").clicked() { panel.bindings[index] = None; }
                    ui.end_row();
                }
            });
            if panel.learning.is_some() { ui.colored_label(AMBER, "Release controls, then press a key, mouse/controller button, or MIDI note/CC. Esc cancels."); }
            if ui.button("Swap dit / dah").clicked() { panel.bindings.swap(1, 2); }
            midi_controls(ui, panel);
        });
        if ui.add(egui::Slider::new(&mut panel.volume, 0.0..=1.0).text("Practice volume")).changed()
            && let Some(session) = &panel.session {
            session.volume.store(panel.volume.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
        ui.small("Bindings last for this session. While armed, keyboard shortcuts and bound mouse clicks are captured. Closing this window or losing app focus disarms practice.");
        if !panel.message.is_empty() { ui.colored_label(AMBER, &panel.message); }
        if let Some(error) = &panel.status.audio_error { ui.colored_label(AMBER, error); }
        ui.separator();
        ui.label("Your sending · experimental decoder");
        if let Some(decoded) = &panel.status.decoded {
            ui.small(format!("{:.1} WPM estimate · {} unknown patterns", decoded.wpm, decoded.unknown_characters));
            egui::ScrollArea::vertical().max_height(130.0).stick_to_bottom(true).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&decoded.text).monospace()).wrap().selectable(true));
            });
            if ui.button("Copy practice text").clicked() { ui.ctx().copy_text(decoded.text.clone()); }
        }
        ui.small("Re-arming clears the transcript. Character and word spacing are up to you; decoded text is feedback, not a definitive score.");
    });
    panel.open = open;
    if !open {
        panel.stop();
        panel.learning = None;
    }
}

#[cfg(not(feature = "midi"))]
fn midi_controls(ui: &mut egui::Ui, _: &mut PracticePanel) {
    ui.small("Build with the midi feature for MIDI input.");
}
#[cfg(feature = "midi")]
fn midi_controls(ui: &mut egui::Ui, panel: &mut PracticePanel) {
    ui.collapsing("MIDI input", |ui| {
        ui.horizontal(|ui| {
            if ui.button("Refresh ports").clicked() {
                match sdr_app::practice_midi::ports() {
                    Ok(ports) => { panel.port = ports.iter().position(|p| p.id == panel.midi_port_id).unwrap_or(0); panel.ports = ports; }, Err(error) => panel.message = error,
                }
            }
            egui::ComboBox::from_id_salt("practice-midi-port").selected_text(panel.ports.get(panel.port).map_or("Select MIDI input", |port| port.name.as_str())).show_ui(ui, |ui| {
                for (index, port) in panel.ports.iter().enumerate() { if ui.selectable_value(&mut panel.port, index, &port.name).changed() { panel.midi_port_id = port.id.clone(); } }
            });
            if ui.add_enabled(panel.port < panel.ports.len(), egui::Button::new("Connect")).clicked() {
                panel.midi = None;
                panel.midi_port_id = panel.ports[panel.port].id.clone();
                match sdr_app::practice_midi::MidiPractice::connect(panel.ports[panel.port].clone()) {
                    Ok(midi) => panel.midi = Some(midi), Err(error) => panel.message = error,
                }
            }
            if ui.button("Disconnect").clicked() { panel.midi = None; }
        });
        if let Some(midi) = &panel.midi {
            let status = midi.status();
            ui.label(if status.connected { "MIDI connected" } else { "MIDI not connected" });
            if let Some(error) = status.error { ui.colored_label(AMBER, error); }
            if let Some(event) = status.last { ui.small(format!("Last input: {event:?}")); }
        }
        ui.small("Connect, then Learn input on a contact. Notes: On/Off; CC: 64–127 down, 0–63 up. Channels are matched. MIDI clock and unrelated messages are ignored.");
    });
}

#[derive(Resource)]
pub(super) struct PracticeSmoke {
    stage: u8,
    started: std::time::Instant,
    marked: std::time::Instant,
    pad: Option<Entity>,
}
impl Default for PracticeSmoke {
    fn default() -> Self {
        Self {
            stage: 0,
            started: std::time::Instant::now(),
            marked: std::time::Instant::now(),
            pad: None,
        }
    }
}
pub(super) fn smoke(
    mut smoke: ResMut<PracticeSmoke>,
    mut display: ResMut<Display>,
    (mut keys, mut mouse): (
        ResMut<ButtonInput<KeyCode>>,
        ResMut<ButtonInput<MouseButton>>,
    ),
    (mut gamepads, windows): (Query<&mut Gamepad>, Query<&Window>),
    mut inputs: Query<&mut bevy_egui::EguiInput>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let panel = &mut display.practice;
    let mut click = None;
    match smoke.stage {
        0 if panel.button.is_some() => {
            click = panel.button.map(|p| (p, true));
            smoke.stage = 1;
        }
        1 => {
            click = panel.button.map(|p| (p, false));
            smoke.stage = 2;
        }
        2 if panel.open && panel.learn_buttons[0].is_some() => {
            smoke.marked = std::time::Instant::now();
            smoke.stage = 30;
        }
        30 if smoke.marked.elapsed().as_millis() > 500 => {
            click = panel.learn_buttons[0].map(|p| (p, true));
            smoke.stage = 3;
        }
        3 => {
            click = panel.learn_buttons[0].map(|p| (p, false));
            smoke.stage = 4;
        }
        4 if panel.learning == Some(0) && !panel.wait_release => {
            keys.press(KeyCode::KeyV);
            smoke.stage = 5;
        }
        5 if panel.bindings[0] == Some(Binding::Key(KeyCode::KeyV)) => {
            keys.release(KeyCode::KeyV);
            click = panel.arm_button.map(|p| (p, true));
            smoke.stage = 6;
        }
        6 => {
            click = panel.arm_button.map(|p| (p, false));
            smoke.stage = 7;
        }
        7 if panel.session.is_some() && !panel.wait_release => {
            keys.press(KeyCode::KeyV);
            smoke.marked = std::time::Instant::now();
            smoke.stage = 8;
        }
        8 if smoke.marked.elapsed().as_millis() > 65 => {
            keys.release(KeyCode::KeyV);
            smoke.stage = 9;
        }
        9 if panel.status.decoded.as_ref().is_some_and(|d| d.text == "E") => {
            keys.press(KeyCode::Escape);
            smoke.stage = 10;
        }
        10 if panel.session.is_none() => {
            keys.release(KeyCode::Escape);
            panel.config.mode = KeyerMode::IambicB;
            click = panel.learn_buttons[1].map(|p| (p, true));
            smoke.stage = 11;
        }
        11 => {
            click = panel.learn_buttons[1].map(|p| (p, false));
            smoke.stage = 12;
        }
        12 if panel.learning == Some(1) && !panel.wait_release => {
            mouse.press(MouseButton::Right);
            smoke.stage = 13;
        }
        13 if panel.bindings[1] == Some(Binding::Mouse(MouseButton::Right)) => {
            mouse.release(MouseButton::Right);
            smoke.pad = Some(commands.spawn(Gamepad::default()).id());
            smoke.marked = std::time::Instant::now();
            smoke.stage = 31;
        }
        31 if smoke.marked.elapsed().as_millis() > 500 => {
            click = panel.learn_buttons[2].map(|p| (p, true));
            smoke.stage = 14;
        }
        14 => {
            click = panel.learn_buttons[2].map(|p| (p, false));
            smoke.stage = 15;
        }
        15 if panel.learning == Some(2) && !panel.wait_release => {
            gamepads
                .get_mut(smoke.pad.unwrap())
                .unwrap()
                .digital_mut()
                .press(GamepadButton::South);
            smoke.stage = 16;
        }
        16 if matches!(
            panel.bindings[2],
            Some(Binding::Gamepad(_, GamepadButton::South))
        ) =>
        {
            gamepads
                .get_mut(smoke.pad.unwrap())
                .unwrap()
                .digital_mut()
                .release(GamepadButton::South);
            smoke.marked = std::time::Instant::now();
            smoke.stage = 32;
        }
        32 if smoke.marked.elapsed().as_millis() > 500 => {
            click = panel.arm_button.map(|p| (p, true));
            smoke.stage = 17;
        }
        17 => {
            click = panel.arm_button.map(|p| (p, false));
            smoke.stage = 18;
        }
        18 if panel.session.is_some() && !panel.wait_release => {
            mouse.press(MouseButton::Right);
            gamepads
                .get_mut(smoke.pad.unwrap())
                .unwrap()
                .digital_mut()
                .press(GamepadButton::South);
            smoke.marked = std::time::Instant::now();
            smoke.stage = 19;
        }
        19 if smoke.marked.elapsed().as_millis() >= 25 => {
            mouse.release(MouseButton::Right);
            gamepads
                .get_mut(smoke.pad.unwrap())
                .unwrap()
                .digital_mut()
                .release(GamepadButton::South);
            smoke.stage = 20;
        }
        20 if panel.status.decoded.as_ref().is_some_and(|d| d.text == "A") => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk("target/practice-smoke.png"));
            smoke.marked = std::time::Instant::now();
            smoke.stage = 21;
        }
        21 if smoke.marked.elapsed().as_millis() > 600 => {
            // Removing an assigned controller must disarm even if its release event is absent.
            gamepads
                .get_mut(smoke.pad.unwrap())
                .unwrap()
                .digital_mut()
                .press(GamepadButton::South);
            smoke.stage = 22;
        }
        22 => {
            commands.entity(smoke.pad.unwrap()).despawn();
            smoke.stage = 23;
        }
        23 if panel.session.is_none() => {
            assert_eq!(
                display.revision, 1,
                "practice must not reconfigure reception"
            );
            info!(
                "Practice smoke passed: keyboard/mouse/controller learning, straight E, iambic B A, Escape, controller disconnect, independent reception"
            );
            exit.write(AppExit::Success);
            smoke.stage = 24;
        }
        _ => {}
    }
    if let Some((pos, pressed)) = click {
        for mut input in &mut inputs {
            input.events.push(egui::Event::PointerMoved(pos));
            input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    if smoke.stage != 24 && smoke.started.elapsed().as_secs() > 25 {
        let message = format!(
            "Practice smoke timed out at stage {}, learning {:?}, wait {}, focus {}, message {}, text {:?}",
            smoke.stage,
            display.practice.learning,
            display.practice.wait_release,
            windows.iter().any(|w| w.focused),
            display.practice.message,
            display.practice.status.decoded
        );
        error!("{message}");
        exit.write(AppExit::error());
    }
}
