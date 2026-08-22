use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Register global keyboard shortcuts that work from any application.
pub fn setup_shortcuts(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let toggle_visibility =
        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS);
    let trigger_screenshot =
        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyX);
    let focus_input =
        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyF);
    let push_to_talk = Shortcut::new(Some(Modifiers::CONTROL), Code::Space);

    app.global_shortcut().on_shortcut(toggle_visibility, {
        let app = app.clone();
        move |_app_handle, shortcut, event| {
            if event.state == ShortcutState::Pressed && shortcut == &toggle_visibility {
                let _ = app.emit_to("main", "toggle-visibility", ());
            }
        }
    })?;

    app.global_shortcut().on_shortcut(trigger_screenshot, {
        let app = app.clone();
        move |_app_handle, shortcut, event| {
            if event.state == ShortcutState::Pressed && shortcut == &trigger_screenshot {
                let _ = app.emit_to("main", "trigger-screenshot", ());
            }
        }
    })?;

    app.global_shortcut().on_shortcut(focus_input, {
        let app = app.clone();
        move |_app_handle, shortcut, event| {
            if event.state == ShortcutState::Pressed && shortcut == &focus_input {
                let _ = app.emit_to("main", "focus-text-input", ());
            }
        }
    })?;

    app.global_shortcut().on_shortcut(push_to_talk, {
        let app = app.clone();
        move |_app_handle, shortcut, event| {
            if event.state == ShortcutState::Pressed && shortcut == &push_to_talk {
                let _ = app.emit_to("main", "push-to-talk", ());
            }
        }
    })?;

    Ok(())
}
