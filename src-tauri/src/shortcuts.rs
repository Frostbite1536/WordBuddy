use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Register global keyboard shortcuts that work from any application.
pub fn setup_shortcuts(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let toggle_visibility =
        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS);
    let selection_rewrite =
        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyW);
    let focus_input =
        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyF);

    app.global_shortcut().on_shortcut(toggle_visibility, {
        let app = app.clone();
        move |_app_handle, shortcut, event| {
            if event.state == ShortcutState::Pressed && shortcut == &toggle_visibility {
                let _ = app.emit_to("main", "toggle-visibility", ());
            }
        }
    })?;

    // PLAN-04 selection rewrite palette: capture selection, open the
    // palette mode of the widget window.
    app.global_shortcut().on_shortcut(selection_rewrite, {
        let app = app.clone();
        move |_app_handle, shortcut, event| {
            if event.state == ShortcutState::Pressed && shortcut == &selection_rewrite {
                let _ = app.emit_to("main", "selection-rewrite", ());
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

    Ok(())
}
