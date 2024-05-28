# FAQ

## Why a reimplementation?

Ghciwatch started out as a reimplementation and reimagination of
[`ghcid`][ghcid], a similar tool with smaller scope. When we started working on
ghciwatch, `ghcid` suffered from some significant limitations. In particular,
`ghcid` couldn't deal with moved or deleted modules, and wouldn't detect new
directories because it [can't easily update the set of files being watched at
runtime.][ghcid-wait] We've also seen memory leaks requiring multiple restarts
per day. Due to the `ghcid` codebase's relatively small size, a
reimplementation seemed like a more efficient path forward than making
wide-spanning changes to an unfamiliar codebase.

[ghcid]: https://github.com/ndmitchell/ghcid
[ghcid-wait]: https://github.com/ndmitchell/ghcid/blob/e2852979aa644c8fed92d46ab529d2c6c1c62b59/src/Wait.hs#L81-L83

## Why not just use `watchexec` or similar?

TL;DR: Managing a GHCi session is often faster than recompiling a project with
`cabal` or similar.

Recompiling a project when files change is a fairly common development task, so
there's a bunch of tools with this same rough goal. In particular,
[`watchexec`][watchexec] is a nice off-the-shelf solution. Why not just run
`watchexec -e hs cabal build`? In truth, ghciwatch doesn't just recompile the
project when it detects changes. It instead manages an interactive GHCi
session, instructing it to reload modules when relevant. This involves a fairly
complex dance of communicating to GHCi over stdin and parsing its stdout, so
a bespoke tool is useful here.
