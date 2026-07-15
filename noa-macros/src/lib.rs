use proc_macro::TokenStream;
use quote::quote;
use std::{cell::RefCell, rc::Rc};
use syn::parse_macro_input;

#[proc_macro_attribute]
pub fn generate_document(_attr: TokenStream, input: TokenStream) -> TokenStream {
    input
}
