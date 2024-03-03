struct SendUnsend {
  new_obj: mpsc::UnboundSender<Box<dyn FnOnce() -> Box<dyn Future>>>,
}
impl SendUnsend {
  pub fn new() -> Self {
    let (new_obj, on_new_obj) = mpsc::unbounded_channel();
    tokio::task::spawn_blocking(|| {
      let set = LocalSet::new();
      block_on(async move {
        set.run_until(async move {
          while let Some(on_new_obj) = on_new_obj.recv().await {
            tokio::task::spawn_local(on_new_obj());
          }
        })
    });
  }

  pub async fn create<F, Fut, T>(&self, f: F) -> Wrapper<T>
  where F: FnOnce() -> Fut + Send,
    Fut: Future<Output = T>
  {
    let (done, one_done) = oneshot::channel();
    let (job, on_job) = mpsc::unbound_channel();
    self.new_obj.send(Box::new(|| async move {
      let mut obj = f().await;
      one_done.send(()).await.unwrap();
      while let Some((job, back)) = on_job.recv.await {
        let _ = back.send(job(&mut obj).await).await;
      }
    })).unwrap();

    one_done.await.unwrap();

    Wrapper {
      job
    }
  }
}

struct Wrapper<T> {
  job: mpsc::UnboundedSender<(oneshot::Sender<()>, Box<dyn FnOnce(&mut T))>,
}
impl<T> Wrapper<T> {
  pub async fn interact<F, Fut, R>(&self, f: F) -> ()
    where F: FnOnce(&mut T) -> Fut,
    Fut: Future<Output = () /*TODO work with any*/>,
    {
      let (back, on_back) = oneshot::channel();
      self.job.send((back, Box::new(|obj| async move {
        let r = f(obj).await;
        let _ = back.send(r).await;
      }))).unwrap();

      on_back.await.unwrap()
    }
}
