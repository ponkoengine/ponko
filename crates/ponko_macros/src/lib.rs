//! Derive macros for Ponko. Use them through `ponko` or `ponko_ecs`, which
//! re-export them next to the traits they implement:
//!
//! ```
//! # use ecs as ponko_ecs; // the dev-dependency's name here
//! use ponko_ecs::{Component, World};
//!
//! #[derive(Component)]
//! struct Health(f32);
//!
//! let mut world = World::new();
//! let e = world.spawn((Health(10.0),));
//! assert_eq!(world.get::<Health>(e).map(|h| h.0), Some(10.0));
//! ```
//!
//! With `ponko`, the derive comes in through `use ponko::prelude::*;`. The
//! guide's [ECS chapter](https://ponko.dev/guide/ecs.html) explains
//! components.

#![deny(missing_docs)]

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{DeriveInput, Ident, parse_macro_input, parse_quote};

/// Makes a type a component: `#[derive(Component)]` writes
/// `impl Component for Type {}`.
///
/// The type must be `Send + Sync + 'static`. A generic type is a component
/// for the arguments that make it one. `#[component(..)]` is reserved for
/// options; it takes none yet.
#[proc_macro_derive(Component, attributes(component))]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match component(input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn component(mut input: DeriveInput) -> syn::Result<TokenStream2> {
    reject_options(&input)?;
    let trait_path = component_trait();
    let name = &input.ident;
    // A generic type is a component where its arguments make it one, without
    // asking for bounds on each parameter.
    if !input.generics.params.is_empty() {
        input
            .generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(Self: ::core::marker::Send + ::core::marker::Sync + 'static));
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #trait_path for #name #ty_generics #where_clause {}
    })
}

/// No options exist yet; rejecting every `#[component(..)]` now keeps adding
/// options later from breaking code that spelled one out.
fn reject_options(input: &DeriveInput) -> syn::Result<()> {
    let Some(attr) = input.attrs.iter().find(|a| a.path().is_ident("component")) else {
        return Ok(());
    };
    if let syn::Meta::List(list) = &attr.meta {
        let mut unknown = None;
        // Any option is unknown; point at the first one.
        let _ = list.parse_nested_meta(|meta| {
            unknown.get_or_insert_with(|| {
                let path = &meta.path;
                let option = quote!(#path).to_string().replace(' ', "");
                syn::Error::new_spanned(
                    &meta.path,
                    format!("unknown component option `{option}`: `#[derive(Component)]` takes no options yet"),
                )
            });
            Err(meta.error("stop"))
        });
        if let Some(e) = unknown {
            return Err(e);
        }
    }
    Err(syn::Error::new_spanned(
        attr,
        "`#[component]` takes no options yet; remove the attribute",
    ))
}

/// The path of the `Component` trait as the user's crate sees it: through
/// `ponko`'s prelude (`ponko::prelude::Component`, its one documented path
/// there) if it depends on the engine, else `ponko_ecs::Component`, under
/// whatever name `Cargo.toml` gives them. Inside those crates themselves the
/// path starts with the crate's own name, which each crate declares with
/// `extern crate self as ..;`, so it also works in their doctests and
/// examples (where `crate` would name the doctest).
fn component_trait() -> TokenStream2 {
    for krate in ["ponko", "ponko_ecs"] {
        let name = match crate_name(krate) {
            Ok(FoundCrate::Itself) => krate.to_string(),
            Ok(FoundCrate::Name(name)) => name,
            Err(_) => continue,
        };
        let name = Ident::new(&name, Span::call_site());
        return if krate == "ponko" {
            quote!(::#name::prelude::Component)
        } else {
            quote!(::#name::Component)
        };
    }
    // Neither is a direct dependency (a crate that re-exports the engine):
    // the engine's name is the best guess, and the error names the path.
    quote!(::ponko::prelude::Component)
}
