use proc_macro::TokenStream;

mod property_test;

#[proc_macro_attribute]
pub fn property_test(attr: TokenStream, item: TokenStream) -> TokenStream {
    property_test::property_test(attr.into(), item.into()).into()
}
