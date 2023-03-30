use core::mem::replace;

use syn::{punctuated::Punctuated, FnArg, ItemFn, PatType};

/// Convert a function to a zero-arg function, and return the args
///
/// Panics if any arg is a receiver (i.e. `self` or a variant)
pub fn strip_args(mut f: ItemFn) -> (ItemFn, Vec<PatType>) {
    let args = replace(&mut f.sig.inputs, Punctuated::new());
    (f, args.into_iter().map(|arg| match arg {
        FnArg::Typed(arg) => arg,
        FnArg::Receiver(_) => panic!("receivers aren't allowed - should be filtered by `validate`"),
    }).collect())
}
