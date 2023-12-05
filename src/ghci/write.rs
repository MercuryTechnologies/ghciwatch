use async_dup::Arc;
use async_dup::Mutex;
use dyn_clone::DynClone;
use std::fmt::Debug;
use tokio::io::AsyncWrite;
use tokio_util::compat::FuturesAsyncWriteCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;

/// An [`AsyncWrite`]r usable in `GhciOpts`. In particular, it must implement [`Clone`], [`Send`],
/// and [`Sync`].
///
/// Use [`IntoGhciWrite`] to convert a qualifying [`AsyncWrite`]r into a [`GhciWrite`]r.
pub trait GhciWrite: AsyncWrite + DynClone + Debug + Send + Sync + Unpin {}

impl<W> GhciWrite for W where W: AsyncWrite + DynClone + Debug + Send + Sync + Unpin {}

dyn_clone::clone_trait_object!(GhciWrite);

/// Convert a qualifying [`AsyncWrite`]r into a form usable in `GhciOpts`.
pub trait IntoGhciWrite {
    fn into_ghci_write(self) -> Box<dyn GhciWrite>;
}

impl<W> IntoGhciWrite for W
where
    W: AsyncWrite + TokioAsyncWriteCompatExt + Unpin + Send + Sized + Debug + 'static,
{
    fn into_ghci_write(self) -> Box<dyn GhciWrite> {
        Box::new(FuturesAsyncWriteCompatExt::compat_write(Arc::new(
            Mutex::new(TokioAsyncWriteCompatExt::compat_write(self)),
        )))
    }
}
