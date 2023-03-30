use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse2, Block, PatType};

pub(super) fn test_body(
    block: Block,
    args: &[PatType],
    struct_and_impl: TokenStream,
) -> Block {
    let arg_names = args.iter().map(|arg| {
        let name = arg.pat.to_token_stream().to_string();
        quote!(#name,)
    });

    let strategies = args.iter().map(|arg| {
        let ty = &arg.ty;
        quote!(::proptest::strategy::any::<#ty>(),)
    });

    let patterns = args.iter().map(|arg| {
        let pat = &arg.pat;
        quote!(#pat,)
    });

    let tokens = quote! {{

        #struct_and_impl

        let mut config = ::proptest::test_runner::Config::default();
        config.test_name = Some(concat!(module_path!(), "::", stringify!($test_name)));
        config.source_file = Some(file!());
        let mut runner = ::proptest::test_runner::TestRunner::new(config);
        let names = (#(#arg_names)*);

        let result = runner.run(
            &::proptest::strategy::Strategy::prop_map(( #(#strategies)* ), |values| {
                ::proptest::sugar::NamedArguments(names, values)
            }),
            |::proptest::sugar::NamedArguments(_, ( #(#patterns)* ) )| {
                let _: () = #block
                Ok(())
            },
        );

    }};

    // unwrap here is fine because the double braces create a block
    parse2(tokens).unwrap()
}
