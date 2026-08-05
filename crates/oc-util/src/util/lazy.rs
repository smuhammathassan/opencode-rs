/// From reference/packages/opencode/src/util/lazy.ts
///
/// A memoized value computed on first access, with `reset()` and `loaded()`.
use std::sync::{Arc, Mutex};

struct Inner<T, F> {
    init: F,
    value: Option<Arc<T>>,
}

pub struct Lazy<T, F = fn() -> T> {
    inner: Mutex<Inner<T, F>>,
}

impl<T, F: FnMut() -> T> Lazy<T, F> {
    pub fn new(init: F) -> Self {
        Lazy {
            inner: Mutex::new(Inner { init, value: None }),
        }
    }

    pub fn get(&self) -> Arc<T> {
        let mut inner = self.inner.lock().expect("lazy poisoned");
        if let Some(value) = &inner.value {
            return Arc::clone(value);
        }
        let value = Arc::new((inner.init)());
        inner.value = Some(Arc::clone(&value));
        value
    }

    pub fn reset(&self) {
        self.inner.lock().expect("lazy poisoned").value = None;
    }

    pub fn loaded(&self) -> bool {
        self.inner.lock().expect("lazy poisoned").value.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::Lazy;

    #[test]
    fn computes_once() {
        let mut calls = 0;
        let lazy = Lazy::new(|| {
            calls += 1;
            7
        });
        assert!(!lazy.loaded());
        assert_eq!(*lazy.get(), 7);
        assert_eq!(*lazy.get(), 7);
        assert!(lazy.loaded());
        assert_eq!(calls, 1);
    }

    #[test]
    fn reset_recomputes() {
        let mut calls = 0;
        let lazy = Lazy::new(|| {
            calls += 1;
            calls
        });
        assert_eq!(*lazy.get(), 1);
        lazy.reset();
        assert!(!lazy.loaded());
        assert_eq!(*lazy.get(), 2);
    }
}
