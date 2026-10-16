#![cfg(all(feature = "launcher", feature = "discover"))]

use topcoat::{
    Result,
    router::{Body, RouterBuilderDiscoverExt, module_router, page, request::Request, to_bytes},
    view::{View, view},
};

mod arbitrary_module {
    use super::*;

    #[topcoat::launcher]
    async fn connection_screen() -> Result<impl View> {
        Ok(view! { <p>"Connection screen"</p> })
    }
}

#[page("/")]
async fn home() -> Result<impl View> {
    Ok(view! { <h1>"Application root"</h1> })
}

#[tokio::test]
async fn launcher_does_not_claim_application_routes() {
    let app = module_router!().discover().build();
    let response = app
        .handle(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.status(), 200);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Application root"));
    for path in ["/arbitrary-module", "/__topcoat/launcher"] {
        let response = app
            .handle(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await;
        assert_eq!(response.status(), 404);
    }
}
