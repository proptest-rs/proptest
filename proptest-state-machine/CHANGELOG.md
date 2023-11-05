## Unreleased

### Bug Fixes

- Removed the limit of number of transitions that can be deleted in shrinking that depended on the number the of transitions given to `prop_state_machine!` or `ReferenceStateMachine::sequential_strategy`.
- Fixed logging of state machine transitions to be enabled when verbose config is >= 1. The "std" feature is added to proptest-state-machine as a default feature that allows to switch the logging off in non-std env.
