use proc_macro2::TokenStream;
use syn::parse2;

use self::validate::validate;

mod codegen;
mod utils;
mod validate;

#[cfg(test)]
mod tests;

pub fn property_test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_fn = match parse2(item) {
        Ok(item_fn) => item_fn,
        Err(e) => return e.into_compile_error(),
    };

    if let Err(compile_error) = validate(&item_fn) {
        return compile_error;
    }

    codegen::generate(item_fn)
}
