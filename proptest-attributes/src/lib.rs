extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::{spanned::Spanned, Expr, FnArg, ItemFn};

#[proc_macro_attribute]
pub fn proptest(
    attr: TokenStream,
    item: TokenStream,
) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(item as ItemFn);
    let expr = syn::parse_macro_input!(attr as Expr);

    let ret = &input.sig.output;
    let name = &input.sig.ident;
    let body = &input.block;
    let attrs = &input.attrs;
    let inputs = &input.sig.inputs;

    match inputs.len() {
        1 => {
            let param = match inputs.first().unwrap() {
                FnArg::Typed(param) => param,
                FnArg::Receiver(recv) => {
                    let tokens = quote_spanned! { recv.span() =>
                        compile_error!("The `#[proptest]` macro cannot be applied to a method.");
                    };
                    return TokenStream::from(tokens);
                }
            };
            let param_name = &param.pat;
            quote! {
                #[test]
                #(#attrs)*
                fn #name() #ret {
                    proptest::proptest!(|(#param_name in #expr)| {
                        #body
                    })
                }
            }
        }
        _ => {
            let tokens = quote_spanned! { input.sig.span() =>
                compile_error!("The `#[proptest]` macro can only be applied to a function with a single argument.");
            };
            return TokenStream::from(tokens);
        }
    }
    .into()
}
