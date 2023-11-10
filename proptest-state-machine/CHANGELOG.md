## Unreleased

### Other Notes

- `message-io` updated from 0.17 to 0.18

### Bug Fixes

- Fixed state-machine macro's inability to handle missing config
- Fixed logging of state machine transitions to be enabled when verbose config is >= 1. The "std" feature is added to proptest-state-machine as a default feature that allows to switch the logging off in non-std env.
