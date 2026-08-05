/// From reference/packages/opencode/src/util/iife.ts
pub fn iife<T>(f: impl FnOnce() -> T) -> T {
    f()
}

#[cfg(test)]
mod tests {
    use super::iife;

    #[test]
    fn immediately_invokes() {
        assert_eq!(iife(|| 42), 42);
    }

    #[test]
    fn runs_side_effect() {
        let mut called = false;
        let result = iife(|| {
            called = true;
            "x"
        });
        assert!(called);
        assert_eq!(result, "x");
    }
}
