pub trait AsyncReader {
    fn async_read(
        &self,
        position: usize,
    ) -> impl std::future::Future<Output = (&crate::messenger::Header, &[u8])>;
}

pub trait AsyncMessageBus: AsyncReader + super::core::Writer + Sync + Send + Clone {}

pub trait AsyncRouter {
    fn route<W: crate::traits::async_traits::AsyncMessageBus + 'static>(
        &mut self,
        header: &crate::messenger::Header,
        buffer: &[u8],
        writer: W,
    ) -> impl std::future::Future<Output = ()>;
}

pub trait AsyncHandle<M: super::core::Message> {
    fn handle<W: super::core::Writer>(
        message: std::sync::Arc<M>,
        writer: W,
    ) -> impl std::future::Future<Output = ()>;
}

pub trait AsyncHandler: super::core::Handler {
    fn async_on_start<W: crate::traits::core::Writer>(
        &mut self,
        _writer: &W,
    ) -> impl std::future::Future<Output = ()> {
        async {}
    }
    fn async_on_loop<W: crate::traits::core::Writer>(
        &mut self,
        _writer: &W,
    ) -> impl std::future::Future<Output = ()> {
        async {}
    }
    fn async_on_stop(&mut self) -> impl std::future::Future<Output = ()> {
        async {}
    }
}
