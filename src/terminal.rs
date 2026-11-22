use anyhow::Result;
use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableFocusChange, DisableMouseCapture, EnableMouseCapture, PopKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, IsTerminal};

pub struct Session {
    pub enhanced: bool,
}
impl Session {
    pub fn enter() -> Result<Self> {
        anyhow::ensure!(
            io::stdin().is_terminal() && io::stdout().is_terminal(),
            "An interactive terminal is required; use --headless FRAMES for automated runs"
        );
        terminal::enable_raw_mode()?;
        let mut session = Self { enhanced: false };
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide)?;
        session.enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false)
            && execute!(
                io::stdout(),
                crossterm::event::PushKeyboardEnhancementFlags(
                    crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | crossterm::event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                )
            )
            .is_ok();
        execute!(io::stdout(), crossterm::event::EnableFocusChange)?;
        Ok(session)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.enhanced {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            io::stdout(),
            DisableFocusChange,
            DisableMouseCapture,
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}
