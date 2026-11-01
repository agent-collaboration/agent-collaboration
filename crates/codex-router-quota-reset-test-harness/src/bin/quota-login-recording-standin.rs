//! Private synthetic login child: records argv and actual terminal ownership, then exits.
use nix::{
    sys::termios::{self, LocalFlags},
    unistd::{getpgrp, tcgetpgrp},
};
use serde::Serialize;
use std::{
    fs::OpenOptions,
    io::{self, IsTerminal, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process::ExitCode,
};

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum LoginTerminalSnapshot {
    NotTerminal,
    Interactive {
        canonical: bool,
        echo: bool,
        signals: bool,
        foreground: bool,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginStandinReceipt {
    arguments: Vec<String>,
    terminal: LoginTerminalSnapshot,
    exit_code: u8,
}

fn main() -> io::Result<ExitCode> {
    let destination = std::env::var_os("QUOTA_LOGIN_STANDIN_RECEIPT")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| io::Error::other("an absolute private fixture receipt path is required"))?;
    let exit_code = std::env::var("QUOTA_LOGIN_STANDIN_EXIT")
        .ok()
        .map_or(Ok(0), |value| {
            value
                .parse::<u8>()
                .map_err(|_| io::Error::other("invalid fixture exit code"))
        })?;
    let arguments = std::env::args_os()
        .skip(1)
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| io::Error::other("fixture arguments must be UTF-8"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let terminal = if io::stdin().is_terminal() {
        let flags = termios::tcgetattr(io::stdin())
            .map_err(io::Error::other)?
            .local_flags;
        LoginTerminalSnapshot::Interactive {
            canonical: flags.contains(LocalFlags::ICANON),
            echo: flags.contains(LocalFlags::ECHO),
            signals: flags.contains(LocalFlags::ISIG),
            foreground: tcgetpgrp(io::stdin()).map_err(io::Error::other)? == getpgrp(),
        }
    } else {
        LoginTerminalSnapshot::NotTerminal
    };
    let receipt = LoginStandinReceipt {
        arguments,
        terminal,
        exit_code,
    };
    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(destination)?;
    serde_json::to_writer(&mut output, &receipt).map_err(io::Error::other)?;
    writeln!(output)?;
    output.sync_all()?;
    await_owned_fixture_release(&receipt)?;
    println!("Recording login child finished");
    Ok(ExitCode::from(exit_code))
}

fn await_owned_fixture_release(receipt: &LoginStandinReceipt) -> io::Result<()> {
    use std::io::Read;
    use std::os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::UnixStream,
    };
    let Some(path) = std::env::var_os("QUOTA_LOGIN_STANDIN_GATE").map(PathBuf::from) else {
        return Ok(());
    };
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("fixture gate has no parent"))?;
    let metadata = std::fs::symlink_metadata(&path)?;
    if !path.is_absolute()
        || !metadata.file_type().is_socket()
        || std::fs::metadata(parent)?.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "login gate must be an owned socket in a private fixture directory",
        ));
    }
    let mut stream = UnixStream::connect(path)?;
    serde_json::to_writer(&mut stream, receipt).map_err(io::Error::other)?;
    writeln!(stream)?;
    let mut release = [0; 7];
    stream.read_exact(&mut release)?;
    if &release != b"settle\n" {
        return Err(io::Error::other(
            "login fixture gate returned an invalid release",
        ));
    }
    Ok(())
}
