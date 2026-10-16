use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    FnArg, ItemFn, Pat, ReturnType,
    parse::{Parse, ParseStream},
};
use topcoat_core_grammar::paths::{
    topcoat_inventory, topcoat_launcher, topcoat_view, topcoat_view_macro,
};

use crate::common::HandlerArgs;

pub struct LauncherAttr {}

impl Parse for LauncherAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if !input.is_empty() {
            return Err(input.error("launcher does not take a route or arguments"));
        }
        Ok(Self {})
    }
}

pub struct LauncherItem {
    pub item: ItemFn,
}

impl Parse for LauncherItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let item: ItemFn = input.parse()?;
        if item.sig.asyncness.is_none() || matches!(item.sig.output, ReturnType::Default) {
            return Err(syn::Error::new_spanned(
                &item.sig,
                "launcher functions must be async and declare a return type",
            ));
        }
        if !item.sig.generics.params.is_empty() || item.sig.generics.where_clause.is_some() {
            return Err(syn::Error::new_spanned(
                &item.sig.generics,
                "launcher functions cannot be generic",
            ));
        }
        let args = HandlerArgs::parse(&item, "launcher")?;
        if args.request().is_some() {
            return Err(syn::Error::new_spanned(
                &item.sig.inputs,
                "a launcher only accepts an optional `cx: &Cx` parameter",
            ));
        }
        for arg in &item.sig.inputs {
            if let FnArg::Typed(arg) = arg
                && !matches!(&*arg.pat, Pat::Ident(pat) if pat.ident == "cx")
            {
                return Err(syn::Error::new_spanned(
                    &arg.pat,
                    "the context parameter must be named `cx`",
                ));
            }
        }
        Ok(Self { item })
    }
}

pub struct Launcher(pub LauncherAttr, pub LauncherItem);

impl Launcher {
    #[must_use]
    pub fn new(attr: LauncherAttr, item: LauncherItem) -> Self {
        Self(attr, item)
    }

    /// Parses the launcher attribute and its function.
    ///
    /// # Errors
    ///
    /// Returns an error if the attribute has arguments or the function has an
    /// unsupported signature.
    pub fn parse(attr: TokenStream, item: TokenStream) -> syn::Result<Self> {
        Ok(Self::new(syn::parse2(attr)?, syn::parse2(item)?))
    }
}

impl ToTokens for Launcher {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let item = &self.1.item;
        let ident = &item.sig.ident;
        quote! {
            #[#topcoat_view_macro::component]
            #item

            #topcoat_inventory::submit! {
                #topcoat_launcher::Registration {
                    marker: b"\0TOPCOAT_LAUNCHER_V1\0",
                    name: ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                    render: |cx, body| {
                        ::core::mem::drop(body);
                        ::std::boxed::Box::pin(#topcoat_view::HoistView::new(
                            #topcoat_view::internal::ThenView::new(async move {
                                let props = <#ident as #topcoat_view::Component>::props_builder().build();
                                <#ident as #topcoat_view::Component>::render(#ident, cx, props).await
                            })
                        ))
                    },
                }
            }
        }
        .to_tokens(tokens);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_view_with_optional_context() {
        for item in [
            quote! { async fn connect() -> Result<impl View> { todo!() } },
            quote! { pub async fn connect(cx: &Cx) -> Result<impl View> { todo!() } },
        ] {
            assert!(Launcher::parse(quote! {}, item).is_ok());
        }
    }

    #[test]
    fn rejects_routes_and_request_dependent_signatures() {
        assert!(
            Launcher::parse(
                quote! { "/" },
                quote! {
                    async fn connect() -> Result<impl View> { todo!() }
                }
            )
            .is_err()
        );
        for item in [
            quote! { fn connect() -> Result<impl View> { todo!() } },
            quote! { async fn connect() {} },
            quote! { async fn connect(body: Body) -> Result<impl View> { todo!() } },
            quote! { async fn connect(context: &Cx) -> Result<impl View> { todo!() } },
            quote! { async fn connect<T>() -> Result<impl View> { todo!() } },
        ] {
            assert!(Launcher::parse(quote! {}, item).is_err());
        }
    }
}
