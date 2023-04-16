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

#[cfg(test)]
mod tests {
    use quote::ToTokens;
    use syn::parse_str;

    use super::*;

    #[test]
    fn strip_args_works() {
        let f = parse_str("fn foo(i: i32) {}").unwrap();
        let (f, mut types) = strip_args(f);

        assert_eq!(f.to_token_stream().to_string(), "fn foo () { }");

        assert_eq!(types.len(), 1);
        let ty = types.pop().unwrap();
        assert_eq!(ty.to_token_stream().to_string(), "i : i32");
    }
}
