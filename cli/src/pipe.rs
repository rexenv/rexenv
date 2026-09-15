//! `rex`'s transport on Windows: the running app's named pipe (plan §5 W8 S1; owner rulings D3, Q1, Q5;
//! ledger #630).
//!
//! An overlapped client through tokio, not a blocking handle. Measured on the Dell (`windows_cli_pipe_probe`
//! (d), 15 Sep 2026): Windows serializes synchronous I/O on one file object, so with one thread blocked
//! reading a blocking handle, a write on its `try_clone` returned only when that read did — and the next
//! write never returned. `rex mcp` reads on one thread and writes on another, so on a blocking handle it
//! would deadlock. A tokio client split into halves wrote in 0 ms with its read pending (d2).
//!
//! The rest of `rex` is blocking code written against `UnixStream`'s methods, so `Stream` keeps that shape:
//! each call runs its future to completion on a one-worker runtime the stream owns, and `try_clone` shares
//! the halves, as a duplicated socket shares its connection.

use std::future::Future;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

/// How long a connect waits out a busy pipe before giving its error.
const BUSY_DEADLINE: Duration = Duration::from_secs(5);
/// The pause between two opens of a busy pipe.
const BUSY_PAUSE: Duration = Duration::from_millis(50);

pub struct Stream {
    runtime: Arc<tokio::runtime::Runtime>,
    read: Arc<Mutex<ReadHalf<NamedPipeClient>>>,
    write: Arc<Mutex<WriteHalf<NamedPipeClient>>>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
}

impl Stream {
    /// Open the pipe `name`, waiting out a busy one (`super::open_waiting_out_busy`).
    pub fn connect(name: &Path) -> io::Result<Stream> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build()?;
        let client = {
            let _inside = runtime.enter();
            super::open_waiting_out_busy(|| ClientOptions::new().open(name), BUSY_DEADLINE, BUSY_PAUSE)?
        };
        let (read, write) = tokio::io::split(client);
        Ok(Stream {
            runtime: Arc::new(runtime),
            read: Arc::new(Mutex::new(read)),
            write: Arc::new(Mutex::new(write)),
            read_timeout: None,
            write_timeout: None,
        })
    }

    /// Another handle on the same connection: reads and writes on two threads proceed independently.
    pub fn try_clone(&self) -> io::Result<Stream> {
        Ok(Stream {
            runtime: Arc::clone(&self.runtime),
            read: Arc::clone(&self.read),
            write: Arc::clone(&self.write),
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
        })
    }

    /// A named pipe has no half-close: the server sees end-of-input only when the whole pipe closes. Refused,
    /// so no caller mistakes it for the socket's `shutdown(Write)`.
    pub fn shutdown(&self, _how: std::net::Shutdown) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "a named pipe cannot be half-closed"))
    }

    pub fn set_read_timeout(&mut self, limit: Option<Duration>) -> io::Result<()> {
        self.read_timeout = limit;
        Ok(())
    }

    pub fn set_write_timeout(&mut self, limit: Option<Duration>) -> io::Result<()> {
        self.write_timeout = limit;
        Ok(())
    }
}

/// Run `work` to completion on `runtime`, bounded by `limit` when there is one.
fn within<T>(runtime: &tokio::runtime::Runtime, limit: Option<Duration>, work: impl Future<Output = io::Result<T>>) -> io::Result<T> {
    runtime.block_on(async move {
        match limit {
            Some(limit) => tokio::time::timeout(limit, work)
                .await
                .unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "rexenv did not answer in time"))),
            None => work.await,
        }
    })
}

fn poisoned() -> io::Error {
    io::Error::other("a thread using the pipe panicked")
}

impl io::Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut half = self.read.lock().map_err(|_| poisoned())?;
        within(&self.runtime, self.read_timeout, half.read(buf))
    }
}

impl io::Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut half = self.write.lock().map_err(|_| poisoned())?;
        within(&self.runtime, self.write_timeout, half.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut half = self.write.lock().map_err(|_| poisoned())?;
        within(&self.runtime, self.write_timeout, half.flush())
    }
}
