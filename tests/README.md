# Ghciwatch testing

Ghciwatch is mostly tested with integration tests.

Use `cargo nextest run` to run them.

The integration tests launch ghciwatch, write logs to a JSON file, perform
actions which ghciwatch will respond to, and then wait for specific log
messages to be emitted.


## Test harness macro

Tests are annotated with the `#[test_harness::test]` proc macro, which
generates multiple tests for each test function: one for each version in the
`$GHC_VERSIONS` environment variable, which should look like this:

```
$ echo "$GHC_VERSIONS"
9.6.7 9.8.4 9.10.2 9.12.2
```

So a test like `can_detect_compilation_failure` will expand into tests
`can_detect_compilation_failure_967`, `can_detect_compilation_failure_984`,
`can_detect_compilation_failure_9102`, and so on.


## Feedback speed / reliability tradeoff

In general, ghciwatch performs unpredictable actions (like compiling a Cabal
project!), so we don't know that something _won't_ happen, just that it hasn't
happened _yet._ This means the tests are pretty heavily timeout based.

There's a tradeoff between feedback speed and reliability here: if ghciwatch
tends to complete an action in 3 seconds and we have a 10 second timeout, that
means that if the test fails, we _cannot_ know that it's failed until the 10
second timeout has expired.

If we decrease the timeout, then we will get feedback for failing tests faster,
at the cost of flaky tests: sometimes conditions will conspire (background
system load, the whims of the OS scheduler, etc.) to make ghciwatch take a
little longer than normal, and a test will fail because it didn't wait enough.

Note that this only applies to _failing_ tests. If we're willing to wait 10
seconds for a log message, but the message appears after 1 second, we'll
consume that log message and move on. Longer timeouts only make _failing_ tests
slower.


### `$TIMEOUT_MULT`

The `$TIMEOUT_MULT` environment variable is a way to adjust the timeouts
dynamically; our CI runners tend to be slower than our developer workstations,
for example.

The more tests you'd like to run at once, the slower they get. The default
timeouts are fine when I'm running tests at `-j 16`, but at `-j 64` they fail
frequently, just because all the underlying operations have a harder time
contending for resources.

Therefore, you can run something like `TIMEOUT_MULT=2 cargo nextest run
--test-threads 64` to give your tests a little more wiggle room without
changing hard-coded defaults or resorting to retries.
