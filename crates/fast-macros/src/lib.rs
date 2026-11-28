//! Procedural attribute route macros (#[get], #[post], #[delete]) mirroring FastAPI decorators.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, ItemFn, LitStr, Visibility};

// Rewrites as async route handler into a factory function returning fast_rs::Route.
fn expand_route(method_name: &str, attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse the string literal inside #get("/path")] and the function below it.
    let path = parse_macro_input!(attr as LitStr);
    let mut func = parse_macro_input!(item as ItemFn);

    // Reject non-async handlers at compile time with an error pointing at 'fn'.
    if func.sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            func.sig.fn_token,
            "fast-rs route handlers must be declared as 'async fn'",
        )
        .to_compile_error()
        .into();
    }

    // Save the original visiblity ('pub' or private) and function name for the outer wrapper.
    let vis = func.vis.clone();
    let fn_name = func.sig.ident.clone();
    let fn_name_str = fn_name.to_string();
    let method_ident = format_ident!("{}", method_name);

    // Rename the original async fn so it lives privately inside the wrapper function.
    func.vis = Visibility::Inherited;
    func.sig.ident = format_ident!("__fast_handler");

    // Print the final rewritten Rust code and convert it back into a TokenStream.
    let expanded = quote! {
        #vis fn #fn_name() -> fast_rs::Route {
            #func

            fast_rs::Route::new(
                fast_rs::Method::#method_ident,
                #path,
                #fn_name_str,
                __fast_handler,
            )
        }
    };

    expanded.into()
}

// Registers an async hadnler for HTTP GET rquests (e.g. #get("/tasks")]).
#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_route("Get", attr, item)
}

// Registers an async handler for HTTP POST requests (e.g. #[post("/tasks")]).
#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_route("Post", attr, item)
}

// Registers an async handler for HTTP DELETE requests (e.g. #[delete("/tasks/{id}")]).
#[proc_macro_attribute]
pub fn delete(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_route("Delete", attr, item)
}