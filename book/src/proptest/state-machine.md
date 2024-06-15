# State machine testing

A common use of proptest is to generate inputs that are run against both
a system under test and a reference implementation, validating that the
system under test and the reference implementation exhibit the same
behavior.

The `proptest-state-machine` crate provides a lightweight framework using `proptest` to achieve this.

You can find more detailed documentation [here](https://proptest-rs.github.io/proptest/proptest-state-machine/getting-started.html).
