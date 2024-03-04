use futures::{future::LocalBoxFuture, FutureExt};
use tokio::{
    sync::{mpsc, oneshot},
    task::LocalSet,
};

pub struct LocalSetWorker {
    new_obj: mpsc::UnboundedSender<LocalSetJob>,
}
impl LocalSetWorker {
    pub fn new() -> Self {
        let (new_obj, on_new_obj) = mpsc::unbounded_channel();

        tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(async move {
                LocalSet::new().run_until(Self::run(on_new_obj)).await;
            });
        });

        Self { new_obj }
    }

    pub async fn create<F, T>(&self, f: F) -> LocalGuard<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        let (done, one_done) = oneshot::channel();
        let (job, on_job) = mpsc::unbounded_channel();
        let retain = self.new_obj.clone();

        self.new_obj
            .send(Box::new(move || {
                let obj = f();
                let _ = done.send(());

                LocalGuard::<T>::run(obj, on_job, retain).boxed_local()
            }))
            .unwrap();

        one_done.await.unwrap();

        LocalGuard { job }
    }

    async fn run(mut on_new_obj: mpsc::UnboundedReceiver<LocalSetJob>) {
        while let Some(on_new_obj) = on_new_obj.recv().await {
            tokio::task::spawn_local(on_new_obj());
        }
    }
}
impl Default for LocalSetWorker {
    fn default() -> Self {
        Self::new()
    }
}

type LocalSetJob = Box<dyn FnOnce() -> LocalBoxFuture<'static, ()> + Send>;

pub struct LocalGuard<T> {
    job: mpsc::UnboundedSender<JobCallback<T>>,
}
impl<T> Clone for LocalGuard<T> {
    fn clone(&self) -> Self {
        Self {
            job: self.job.clone(),
        }
    }
}
impl<T> LocalGuard<T> {
    pub async fn interact<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (back, on_back) = oneshot::channel();
        self.job
            .send(Box::new(|obj| {
                let r = f(obj);

                let _ = back.send(r);
            }))
            .unwrap();

        on_back.await.unwrap()
    }

    async fn run(
        mut obj: T,
        mut on_job: mpsc::UnboundedReceiver<JobCallback<T>>,
        retain_worker: mpsc::UnboundedSender<LocalSetJob>,
    ) {
        while let Some(job) = on_job.recv().await {
            (job)(&mut obj);
        }

        drop(obj);
        drop(retain_worker);
    }
}

type JobCallback<T> = Box<dyn FnOnce(&mut T) + Send>;

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn rc_test() {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let worker = LocalSetWorker::new();

                let rc = worker.create(|| Arc::new(())).await;
                let one = rc.interact(|rc| Arc::strong_count(rc)).await;
                assert_eq!(one, 1);

                let two = rc
                    .interact(|rc| {
                        let two = rc.clone();
                        Arc::strong_count(&two)
                    })
                    .await;

                assert_eq!(two, 2);

                let weak = rc.interact(|rc| Arc::downgrade(rc)).await;
                assert_eq!(weak.strong_count(), 1);

                drop(rc);
                for _ in 0..8 {
                    if weak.strong_count() > 0 {
                        let _yield = worker.create(|| ()).await;
                    }
                }
                assert_eq!(weak.strong_count(), 0);
            });
    }
}
