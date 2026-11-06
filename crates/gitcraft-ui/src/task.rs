//! Work that `App::update` hands back to the shell: messages to deliver right away, or
//! futures and streams whose output becomes messages later.
//!
//! This is deliberately tiny and knows nothing about the GUI toolkit, so tests can run
//! a task to its end with any executor.

use std::future::Future;
use std::pin::Pin;

use futures::future::{AbortHandle, Abortable};
use futures::stream::{self, BoxStream, Stream, StreamExt};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

enum Job<T> {
    Now(T),
    Future(BoxFuture<T>),
    Stream(BoxStream<'static, T>),
}

pub struct Task<T>(Vec<Job<T>>);

/// Stops a task that was made [`abortable`](Task::abortable).
#[derive(Debug, Clone)]
pub struct Handle(AbortHandle);

impl Handle {
    pub fn abort(&self) {
        self.0.abort();
    }
}

impl<T: Send + 'static> Task<T> {
    pub fn none() -> Self {
        Self(Vec::new())
    }

    /// A message to deliver as soon as possible.
    pub fn done(message: T) -> Self {
        Self(vec![Job::Now(message)])
    }

    pub fn batch(tasks: impl IntoIterator<Item = Self>) -> Self {
        Self(tasks.into_iter().flat_map(|task| task.0).collect())
    }

    /// Runs a future, then turns its output into a message.
    pub fn perform<A: Send + 'static>(
        future: impl Future<Output = A> + Send + 'static,
        map: impl FnOnce(A) -> T + Send + 'static,
    ) -> Self {
        Self::future(async move { map(future.await) })
    }

    pub fn future(future: impl Future<Output = T> + Send + 'static) -> Self {
        Self(vec![Job::Future(Box::pin(future))])
    }

    pub fn stream(stream: impl Stream<Item = T> + Send + 'static) -> Self {
        Self(vec![Job::Stream(stream.boxed())])
    }

    pub fn is_none(&self) -> bool {
        self.0.is_empty()
    }

    /// Everything the task will produce, as one stream. `None` when there is nothing to do.
    pub fn into_stream(self) -> Option<BoxStream<'static, T>> {
        if self.0.is_empty() {
            return None;
        }
        let parts: Vec<BoxStream<'static, T>> = self
            .0
            .into_iter()
            .map(|job| match job {
                Job::Now(message) => stream::once(async move { message }).boxed(),
                Job::Future(future) => stream::once(future).boxed(),
                Job::Stream(stream) => stream,
            })
            .collect();
        Some(stream::select_all(parts).boxed())
    }

    /// The same task, which stops producing output (and drops its futures, ending any
    /// process they own) when the handle is used.
    pub fn abortable(self) -> (Self, Handle) {
        let (handle, registration) = AbortHandle::new_pair();
        let task = match self.into_stream() {
            Some(stream) => Self::stream(Abortable::new(stream, registration)),
            None => Self::none(),
        };
        (task, Handle(handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_everything_and_stops_when_aborted() {
        let task = Task::batch([
            Task::done(1),
            Task::perform(async { 2 }, |n| n + 1),
            Task::stream(stream::iter([10, 11])),
        ]);
        let mut seen: Vec<i32> = task.into_stream().unwrap().collect().await;
        seen.sort_unstable();
        assert_eq!(seen, [1, 3, 10, 11]);
        assert!(Task::<i32>::none().into_stream().is_none());

        let (task, handle) = Task::<i32>::future(std::future::pending()).abortable();
        handle.abort();
        assert!(task.into_stream().unwrap().next().await.is_none());
    }
}
