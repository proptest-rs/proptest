//-
// Copyright 2019, 2020 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::{spanned::Spanned, FnArg, ItemFn};

#[proc_macro_attribute]
pub fn proptest(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as ItemFn);

    make_test_fn(input).unwrap_or_else(|e| e).into()
}

fn make_test_fn(
    input: ItemFn,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let name = &input.sig.ident;
    let inputs = &input.sig.inputs;
    let body = &input.block;

    let params = inputs
        .iter()
        .map(|input| {
            match input {
                FnArg::Typed(param) => make_proptest_param(param),
                FnArg::Receiver(recv) => Err(quote_spanned! { recv.span() =>
                    compile_error!("The `#[proptest]` macro cannot be applied to a method.");
                })
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let test_fn = quote! {
        #[test]
        fn #name() {
            proptest::proptest!(|(#(#params),*)| {
                #body
            })
        }
    };

    Ok(test_fn)
}

fn make_proptest_param(
    param: &syn::PatType,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let param_name = &param.pat;
    let param_type = &param.ty;

    let strategy = match param.attrs.first() {
        Some(attr) => {
            if !attr.path.is_ident("strategy") {
                return Err(quote_spanned! { attr.span() =>
                    fn test(#attr #param_name: #param_type) {}
                });
            }

            let strategy = match attr.parse_args::<syn::Expr>() {
                Ok(strategy) => strategy,
                Err(e) => return Err(e.to_compile_error()),
            };

            if param.attrs.len() > 1 {
                return Err(quote_spanned! { param.attrs[1].span() =>
                    compile_error!("Expected at maximum one #[strategy] attribute");
                });
            }

            quote! { #strategy }
        }
        None => {
            quote! { ::proptest::arbitrary::any::<#param_type>() }
        }
    };

    Ok(quote! { #param_name in #strategy })
}
