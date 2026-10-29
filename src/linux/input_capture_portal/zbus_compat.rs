use byor::{
    executor::{Executor, Handle, RuntimeExecutor},
    fd::{Fd, RuntimeFd},
    net::{
        RuntimeNet, TcpStream,
        unix::{RuntimeUnixSocket, UnixStream},
    },
    process::{Child, Command, RuntimeProcess},
    time::{RuntimeTime, Sleep},
};
use futures_core::future::BoxFuture;
use futures_util::{FutureExt, TryFutureExt};
use std::{
    marker::PhantomData,
    net::SocketAddr,
    os::fd::{AsRawFd, BorrowedFd},
    path::Path,
};
use zbus::runtime::{
    Interest, IoSource,
    traits::{PollIo, Runtime, TaskHandle},
};

pub struct ZbusCompat<T>(T);
pub type ZbusRt<Rt> = ZbusCompat<(<Rt as RuntimeExecutor>::Executor, PhantomData<Rt>)>;

impl<Rt: RuntimeExecutor> ZbusRt<Rt> {
    pub fn new() -> std::io::Result<Self> {
        let rt = Rt::Executor::new()?;
        Ok(Self((rt, PhantomData)))
    }
}

// Workaround to RTN not supporting type-generic functions
trait ConnectConcreteTcp: TcpStream {
    fn connect_concrete(addr: SocketAddr) -> impl Future<Output = std::io::Result<Self>> {
        Self::connect(addr)
    }
}
impl<T: TcpStream> ConnectConcreteTcp for T {}

trait ConnectConcreteUnix: UnixStream {
    fn connect_concrete(path: &Path) -> impl Future<Output = std::io::Result<Self>> {
        Self::connect(path)
    }
}
impl<T: UnixStream> ConnectConcreteUnix for T {}

impl<
    Rt: RuntimeFd<Fd<IoSource>: Send + Sync + 'static>
        + RuntimeTime<Sleep: Send>
        + RuntimeExecutor<Executor: Send + Sync>
        + RuntimeNet<TcpStream: Send>
        + RuntimeUnixSocket<UnixStream: Send>
        + RuntimeProcess
        + Send
        + Sync
        + 'static,
> Runtime for ZbusRt<Rt>
where
    <<Rt as RuntimeProcess>::Command as Command>::Child: Send,
    <<<Rt as RuntimeProcess>::Command as Command>::Child as Child>::status(..): Send,
    <<Rt as RuntimeNet>::TcpStream as ConnectConcreteTcp>::connect_concrete(..): Send,
    <<Rt as RuntimeUnixSocket>::UnixStream as ConnectConcreteUnix>::connect_concrete(..): Send,
{
    type RegisteredIoSource = ZbusCompat<Rt::Fd<IoSource>>;
    type Sleep = BoxFuture<'static, ()>;
    type Task<T: Send + 'static> = ZbusCompat<<Rt::Executor as Executor>::Handle<T>>;

    fn register_io_source(&self, source: IoSource) -> std::io::Result<Self::RegisteredIoSource> {
        Rt::Fd::new(source).map(ZbusCompat)
    }

    fn sleep(&self, duration: std::time::Duration) -> Self::Sleep {
        Box::pin(Rt::Sleep::after(duration).map(|_| ()))
    }

    fn spawn<T>(
        &self,
        _name: &str,
        future: impl Future<Output = T> + Send + 'static,
    ) -> Self::Task<T>
    where
        T: Send + 'static,
    {
        ZbusCompat(self.0.0.spawn(future))
    }

    fn connect_tcp(
        &self,
        address: std::net::SocketAddr,
    ) -> impl Future<Output = std::io::Result<IoSource>> + Send {
        Rt::TcpStream::connect_concrete(address).and_then(async |s| {
            Ok(unsafe { BorrowedFd::borrow_raw(s.as_raw_fd()) }
                .try_clone_to_owned()?
                .into())
        })
    }

    fn connect_unix(
        &self,
        path: &std::path::Path,
    ) -> impl Future<Output = std::io::Result<IoSource>> + Send {
        Rt::UnixStream::connect_concrete(path).and_then(async |s| {
            Ok(unsafe { BorrowedFd::borrow_raw(s.as_raw_fd()) }
                .try_clone_to_owned()?
                .into())
        })
    }

    fn spawn_process(
        &self,
        command: std::process::Command,
        stdin: std::process::Stdio,
        stdout: std::process::Stdio,
        stderr: std::process::Stdio,
    ) -> std::io::Result<
        std::pin::Pin<
            Box<dyn Future<Output = std::io::Result<std::process::ExitStatus>> + Send + 'static>,
        >,
    > {
        Rt::Command::from(command)
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .map(
                |mut v| -> std::pin::Pin<Box<dyn Future<Output = _> + Send + 'static>> {
                    Box::pin(async move { v.status().await })
                },
            )
    }
}

impl<F: Fd<Inner = IoSource> + Send + Sync + 'static> PollIo for ZbusCompat<F> {
    fn poll_io<T>(
        &self,
        cx: &mut std::task::Context<'_>,
        interest: zbus::runtime::Interest,
        mut operation: impl FnMut() -> std::io::Result<T>,
    ) -> std::task::Poll<std::io::Result<T>> {
        match interest {
            Interest::Readable => self.0.poll_readable(cx),
            Interest::Writable => self.0.poll_writable(cx),
            _ => unimplemented!("interest {interest:?}"),
        }
        .map(|r| r.and_then(move |()| operation()))
    }
}

impl<T: Send + 'static, H: Handle<Out = T> + Send + Sync + Unpin + 'static> TaskHandle<T>
    for ZbusCompat<H>
{
    fn detach(self) {
        self.0.detach()
    }
}

impl<H: Handle> Future for ZbusCompat<H> {
    type Output = std::io::Result<H::Out>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        unsafe { self.map_unchecked_mut(|v| &mut v.0) }
            .poll(cx)
            .map(H::io_result)
    }
}
