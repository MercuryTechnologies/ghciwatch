# Rust for Haskellers

## Getting started

Rust is usually built with `cargo` (which is a bit like `cabal` or `stack`) and
`rustc`. The Rust installation is managed with a tool called `rustup` (like
`ghcup`), which can be installed at [rustup.rs][rustup], or using `rustup-init`
(e.g., `brew install rustup-init && rustup-init`).

Once `cargo` is installed, you'll be able to build the project with `cargo build`.

Many tools that are provided separately in Haskell projects are unified into
`cargo` in Rust projects; formatting is `cargo fmt`, linting is `cargo clippy`,
documentation can be generated with `cargo doc`, and so on.

Using the [rust-analyzer] language server is highly recommended.

[rustup]: https://rustup.rs/
[rust-analyzer]: https://rust-analyzer.github.io/


## Syntax and structure

[cheats.rs] has a fantastic overview to the Rust language with plenty of links,
but here’s the basics:

(Run/modify this code on the [Rust Playground!][playground])

```rust
/// A function with no return type returns `()`.
/// These triple-slashed comments are documentation comments, written in
/// Markdown and rendered with `cargo doc`.
fn main() {
    // Types are inferred by default for values but mandatory for functions.
    let hello = "Hello";

    // `println!` is a macro, indicated by the `!` after the name.
    //
    // This format string is translated into a lower-level format at
    // compile-time, and is able to figure out that `{hello}` is the local
    // variable `hello`. Just like Template Haskell!
    println!("{hello}, world!");

    // Now let's create a user.
    let user = User {
        // Here's where Rust starts to differ from Haskell; a string literal is
        // just data in the binary, so we need to copy it into a buffer before we
        // can start modifying it.
        name: "Wiggles".to_owned(),
        age: None,
    };

    // Again, we need to `.clone()` the `user`, or else it'll be gone after this
    // function call. (`how_many_bytes_in_a_name` takes a `User`, not a `&User`
    // reference, so it "owns" its parameter.)
    println!("My name has {} bytes", how_many_bytes_in_a_name(user.clone()));
    match_demo(Some(user));
}

/// Here we're declaring a sum type. `Maybe` is called `Option` in Rust, and it's
/// in the prelude by default.
enum MyOption<T> {
    None,
    Some(T),
}

/// Records are called `struct`s.
/// We can “derive” instances of traits using a `#[derive()]` attribute; this
/// uses a compile-time macro to compute the requested instance.
/// Here, the `Debug` trait lets us print a representation of the object for
/// debugging, and the `Clone` trait lets us deeply copy the object.
#[derive(Debug, Clone)]
struct User {
    name: String,
    age: Option<u16>,
}

/// Of course, we can pattern match on values of all sorts:
fn how_many_bytes_in_a_name(User { name, .. }: User) -> usize {
    // Strings are UTF-8 under the hood, so getting a count of bytes is O(1)
    // and a count of codepoints is O(n).
    name.len()
}

/// We can also pattern match using the `match` expression.
fn match_demo(maybe_user: Option<User>) {
    match maybe_user {
        Some(user) => println!("{user:?}"),
        None => println!("No user found!"),
    }
}
```

[cheats.rs]: https://cheats.rs/
[playground]: https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=31c0cf584fb5b6b71b47e11d1eafec86


## Types galore!

Aside from the surface syntax and the fact that Rust is a strict language,
Haskellers should feel right at home; Rust is based on the same [Hindley-Milner
type system][HM] that Haskell uses, and supports sum types and pattern matching
natively.

Rust has a system of typeclasses and instances that's almost directly copied
from Haskell (Rust calls them “traits”), which even supports fancy features
like associated types.

One of the bigger differences with Rust is its linear type system, which
enforces that values are used “once.” (To be more precise, you can have any
number of immutable references to an object, or one mutable reference, but
never both.) This gives Rust the ability to generate very efficient and
memory-safe code and also equips Rust with a first-class notion of mutability,
which is useful for all the reasons we love immutability in Haskell.

[HM]: https://en.wikipedia.org/wiki/Hindley%E2%80%93Milner_type_system


## Onward!

Read more of that [Rust cheat sheet][cheats.rs], read [the Rust Book][trpl],
and check out the [standard library documentation][std].

[trpl]: https://doc.rust-lang.org/book/
[std]: https://doc.rust-lang.org/stable/std/
