use proc_macro2::TokenStream;
use quote::quote_spanned;
use syn::{spanned::Spanned, FnArg, ItemFn};

/// Validate an `ItemFn` for some basic sanity checks
///
/// Many checks are deferred to rustc (e.g. rustc already errors if you make a test function
/// unsafe, so we just transparently pass unsafe through to the generated function and let rustc
/// emit the error)
pub(super) fn validate(f: &ItemFn) -> Result<(), TokenStream> {
    all_args_non_self(f)?;

    Ok(())
}

fn all_args_non_self(f: &ItemFn) -> Result<(), TokenStream> {
    let first_self_arg = f
        .sig
        .inputs
        .iter()
        .find(|arg| matches!(arg, FnArg::Receiver(_)));

    match first_self_arg {
        None => Ok(()),
        Some(arg) => err(arg, "`self` parameters are forbidden"),
    }
}

/// Helper function to generate `compile_error!()` outputs
fn err(span: impl Spanned, s: &str) -> Result<(), TokenStream> {
    Err(quote_spanned! { span.span() => compile_error!(#s) })
}

#[cfg(test)]
mod tests {
    use syn::parse_str;

    use super::*;

    #[test]
    fn validate_fails_with_self_arg() {
        let invalids = [
            "fn foo(self) {}",
            "fn foo(&self) {}",
            "fn foo(&mut self) {}",
            "fn foo(self: Self) {}",
            "fn foo(self: &Self) {}",
            "fn foo(self: &mut Self) {}",
            "fn foo(self: Box<Self>) {}",
            "fn foo(self: Rc<Self>) {}",
            "fn foo(self: Arc<Self>) {}",
        ];

        for invalid in invalids {
            let f: ItemFn = parse_str(invalid).unwrap();
            assert!(validate(&f).is_err());
        }
    }
}
