//! PLAN-08 §5 offset-semantics probe (Linux / AT-SPI).
//!
//! Never assume text-unit semantics on a new platform — measure them. This
//! binary drives a REAL focused editable field (gedit works unmodified;
//! Electron targets need `ACCESSIBILITY_ENABLED=1`) and answers: do AT-SPI
//! Text offsets count Unicode characters (the spec's intent) or something
//! else (bytes / UTF-16 units) for the toolkit actually in front of you?
//!
//! Procedure:
//!   1. Requires a running at-spi2-core daemon.
//!   2. Run `cargo run --example atspi_probe`.
//!   3. Type/paste the marker string into the focused editor when prompted.
//!   4. The probe compares character_count against scalar/UTF-16 lengths,
//!      then selects (0, n) via SetSelection and reads back what one unit
//!      spans.
//!
//! Record the observed result next to `select_range`'s comment block in
//! apply.rs before wiring any span selection on this platform.

// Linux-only probe, but `cargo test` compiles examples everywhere — the
// real code lives in a cfg-gated module and a no-op main keeps other
// platforms valid.
#[cfg(target_os = "linux")]
mod probe {
    use std::collections::VecDeque;
    use std::io::{BufRead, Write};

    use atspi::proxy::accessible::AccessibleProxy;
    use atspi::proxy::text::TextProxy;
    use atspi::zbus::names::UniqueName;
    use atspi::zbus::zvariant::ObjectPath;
    use atspi::{AccessibilityConnection, CoordType, Role, State};

    /// Same marker as ax_probe: mixes 1-unit ASCII, a surrogate pair, and a
    /// combining sequence so every counting scheme disagrees.
    const MARKER: &str = "a\u{1F600}b e\u{301}x";

    async fn accessible(
        conn: &AccessibilityConnection,
        name: &UniqueName<'static>,
        path: &ObjectPath<'static>,
    ) -> Option<AccessibleProxy<'static>> {
        AccessibleProxy::builder(&conn.connection())
            .destination(name.clone())
            .ok()?
            .path(path.clone())
            .ok()?
            .build()
            .await
            .ok()
    }

    async fn text_proxy(
        conn: &AccessibilityConnection,
        name: &UniqueName<'static>,
        path: &ObjectPath<'static>,
    ) -> Option<TextProxy<'static>> {
        TextProxy::builder(&conn.connection())
            .destination(name.clone())
            .ok()?
            .path(path.clone())
            .ok()?
            .build()
            .await
            .ok()
    }

    /// Breadth-first search under the active frame for an editable text node.
    async fn find_focused_text(
        conn: &AccessibilityConnection,
    ) -> Option<(UniqueName<'static>, ObjectPath<'static>, TextProxy<'static>)> {
        let root = conn.root_accessible_on_registry().await.ok()?;
        let mut queue: VecDeque<(Option<UniqueName<'static>>, ObjectPath<'static>)> = root
            .get_children()
            .await
            .ok()?
            .into_iter()
            .map(|r| (r.name().cloned(), r.path().clone()))
            .collect();

        while let Some((name, path)) = queue.pop_front() {
            let Some(name) = name else { continue };
            let Some(node) = accessible(conn, &name, &path).await else {
                continue;
            };
            if matches!(node.get_role().await, Ok(Role::Entry | Role::Text))
                && matches!(node.get_state().await, Ok(s) if s.contains(State::Focused | State::Active))
            {
                if let Some(tp) = text_proxy(conn, &name, &path).await {
                    return Some((name, path, tp));
                }
            }
            if let Ok(children) = node.get_children().await {
                queue.extend(
                    children
                        .into_iter()
                        .take(200)
                        .map(|r| (r.name().cloned(), r.path().clone())),
                );
            }
        }
        None
    }

    pub(super) fn run() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        println!(
            "UTF-16 length = {}, char (scalar) length = {}, byte length = {}",
            MARKER.encode_utf16().count(),
            MARKER.chars().count(),
            MARKER.len()
        );
        print!("Type/paste the marker into gedit's focused document, then press Enter here... ");
        std::io::stdout().flush().ok();
        std::io::stdin().lock().lines().next();

        rt.block_on(async move {
            let conn = match AccessibilityConnection::new().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("AT-SPI2 bus unavailable ({e}) — is at-spi2-core running?");
                    std::process::exit(1);
                }
            };
            let Some((dest, path, text)) = find_focused_text(&conn).await else {
                eprintln!("No focused editable Text node found — focus gedit and retry.");
                std::process::exit(1);
            };
            drop((dest, path));

            let count = text.character_count().await.unwrap_or(0);
            println!(
                "\ncharacter_count = {count} | scalars = {} | utf16 = {} | bytes = {}",
                MARKER.chars().count(),
                MARKER.encode_utf16().count(),
                MARKER.len()
            );

            // Select (0, n) and read back what n units span.
            for n in 1..=count.min(12) {
                if !matches!(text.set_selection(0, 0, n).await, Ok(true)) {
                    println!("n={n} -> SetSelection failed");
                    continue;
                }
                let Ok((s, e)) = text.get_selection(0).await else {
                    continue;
                };
                let got = text.get_text(0, e.max(s)).await.unwrap_or_default();
                let got_chars = got.chars().count();
                let span = (e - s).max(0) as usize;
                let verdict = if got_chars == span && got == MARKER.get(..span).unwrap_or("") {
                    "SCALAR-consistent"
                } else if got.encode_utf16().count() == span {
                    "UTF-16-consistent"
                } else {
                    "?"
                };
                println!("select(0,{n}) -> range=({s},{e}) text={got:?} [{verdict}]");
            }
            let _ = CoordType::Screen; // referenced so the import documents intent
            println!("\nRecord which interpretation matched in apply.rs before wiring spans.");
        });
    }
}

// Unconditional crate-level entry: dispatches to the Linux-only probe.
fn main() {
    #[cfg(target_os = "linux")]
    probe::run();
}
