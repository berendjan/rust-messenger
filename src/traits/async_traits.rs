pub trait AsyncReader {
    // async fn read(&self, position: usize) -> (&crate::messenger::Header, &[u8]);
    // fn read(&self, position: usize) -> impl std::future::Future<Output = (&crate::messenger::Header, &[u8])> + Send;
}

pub trait AsyncMessageBus: AsyncReader + super::core::Writer + Sync + Send {}
