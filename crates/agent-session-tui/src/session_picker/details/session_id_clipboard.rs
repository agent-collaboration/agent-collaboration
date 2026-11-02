//! Clipboard command output belongs to this terminal, including an SSH terminal.
use std::io;
use std::sync::Arc;

pub(crate) type SessionClipboardWriter = Arc<dyn Fn(&str) -> io::Result<()> + Send + Sync>;

pub(crate) fn terminal_session_clipboard_writer() -> SessionClipboardWriter {
    Arc::new(|payload| write_terminal_clipboard(&mut io::stdout(), payload))
}

pub(crate) fn write_terminal_clipboard(
    writer: &mut impl io::Write,
    payload: &str,
) -> io::Result<()> {
    use crossterm::ExecutableCommand;
    use crossterm::clipboard::CopyToClipboard;
    writer.execute(CopyToClipboard::to_clipboard_from(payload))?;
    Ok(())
}

pub(crate) fn unavailable_session_clipboard_writer() -> SessionClipboardWriter {
    Arc::new(|_| {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "clipboard writer unavailable",
        ))
    })
}

#[cfg(test)]
mod tests {
    use crate::session_picker::details::session_id_clipboard::*;

    #[test]
    fn owner_details_navigation_copy_uses_exact_terminal_clipboard_command() {
        let mut bytes = Vec::new();
        write_terminal_clipboard(&mut bytes, "thread-a").unwrap();
        assert_eq!(bytes, b"\x1b]52;c;dGhyZWFkLWE=\x1b\\");
    }

    #[test]
    fn owner_details_navigation_copy_returns_writer_failure() {
        struct FailingWriter;
        impl io::Write for FailingWriter {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(
            write_terminal_clipboard(&mut FailingWriter, "thread-a")
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
