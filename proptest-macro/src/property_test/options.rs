use proc_macro2::TokenStream;
use quote::quote_spanned;
use syn::{
    parse::Parse, punctuated::Punctuated, spanned::Spanned, Expr, Ident,
    LitStr, MetaNameValue, Token,
};

/// Options parsed from the attribute itself (e.g. the config from `#[property_test(config = ...)]`)
#[derive(Default)]
pub(super) struct Options {
    /// Collect compiler errors and emit them later, since errors here are largely recoverable
    pub errors: Vec<TokenStream>,
    pub config: Option<Expr>,
}

impl Parse for Options {
    // note: this impl takes only the contents of the attr, not the attr itself
    // e.g. it will get `foo = bar, baz = qux`, not `#[macro(foo = bar, baz = qux)]`
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let pairs =
            Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;

        let mut errors = Vec::new();

        let mut config = None;

        for MetaNameValue { path, value, .. } in pairs {
            let path_string = path.get_ident().map(Ident::to_string);

            match path_string.as_deref() {
                None => errors.push(quote_spanned!(path.span() => compile_error!("unknown argument"))),
                Some("config") => config = Some(value),
                Some(other) => {
                    let error_message = format!("unknown argument: {other}");
                    let error_message = LitStr::new(&error_message, other.span());
                    let error = quote_spanned!(other.span() => compile_error!(#error_message));
                    errors.push(error);
                }
            }
        }

        Ok(Self { errors, config })
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_str;

    use super::*;

    #[test]
    fn simple_parse_example() {
        let Options { errors, config } =
            parse_str("config = (), random = 123").unwrap();

        assert!(config.is_some());
        assert_eq!(errors.len(), 1);
    }
}
