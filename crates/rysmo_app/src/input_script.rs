//! Scripted pointer and keyboard input for workflow verification.
//!
//! With `RYSMO_SCREENSHOT` set, `RYSMO_INPUT_SCRIPT=file` replays the file's
//! commands as window input, through egui and the tools exactly as a user's mouse and
//! keyboard would. Real input is ignored meanwhile. One command per line; `#` starts a comment. Positions are logical
//! window pixels.
//!
//! | Command | Does |
//! | --- | --- |
//! | `wait N` | Waits N frames. |
//! | `move X Y` | Moves the pointer. |
//! | `press B`, `release B` | Presses or releases `left`, `right` or `middle`. |
//! | `click X Y`, `dclick X Y` | A single or double left click. |
//! | `drag X1 Y1 X2 Y2 [N]` | A left drag in N steps (8). |
//! | `wheel X Y LINES` | Scrolls at a point. |
//! | `touch ID start\|move\|end X Y` | One finger's touch event. |
//! | `swipe ID X1 Y1 X2 Y2 [N]` | One finger down, moved in N steps (8) and lifted. |
//! | `key K`, `hold K`, `unhold K` | Taps, presses or releases a key by its `KeyCode` name. |
//! | `type TEXT` | Types the rest of the line. |
//! | `shot FILE` | Saves a screenshot. |
//! | `save FILE` | Writes the model as `.rysmo` text. |
//! | `log TEXT` | Prints the text with the tool, selection, counts and message. |
//! | `exit` | Quits. |
use bevy::ecs::message::Messages;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::mouse::{MouseButton, MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{CursorMoved, PrimaryWindow, WindowEvent};

use crate::state::App3d;

#[derive(Resource)]
struct Script {
    steps: Vec<Step>,
    next: usize,
    wait: u32,
    pointer: Vec2,
    /// The window size and for how many frames it has held; the window settles its
    /// scale and size after opening, and input before that lands in the wrong place.
    size: Vec2,
    settled: u32,
}

enum Step {
    Wait(u32),
    Move(Vec2),
    Button(MouseButton, bool),
    Wheel(f32),
    Key(KeyCode, bool),
    Touch(u64, TouchPhase, Vec2),
    Text(String),
    Shot(String),
    Save(String),
    Log(String),
    Exit,
}

pub fn plugin(app: &mut App) {
    let Some(path) = std::env::var_os("RYSMO_INPUT_SCRIPT") else { return };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read input script: {e}"));
    let steps = parse(&text).unwrap_or_else(|e| panic!("input script: {e}"));
    app.insert_resource(Script { steps, next: 0, wait: 0, pointer: Vec2::ZERO, size: Vec2::ZERO, settled: 0 })
        .add_systems(First, run);
}

pub fn active() -> bool {
    std::env::var_os("RYSMO_INPUT_SCRIPT").is_some()
}

fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        let nums = || -> Result<Vec<f32>, String> {
            rest.split_whitespace()
                .map(|v| v.parse::<f32>().map_err(|_| format!("line {}: bad number {v}", n + 1)))
                .collect()
        };
        let at = |v: &[f32], i: usize| v.get(i).copied().ok_or(format!("line {}: missing value", n + 1));
        let button = |name: &str| match name {
            "left" => Ok(MouseButton::Left),
            "right" => Ok(MouseButton::Right),
            "middle" => Ok(MouseButton::Middle),
            _ => Err(format!("line {}: unknown button {name}", n + 1)),
        };
        let key = |name: &str| key_code(name).ok_or(format!("line {}: unknown key {name}", n + 1));
        match cmd {
            "wait" => steps.push(Step::Wait(at(&nums()?, 0)? as u32)),
            "move" => {
                let v = nums()?;
                steps.push(Step::Move(Vec2::new(at(&v, 0)?, at(&v, 1)?)));
            }
            "press" => steps.push(Step::Button(button(rest)?, true)),
            "release" => steps.push(Step::Button(button(rest)?, false)),
            "click" | "dclick" => {
                let v = nums()?;
                steps.push(Step::Move(Vec2::new(at(&v, 0)?, at(&v, 1)?)));
                steps.push(Step::Wait(2));
                for _ in 0..if cmd == "click" { 1 } else { 2 } {
                    steps.push(Step::Button(MouseButton::Left, true));
                    steps.push(Step::Wait(1));
                    steps.push(Step::Button(MouseButton::Left, false));
                    steps.push(Step::Wait(1));
                }
                steps.push(Step::Wait(2));
            }
            "drag" => {
                let v = nums()?;
                let (a, b) = (Vec2::new(at(&v, 0)?, at(&v, 1)?), Vec2::new(at(&v, 2)?, at(&v, 3)?));
                let count = v.get(4).copied().unwrap_or(8.).max(1.) as u32;
                steps.push(Step::Move(a));
                steps.push(Step::Wait(2));
                steps.push(Step::Button(MouseButton::Left, true));
                for i in 1..=count {
                    steps.push(Step::Wait(1));
                    steps.push(Step::Move(a.lerp(b, i as f32 / count as f32)));
                }
                steps.push(Step::Wait(1));
                steps.push(Step::Button(MouseButton::Left, false));
                steps.push(Step::Wait(2));
            }
            "wheel" => {
                let v = nums()?;
                steps.push(Step::Move(Vec2::new(at(&v, 0)?, at(&v, 1)?)));
                steps.push(Step::Wait(1));
                steps.push(Step::Wheel(at(&v, 2)?));
                steps.push(Step::Wait(2));
            }
            "touch" => {
                let mut parts = rest.split_whitespace();
                let id =
                    parts.next().and_then(|v| v.parse().ok()).ok_or(format!("line {}: touch needs an id", n + 1))?;
                let phase = match parts.next() {
                    Some("start") => TouchPhase::Started,
                    Some("move") => TouchPhase::Moved,
                    Some("end") => TouchPhase::Ended,
                    _ => return Err(format!("line {}: touch phase is start, move or end", n + 1)),
                };
                let xy: Vec<f32> = parts.filter_map(|v| v.parse().ok()).collect();
                steps.push(Step::Touch(id, phase, Vec2::new(at(&xy, 0)?, at(&xy, 1)?)));
                steps.push(Step::Wait(1));
            }
            "swipe" => {
                let v = nums()?;
                let id = at(&v, 0)? as u64;
                let (a, b) = (Vec2::new(at(&v, 1)?, at(&v, 2)?), Vec2::new(at(&v, 3)?, at(&v, 4)?));
                let count = v.get(5).copied().unwrap_or(8.).max(1.) as u32;
                steps.extend([Step::Touch(id, TouchPhase::Started, a), Step::Wait(1)]);
                for i in 1..=count {
                    steps.extend([
                        Step::Touch(id, TouchPhase::Moved, a.lerp(b, i as f32 / count as f32)),
                        Step::Wait(1),
                    ]);
                }
                steps.extend([Step::Touch(id, TouchPhase::Ended, b), Step::Wait(2)]);
            }
            "key" => {
                let k = key(rest)?;
                steps.extend([Step::Key(k, true), Step::Wait(1), Step::Key(k, false), Step::Wait(2)]);
            }
            "hold" => steps.extend([Step::Key(key(rest)?, true), Step::Wait(1)]),
            "unhold" => steps.extend([Step::Key(key(rest)?, false), Step::Wait(1)]),
            "type" => steps.extend([Step::Text(rest.into()), Step::Wait(2)]),
            "shot" => steps.extend([Step::Wait(3), Step::Shot(rest.into()), Step::Wait(10)]),
            "save" => steps.push(Step::Save(rest.into())),
            "log" => steps.push(Step::Log(rest.into())),
            "exit" => steps.push(Step::Exit),
            _ => return Err(format!("line {}: unknown command {cmd}", n + 1)),
        }
    }
    Ok(steps)
}

fn key_code(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    if let Some(c) = name.strip_prefix("Key").and_then(|c| c.chars().next()).filter(|c| c.is_ascii_uppercase()) {
        return letter(c);
    }
    if let Some(d) = name.strip_prefix("Digit").and_then(|d| d.parse::<u8>().ok()) {
        return digit(d);
    }
    Some(match name {
        "Enter" => Enter,
        "Escape" => Escape,
        "Tab" => Tab,
        "Backspace" => Backspace,
        "Delete" => Delete,
        "Space" => Space,
        "ShiftLeft" => ShiftLeft,
        "ControlLeft" => ControlLeft,
        "AltLeft" => AltLeft,
        "SuperLeft" => SuperLeft,
        "ArrowUp" => ArrowUp,
        "ArrowDown" => ArrowDown,
        "ArrowLeft" => ArrowLeft,
        "ArrowRight" => ArrowRight,
        "F1" => F1,
        "Comma" => Comma,
        "Period" => Period,
        "Minus" => Minus,
        "Slash" => Slash,
        _ => return None,
    })
}

fn letter(c: char) -> Option<KeyCode> {
    use KeyCode::*;
    const KEYS: [KeyCode; 26] = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR,
        KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    KEYS.get((c.to_ascii_uppercase() as u8).checked_sub(b'A')? as usize).copied()
}

fn digit(d: u8) -> Option<KeyCode> {
    use KeyCode::*;
    [Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9].get(d as usize).copied()
}

fn logical(code: KeyCode) -> Key {
    use KeyCode::*;
    match code {
        Enter => Key::Enter,
        Escape => Key::Escape,
        Tab => Key::Tab,
        Backspace => Key::Backspace,
        Delete => Key::Delete,
        Space => Key::Space,
        ShiftLeft => Key::Shift,
        ControlLeft => Key::Control,
        AltLeft => Key::Alt,
        SuperLeft => Key::Super,
        ArrowUp => Key::ArrowUp,
        ArrowDown => Key::ArrowDown,
        ArrowLeft => Key::ArrowLeft,
        ArrowRight => Key::ArrowRight,
        F1 => Key::F1,
        _ => match character(code) {
            Some(c) => Key::Character(c.to_string().into()),
            None => Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        },
    }
}

fn character(code: KeyCode) -> Option<char> {
    use KeyCode::*;
    let name = format!("{code:?}");
    if let Some(c) = name.strip_prefix("Key") {
        return c.chars().next().map(|c| c.to_ascii_lowercase());
    }
    if let Some(d) = name.strip_prefix("Digit") {
        return d.chars().next();
    }
    Some(match code {
        Comma => ',',
        Period => '.',
        Minus => '-',
        Slash => '/',
        _ => return None,
    })
}

fn code_for(c: char) -> KeyCode {
    if c.is_ascii_alphabetic() {
        return letter(c).unwrap_or(KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified));
    }
    if let Some(d) = c.to_digit(10) {
        return digit(d as u8).unwrap_or(KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified));
    }
    match c {
        ',' => KeyCode::Comma,
        '.' => KeyCode::Period,
        '-' => KeyCode::Minus,
        '/' => KeyCode::Slash,
        ' ' => KeyCode::Space,
        _ => KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    mut script: ResMut<Script>,
    app: Res<App3d>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut window_events: ResMut<Messages<WindowEvent>>,
    mut moved: ResMut<Messages<CursorMoved>>,
    mut buttons: ResMut<Messages<MouseButtonInput>>,
    mut wheels: ResMut<Messages<MouseWheel>>,
    mut keys: ResMut<Messages<KeyboardInput>>,
    mut touches: ResMut<Messages<TouchInput>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    let Ok((window, mut w)) = windows.single_mut() else { return };
    // Only the script drives the window: drop real keyboard and pointer input, which
    // would otherwise reach it whenever the user types or moves the mouse meanwhile.
    window_events.clear();
    moved.clear();
    buttons.clear();
    wheels.clear();
    keys.clear();
    touches.clear();
    // Keep the scripted pointer in the window even when the real one is elsewhere.
    // Bypassing change detection stops Bevy from warping the system cursor.
    let pointer = script.pointer;
    w.bypass_change_detection().set_cursor_position(Some(pointer));
    // The window opens without system focus so the user's own typing stays elsewhere;
    // egui still has to treat it as focused to accept the scripted text.
    w.bypass_change_detection().focused = true;
    if script.next == 0 {
        if w.size() != script.size {
            (script.size, script.settled) = (w.size(), 0);
            return;
        }
        script.settled += 1;
        if script.settled < 30 {
            return;
        }
    }
    if script.wait > 0 {
        script.wait -= 1;
        return;
    }
    while script.next < script.steps.len() {
        // Each step runs once, so it can be taken out of the list.
        let index = script.next;
        let step = std::mem::replace(&mut script.steps[index], Step::Wait(0));
        script.next += 1;
        match step {
            Step::Wait(n) => {
                script.wait = n.saturating_sub(1);
                return;
            }
            Step::Move(p) => {
                let delta = Some(p - script.pointer);
                script.pointer = p;
                w.bypass_change_detection().set_cursor_position(Some(p));
                let m = CursorMoved { window, position: p, delta };
                moved.write(m.clone());
                window_events.write(WindowEvent::CursorMoved(m));
            }
            Step::Button(button, down) => {
                let state = if down { ButtonState::Pressed } else { ButtonState::Released };
                let b = MouseButtonInput { button, state, window };
                buttons.write(b);
                window_events.write(WindowEvent::MouseButtonInput(b));
            }
            Step::Wheel(lines) => {
                let m = MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: 0.,
                    y: lines,
                    window,
                    phase: bevy::input::touch::TouchPhase::Moved,
                };
                wheels.write(m);
                window_events.write(WindowEvent::MouseWheel(m));
            }
            Step::Key(code, down) => {
                let state = if down { ButtonState::Pressed } else { ButtonState::Released };
                let text = if down { character(code).map(|c| c.to_string().into()) } else { None };
                keys.write(KeyboardInput {
                    key_code: code,
                    logical_key: logical(code),
                    state,
                    text,
                    repeat: false,
                    window,
                });
            }
            Step::Touch(id, phase, position) => {
                touches.write(TouchInput { phase, position, window, force: None, id });
            }
            Step::Text(text) => {
                for c in text.chars() {
                    let code = code_for(c);
                    let logical_key = if c == ' ' { Key::Space } else { Key::Character(c.to_string().into()) };
                    for state in [ButtonState::Pressed, ButtonState::Released] {
                        keys.write(KeyboardInput {
                            key_code: code,
                            logical_key: logical_key.clone(),
                            state,
                            text: (state == ButtonState::Pressed).then(|| c.to_string().into()),
                            repeat: false,
                            window,
                        });
                    }
                }
            }
            Step::Shot(path) => {
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            }
            Step::Save(path) => {
                if let Err(e) = std::fs::write(&path, rysmo_io::save(app.editor.doc())) {
                    eprintln!("input script: cannot save {path}: {e}");
                }
            }
            Step::Log(text) => {
                let mesh = app.editor.mesh();
                println!(
                    "[{text}] tool={:?} selected={} context={} faces={} edges={} instances={} undo={:?} measure={:?} message={:?}",
                    app.tools.active(),
                    app.editor.selection().len(),
                    app.editor.context().len(),
                    mesh.faces().count(),
                    mesh.edges().count(),
                    app.editor.doc().root.instances.len(),
                    app.editor.undo_label(),
                    app.vcb,
                    app.message,
                );
                let (doc, owner) = (app.editor.doc(), app.editor.active_definition());
                let active = app.editor.active();
                if !active.dimensions.is_empty() || !active.texts.is_empty() {
                    let dimensions: Vec<String> = active
                        .dimensions
                        .values()
                        .map(|d| {
                            let r = d.resolve(doc, owner);
                            format!(
                                "{:?} {}{}",
                                d.label_in_document(&r, doc),
                                if d.text.is_empty() { "" } else { "text " },
                                if r.associated { "attached" } else { "free" }
                            )
                        })
                        .collect();
                    let texts: Vec<String> = active
                        .texts
                        .values()
                        .map(|t| {
                            use rysmo_model::text::{TextLeader, TextPlacement};
                            let kind = match &t.placement {
                                TextPlacement::Screen { .. } => "screen",
                                TextPlacement::Attached { leader: TextLeader::None { .. }, .. } => "attached",
                                TextPlacement::Attached { leader: TextLeader::ViewBased { .. }, .. } => "view-leader",
                                TextPlacement::Attached { leader: TextLeader::Pushpin { .. }, .. } => "pushpin-leader",
                            };
                            format!("{:?} {kind}", t.content)
                        })
                        .collect();
                    println!("[{text} annotations] dimensions={dimensions:?} texts={texts:?}");
                }
                let c = &app.camera;
                println!(
                    "[{text} camera] eye=({:.3},{:.3},{:.3}) target=({:.3},{:.3},{:.3}) distance={:.3} yaw={:.1} pitch={:.1} fov={:.1} parallel={}",
                    c.eye().x,
                    c.eye().y,
                    c.eye().z,
                    c.target.x,
                    c.target.y,
                    c.target.z,
                    c.distance,
                    c.yaw.to_degrees(),
                    c.pitch.to_degrees(),
                    c.fov_y.to_degrees(),
                    c.parallel,
                );
            }
            Step::Exit => {
                exit.write(AppExit::Success);
                return;
            }
        }
    }
}
