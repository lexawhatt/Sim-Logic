//! Compiles and runs with `--no-default-features --features fonts`: no renderer.
#![cfg(feature = "fonts")]

use sim_logic::fonts::*;

#[test]
fn cpu_font_session_is_available_without_text_or_desktop() -> sim_logic::LogicResult {
    let font = FontFace::from_bytes(
        include_bytes!("assets/text/DejaVuSans.ttf").to_vec(),
        FontBudget::default(),
    )?;
    let style = TextStyle::new(LogicalPixels::new(18.0)?, PhysicalPerLogical::new(1.0)?)?;
    let mut session = TextShapingSession::new(&font, style, TextLayoutBudget::default())?;
    let first = session.shape_line("Ready / Готово")?;
    let second = session.shape_line("Ready / 123")?;
    assert_eq!(first.text(), "Ready / Готово");
    assert_eq!(second.text(), "Ready / 123");
    assert!(!first.glyphs().is_empty());
    assert_eq!(session.cached_plan_count(), 1);
    Ok(())
}
