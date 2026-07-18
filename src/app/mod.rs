use leptos::prelude::*;
use leptos_meta::{Stylesheet, Title, provide_meta_context};
use leptos_router::{
    StaticSegment, WildcardSegment,
    components::{Route, Router, Routes},
};

mod export;
mod home;
mod style;

use home::HomePage;

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/grok-build-quota.css" />
        <Title text="Grok Build 额度检测" />
        <Router>
            <main class="min-h-screen">
                <Routes fallback=move || view! { <NotFound /> }>
                    <Route path=StaticSegment("") view=HomePage />
                    <Route path=WildcardSegment("any") view=NotFound />
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    #[cfg(feature = "ssr")]
    {
        let resp = expect_context::<leptos_actix::ResponseOptions>();
        resp.set_status(actix_web::http::StatusCode::NOT_FOUND);
    }

    view! {
        <div class="grid min-h-screen place-items-center bg-[#f5f5f7] p-6 font-sans text-[#1d1d1f]">
            <div class="rounded-[24px] border border-white bg-white/62 px-10 py-14 text-center shadow-[0_24px_70px_rgba(0,0,0,0.07)] ring-1 ring-black/4 backdrop-blur-3xl">
                <div class="text-[11px] font-700 tracking-[0.14em] text-[#86868b]">"404"</div>
                <h1 class="mb-0 mt-2 text-[28px] font-700 tracking-0">"页面不存在"</h1>
            </div>
        </div>
    }
}
