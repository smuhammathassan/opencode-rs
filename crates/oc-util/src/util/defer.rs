/// From reference/packages/opencode/src/util/defer.ts
///
/// The reference returns an object that is both `Disposable` and
/// `AsyncDisposable`. `Defer` runs the closure on drop (the `[Symbol.dispose]`
/// path, which fire-and-forgets the closure even for async fns); `AsyncDefer`
/// requires an explicit `dispose().await` (the `[Symbol.asyncDispose]` path).
use std::future::Future;
pub struct Defer<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Drop for Defer<F> {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

pub fn defer<F: FnOnce()>(f: F) -> Defer<F> {
    Defer(Some(f))
}

pub struct AsyncDefer<F: Future<Output = ()>>(Option<F>);

impl<F: Future<Output = ()>> AsyncDefer<F> {
    pub async fn dispose(&mut self) {
        if let Some(f) = self.0.take() {
            f.await;
        }
    }
}

impl<F: Future<Output = ()>> Drop for AsyncDefer<F> {
    fn drop(&mut self) {
        self.0 = None;
    }
}

pub fn defer_async<F: Future<Output = ()>>(f: F) -> AsyncDefer<F> {
    AsyncDefer(Some(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_on_drop() {
        let mut called = false;
        {
            let _guard = defer(|| called = true);
        }
        assert!(called);
    }

    #[tokio::test]
    async fn async_defer_awaits() {
        let mut called = false;
        {
            let mut guard = defer_async(async {
                called = true;
            });
            guard.dispose().await;
        }
        assert!(called);
    }
}
