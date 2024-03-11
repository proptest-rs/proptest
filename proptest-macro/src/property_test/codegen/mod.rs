use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{spanned::Spanned, FnArg, Ident, ItemFn, PatType};

use self::test_body::test_body;

use super::utils::strip_args;

mod test_body;

/// Generate the modified test function
///
/// The rough process is:
///  - strip out the function args from the provided function
///  - turn them into a struct
///  - implement `Arbitrary` for that struct (simple field-wise impl)
///  - create a runner, do the rest
///
///  Currently, any attributes on fields are ignored - in the future, we probably want to read
///  these for things like customizing strategies
pub fn generate(item_fn: ItemFn) -> TokenStream {
    let (mut argless_fn, args) = strip_args(item_fn);

    let struct_tokens = generate_struct(&argless_fn.sig.ident, &args);
    let arb_tokens = generate_arbitrary_impl(&argless_fn.sig.ident, &args);

    let struct_and_tokens = quote! {
        #struct_tokens
        #arb_tokens
    };

    *argless_fn.block = test_body(*argless_fn.block, &args, struct_and_tokens);

    argless_fn.to_token_stream()
}

/// Generate the inner struct that represents the arguments of the function
fn generate_struct(fn_name: &Ident, args: &[PatType]) -> TokenStream {
    let struct_name = struct_name(fn_name);

    let fields = args.iter().enumerate().map(|(index, arg)| {
        let field_name = nth_field_name(&arg.pat, index);
        let ty = &arg.ty;

        quote! { #field_name: #ty, }
    });

    quote! {
        struct #struct_name {
            #(#fields)*
        }
    }
}

/// Generate the arbitrary impl for the struct
fn generate_arbitrary_impl(fn_name: &Ident, args: &[PatType]) -> TokenStream {
    let struct_name = struct_name(fn_name);

    let arg_types = args.iter().map(|arg| {
        let ty = &arg.ty;
        quote!(#ty,)
    });

    let arg_types = quote! { #(#arg_types)* };

    let arg_names = args.iter().enumerate().map(|(index, ty)| {
        let name = nth_field_name(ty.span(), index);
        quote!(#name,)
    });

    let arg_names = quote! { #(#arg_names)* };

    quote! {
        impl ::proptest::prelude::Arbitrary for #struct_name {
            type Parameters = ();
            type Strategy = ::proptest::strategy::Map<::proptest::arbitrary::StrategyFor<(#arg_types)>, fn((#arg_types)) -> Self>;

            fn arbitrary_with((): Self::Parameters) -> Self::Strategy {
                use ::proptest::strategy::Strategy;

                ::proptest::strategy::any::<(#arg_types)>().prop_map(|(#arg_names)| Self { #arg_names })
            }
        }
    }
}

/// Convert the name of a function to the name of a struct representing its args
///
/// E.g. `some_function` -> `SomeFunctionArgs`
fn struct_name(fn_name: &Ident) -> Ident {
    use convert_case::{Case, Casing};

    let name = fn_name.to_string();
    let name = name.to_case(Case::Pascal);
    let name = format!("{name}Args");
    Ident::new(&name, fn_name.span())
}

/// We convert all fields to `"field0"`, etc. to account for various different patterns that can
/// exist in function args. We restore the patterns/bindings when we destructure the struct in the
/// test body
fn nth_field_name(span: impl Spanned, index: usize) -> Ident {
    Ident::new(&format!("field{index}"), span.span())
}

#[cfg(test)]
mod tests {
    use quote::ToTokens;
    use syn::{parse2, parse_str, ItemStruct};

    use super::*;

    /// Simple helper that parses a function, and validates that the struct name and fields are
    /// correct
    fn check_struct(
        fn_def: &str,
        expected_name: &'static str,
        expected_fields: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) {
        let f: ItemFn = parse_str(fn_def).unwrap();
        let (f, args) = strip_args(f);
        let tokens = generate_struct(&f.sig.ident, &args);
        let s: ItemStruct = parse2(tokens).unwrap();

        let fields: Vec<_> = s
            .fields
            .into_iter()
            .map(|field| {
                (
                    field.ident.unwrap().to_string(),
                    field.ty.to_token_stream().to_string(),
                )
            })
            .collect();

        assert_eq!(s.ident.to_string(), expected_name);
        let expected_fields: Vec<_> = expected_fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        assert_eq!(fields, expected_fields);
    }

    #[test]
    fn generates_correct_struct() {
        check_struct("fn foo() {}", "FooArgs", []);
        check_struct("fn foo(x: i32) {}", "FooArgs", [("field0", "i32")]);
        check_struct(
            "fn foo(a: i32, b: String) {}",
            "FooArgs",
            [("field0", "i32"), ("field1", "String")],
        );
    }

    #[test]
    fn generates_arbitrary_impl() {
        let f: ItemFn = parse_str("fn foo(x: i32, y: u8) {}").unwrap();
        let (f, args) = strip_args(f);
        let arb = generate_arbitrary_impl(&f.sig.ident, &args);

        let expected = quote! {
            impl ::proptest::prelude::Arbitrary for FooArgs {
                type Parameters = ();
                type Strategy = ::proptest::strategy::Map<::proptest::arbitrary::StrategyFor<(i32, u8,)>, fn((i32, u8,)) -> Self>;

                fn arbitrary_with((): Self::Parameters) -> Self::Strategy {
                    use ::proptest::strategy::Strategy;

                    ::proptest::strategy::any::<(i32, u8,)>().prop_map(|(field0, field1,)| Self { field0, field1, })
                }

            }
        };

        assert_eq!(arb.to_string(), expected.to_string());
    }
}
