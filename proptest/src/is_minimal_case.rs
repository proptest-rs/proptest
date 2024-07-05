use core::cell::Cell;

thread_local! {
    static IS_MINIMAL_CASE: Cell<bool> = Cell::new(false);
}

/// When run inside a property test, indicates whether the current case being tested
/// is the minimal test case.
///
/// `proptest` typically runs a large number of test cases for each
/// property test. If it finds a failing test case, it tries to shrink it
/// in the hopes of finding a simpler test case. When debugging a failing
/// property test, we are often only interested in the actual minimal
/// failing case. After the minimal test case has been identified,
/// the test is rerun with the minimal input, and this function
/// returns `true` when called inside the test.
///
/// The results are undefined if property tests are nested, meaning that a property test
/// is run inside another property test.
///
/// # Example
///
/// ```rust
/// use proptest::{proptest, prop_assert, is_minimal_case};
/// # fn export_to_file_for_analysis() {}
///
/// proptest! {
///     #[test]
///     fn test_is_not_five(num in 0 .. 10) {
///         if is_minimal_case() {
///             eprintln!("Minimal test case is {num:?}");
///             export_to_file_for_analysis(num);
///         }
///
///         prop_assert!(num != 5);
///     }
/// }
/// ```
pub fn is_minimal_case() -> bool {
    IS_MINIMAL_CASE.get()
}

/// Helper struct that helps to ensure panic safety when entering a minimal case.
///
/// Specifically, if the test case panics, we must ensure that we still
/// correctly reset the thread-local variable.
#[non_exhaustive]
pub(crate) struct MinimalCaseGuard;

impl MinimalCaseGuard {
    pub(crate) fn begin_minimal_case() -> Self {
        IS_MINIMAL_CASE.replace(true);
        Self
    }
}

impl Drop for MinimalCaseGuard {
    fn drop(&mut self) {
        IS_MINIMAL_CASE.replace(false);
    }
}