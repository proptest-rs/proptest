use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    parse2, spanned::Spanned, Block, Expr, Ident, PatType, ReturnType, Type,
    TypeTuple,
};

use crate::property_test::options::Options;

use super::{nth_field_name, struct_name};

/// Generate the new test body by putting the struct and arbitrary impl at the start, then adding
/// the usual glue that `proptest!` adds
pub(super) fn body(
    block: Block,
    args: &[PatType],
    struct_and_impl: TokenStream,
    fn_name: &Ident,
    ret_ty: &ReturnType,
    options: &Options, 
) -> Block {
    let struct_name = struct_name(fn_name);

    let errors = &options.errors;

    // convert each arg to `field0: x`
    let struct_fields = args.iter().enumerate().map(|(index, arg)| {
        let pat = &arg.pat;
        let field_name = nth_field_name(arg.pat.span(), index);
        quote!(#field_name: #pat,)
    });


    // e.g. FooArgs { field0: x, field1: (y, z), }
    let struct_pattern = quote! {
        #struct_name { #(#struct_fields)* }
    };

    let handle_result = handle_result(ret_ty);

    let config = make_config(options.config.as_ref());

    let tokens = quote! ( {

        #(#errors)*

        #struct_and_impl

        #config

        let mut runner = ::proptest::test_runner::TestRunner::new(config);
        
        let result = runner.run(
            &::proptest::strategy::Strategy::prop_map(::proptest::prelude::any::<#struct_name>(), |values| {
                ::proptest::sugar::NamedArguments(stringify!(#struct_name), values)
            }),
            |::proptest::sugar::NamedArguments(_, #struct_pattern)| {
                let result = #block;
                #handle_result
            },
        );

        match result {
            Ok(()) => {}
            Err(e) => panic!("{}", e),
        }
    } );

    // unwrap here is fine because the double braces create a block
    parse2(tokens).unwrap()
}

/// rough heuristic for whether we should use result-style syntax - if the function returns either
/// nothing (i.e. `()`) or an empty tuple, it will be non-result handling, otherwise it uses
/// result-style handling
///
/// Note, this won't catch cases like `type Foo = ();`, since type information isn't available yet,
/// it's just looking for the syntax `fn foo() {}` or `fn foo() -> () {}`
fn handle_result(ret_ty: &ReturnType) -> TokenStream {
    let default_body = || quote! { let _ = result; Ok(()) };
    let result_body = || quote! { result };

    match ret_ty {
        ReturnType::Default => default_body(),
        ReturnType::Type(_, ty) => match ty.as_ref() {
            Type::Tuple(TypeTuple { elems, .. }) if elems.is_empty() => {
                default_body()
            }
            _ => result_body(),
        },
    }
}

fn make_config(config: Option<&Expr>) -> TokenStream {
    let trailing = match config {
        None => quote! { ::proptest::test_runner::Config::default() },
        Some(config) => config.to_token_stream(),
    };

    quote! {
        let config = ::proptest::test_runner::Config {
            test_name: Some(concat!(module_path!(), "::", stringify!($test_name))),
            source_file: Some(file!()),
            ..#trailing
        };
    }
}
