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

mod replicate;

/// Makes a type a component: `#[derive(Component)]` writes
/// `impl Component for Type {}`.
///
/// The type must be `Send + Sync + 'static`. A generic type is a component
/// for the arguments that make it one.
///
/// One option, `#[component(requires(..))]`, names the components an
/// entity with this one needs (see `Component::requires`): a type alone
/// starts at its `Default`, `Type = expression` at that value.
///
/// ```
/// # use ecs as ponko_ecs; // the dev-dependency's name here
/// use ponko_ecs::{Component, World};
///
/// #[derive(Component, Default)]
/// struct Position(f32, f32);
/// #[derive(Component)]
/// struct Health(u32);
///
/// #[derive(Component)]
/// #[component(requires(Position, Health = Health(100)))]
/// struct Enemy;
///
/// let mut world = World::new();
/// let e = world.spawn((Enemy,));
/// assert_eq!(world.get::<Health>(e).map(|h| h.0), Some(100));
/// ```
///
/// An expression runs once per entity that needs the value, as the body
/// of a closure without captures (`|| expression`). A comma in it that
/// isn't inside brackets ends it: wrap such an expression in parentheses.
#[proc_macro_derive(Component, attributes(component))]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match component(input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// Makes a struct replicated over the network: `#[derive(Replicate)]`
/// writes `ponko::net::Replicate`, visiting every field in order. Use it
/// through `ponko::net::Replicate`.
///
/// Each field's type implements `ponko::net::ReplicateField` (numbers,
/// `bool`, `Vec2`, `Vec3`, `Quat`, `NetId`, or your own), and
/// `#[replicate(..)]` says how it travels:
///
/// - nothing: exact (32 bits for a float);
/// - `min = .., max = .., step = ..`: a float (or each part of a vector)
///   rounded to `step` within the range;
/// - `angle` (with `bits = n`, 12 by default): an angle in radians,
///   wrapping;
/// - `bits = n`: an integer in `n` bits, or a rotation in `2 + 3n` bits;
/// - `skip`: not sent.
#[proc_macro_derive(Replicate, attributes(replicate))]
pub fn derive_replicate(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match replicate::replicate(input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn component(mut input: DeriveInput) -> syn::Result<TokenStream2> {
    let required = options(&input)?;
    let (trait_path, requires_path) = paths();
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
    let body = if required.is_empty() {
        quote!()
    } else {
        let adds = required.iter().map(|Required { ty, value }| match value {
            Some(value) => quote!(required.add::<#ty>(|| #value);),
            None => quote!(required.add::<#ty>(<#ty as ::core::default::Default>::default);),
        });
        quote! {
            fn requires(required: &mut #requires_path<'_>) {
                #(#adds)*
            }
        }
    };
    Ok(quote! {
        impl #impl_generics #trait_path for #name #ty_generics #where_clause {
            #body
        }
    })
}

/// One entry of `requires(..)`: a component type, and its value if given.
struct Required {
    ty: syn::Type,
    value: Option<TokenStream2>,
}

/// Reads every `#[component(..)]`: `requires(..)` is the one option.
/// Rejecting every other keeps adding options later from breaking code
/// that spelled one out.
fn options(input: &DeriveInput) -> syn::Result<Vec<Required>> {
    let mut required = Vec::new();
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("component")) {
        let syn::Meta::List(_) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "`#[component]` needs an option, as in `#[component(requires(Transform))]`",
            ));
        };
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("requires") {
                let content;
                syn::parenthesized!(content in meta.input);
                while !content.is_empty() {
                    let ty: syn::Type = content.parse()?;
                    let value = if content.peek(syn::Token![=]) {
                        content.parse::<syn::Token![=]>()?;
                        let value = until_comma(&content)?;
                        if value.is_empty() {
                            return Err(content.error("expected a value after `=`"));
                        }
                        Some(value)
                    } else {
                        None
                    };
                    required.push(Required { ty, value });
                    if content.is_empty() {
                        break;
                    }
                    content.parse::<syn::Token![,]>()?;
                }
                return Ok(());
            }
            let path = &meta.path;
            let option = quote!(#path).to_string().replace(' ', "");
            Err(syn::Error::new_spanned(
                &meta.path,
                format!("unknown component option `{option}`: `#[derive(Component)]` takes `requires(..)`"),
            ))
        })?;
    }
    Ok(required)
}

/// The tokens up to the next comma outside brackets (or the end).
fn until_comma(input: syn::parse::ParseStream) -> syn::Result<TokenStream2> {
    let mut tokens = TokenStream2::new();
    while !input.is_empty() && !input.peek(syn::Token![,]) {
        tokens.extend([input.parse::<proc_macro2::TokenTree>()?]);
    }
    Ok(tokens)
}

/// The paths of the `Component` trait and of `Requires` as the user's
/// crate sees them: through `ponko` if it depends on the engine (the
/// prelude's `Component`, its one documented path there, and
/// `ponko::ecs_core::Requires`), else `ponko_ecs::{Component, Requires}`,
/// under whatever name `Cargo.toml` gives them. Inside those crates
/// themselves the path starts with the crate's own name, which each crate
/// declares with `extern crate self as ..;`, so it also works in their
/// doctests and examples (where `crate` would name the doctest).
fn paths() -> (TokenStream2, TokenStream2) {
    for krate in ["ponko", "ponko_ecs"] {
        let name = match crate_name(krate) {
            Ok(FoundCrate::Itself) => krate.to_string(),
            Ok(FoundCrate::Name(name)) => name,
            Err(_) => continue,
        };
        let name = Ident::new(&name, Span::call_site());
        return if krate == "ponko" {
            (quote!(::#name::prelude::Component), quote!(::#name::ecs_core::Requires))
        } else {
            (quote!(::#name::Component), quote!(::#name::Requires))
        };
    }
    // Neither is a direct dependency (a crate that re-exports the engine):
    // the engine's name is the best guess, and the error names the path.
    (quote!(::ponko::prelude::Component), quote!(::ponko::ecs_core::Requires))
}
