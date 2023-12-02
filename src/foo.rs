use std::{
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// An `Option<T>` that behaves like a `T`. Avoid paying the cost of `Option` - no unwrapping or
/// matching needed to access the value - when your value is present 99% of the time. For the rare
/// moment where you need to take the value, you just have to be careful (but invariants are
/// enforced with panics, so don't worry too much!).
///
/// An `unsafe`-free alternative to the [`replace_with`](https://docs.rs/replace_with) crate, for
/// temporarily taking ownership of a value through an exclusive/mutable reference.
pub struct Foo<T> {
    value: Option<T>,
    armed: Arc<AtomicBool>,
}

impl<T> Foo<T> {
    pub fn new(value: T) -> Self {
        Self {
            value: Some(value),
            armed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_armed(foo: &Self) -> bool {
        foo.armed.load(Ordering::SeqCst)
    }

    pub fn take(foo: &mut Self) -> (FooBomb, T) {
        let value = foo
            .value
            .take()
            .unwrap_or_else(|| Self::explode(Arc::clone(&foo.armed)));
        foo.armed.store(true, Ordering::SeqCst);
        (
            FooBomb {
                armed: Arc::clone(&foo.armed),
            },
            value,
        )
    }

    pub fn put(foo: &mut Self, value: T) {
        foo.value = Some(value);
        foo.armed.store(false, Ordering::SeqCst);
    }

    fn explode<X>(armed: Arc<AtomicBool>) -> X {
        // Prevent further explosions, which would turn this panic into an abort
        armed.store(false, Ordering::SeqCst);
        panic!("KABOOM");
    }
}

impl<T> Deref for Foo<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.value
            .as_ref()
            .unwrap_or_else(|| Self::explode(Arc::clone(&self.armed)))
    }
}

impl<T> DerefMut for Foo<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
            .as_mut()
            .unwrap_or_else(|| Self::explode(Arc::clone(&self.armed)))
    }
}

impl<T> Drop for Foo<T> {
    fn drop(&mut self) {
        self.armed.store(false, Ordering::SeqCst);
    }
}

pub struct FooBomb {
    armed: Arc<AtomicBool>,
}

impl Drop for FooBomb {
    fn drop(&mut self) {
        if self.armed.load(Ordering::SeqCst) {
            Foo::<()>::explode(Arc::clone(&self.armed))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unarmed() {
        let mut foo = Foo::new(String::from("Hello, world"));
        assert_eq!(foo.len(), 12);
        foo.push('!');
        assert_eq!(foo.len(), 13);
    }

    #[test]
    #[should_panic]
    fn test_armed_bad_drop() {
        let mut foo = Foo::new(42);
        let (_foo_bomb, value) = Foo::take(&mut foo);
        assert_eq!(value, 42);
        // `foo_bomb` detonates to prevent `foo` from escaping this scope without a value
        drop(_foo_bomb); // armed; panics
        drop(foo);
    }

    #[test]
    #[should_panic]
    fn test_armed_bad_deref() {
        let mut foo = Foo::new(42);
        let (_foo_bomb, value) = Foo::take(&mut foo);
        assert_eq!(value, 42);
        // `foo`'s value was taken, so attempts to retrieve it via `Deref` or `DerefMut` will fail
        let _value2 = *foo; // panics
        drop(foo);
    }

    #[test]
    fn test_armed_good_drop() {
        let mut foo = Foo::new(42);
        let (foo_bomb, value) = Foo::take(&mut foo);
        assert_eq!(value, 42);
        // `foo` is dropped, which disarms the bomb
        drop(foo);
        drop(foo_bomb);
    }

    #[test]
    fn test_armed_good_put() {
        let mut foo = Foo::new(42);
        let (foo_bomb, value) = Foo::take(&mut foo);
        assert_eq!(value, 42);
        // `foo`'s value is returned, which disarms the bomb
        Foo::put(&mut foo, 42);
        drop(foo_bomb);
    }
}
