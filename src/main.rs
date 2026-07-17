#![recursion_limit = "512"]

#[cfg(feature = "ssr")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "ssr")]
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    use actix_files::Files;
    use actix_web::*;
    use grok_build_quota::app::*;
    use leptos::{config::get_configuration, prelude::*};
    use leptos_actix::{LeptosRoutes, generate_route_list};
    use leptos_meta::MetaTags;

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;

    let mut server = HttpServer::new(move || {
        let routes = generate_route_list(App);
        let leptos_options = &conf.leptos_options;
        let site_root = leptos_options.site_root.clone().to_string();

        App::new()
            // serve JS/WASM/CSS from `pkg`
            .service(Files::new("/pkg", format!("{site_root}/pkg")))
            // serve other assets from the `assets` directory
            .service(Files::new("/assets", &site_root))
            // serve the favicon from /favicon.ico
            .service(favicon)
            .leptos_routes(routes, {
                let leptos_options = leptos_options.clone();
                move || {
                    view! {
                        <!DOCTYPE html>
                        <html lang="zh-CN">
                            <head>
                                <meta charset="utf-8" />
                                <meta
                                    name="viewport"
                                    content="width=device-width, initial-scale=1"
                                />
                                <AutoReload options=leptos_options.clone() />
                                <HydrationScripts options=leptos_options.clone() />
                                <MetaTags />
                            </head>
                            <body>
                                <App />
                            </body>
                        </html>
                    }
                }
            })
            .app_data(web::Data::new(leptos_options.to_owned()))
            // wasm/js/css 走 br/zstd/gzip 压缩，传输体积 ~-70%
            .wrap(middleware::Compress::default())
    });
    // 0.0.0.0 / :: → 双栈；指定地址 → 单绑。IPv6 不可用时跳过该族。
    let mut bound = false;
    let mut last_err: Option<std::io::Error> = None;
    for bind_addr in dual_stack_addrs(addr) {
        match std::net::TcpListener::bind(bind_addr) {
            Ok(listener) => {
                listener.set_nonblocking(true)?;
                server = server.listen(listener)?;
                bound = true;
                println!("listening on http://{bind_addr}");
            }
            Err(err) => {
                eprintln!("bind {bind_addr} failed: {err}");
                last_err = Some(err);
            }
        }
    }
    if !bound {
        return Err(last_err.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                "no listen address bound",
            )
        }));
    }

    server.run().await
}

/// 未指定地址（全接口）时同时绑定 IPv4 + IPv6；否则只绑配置地址。
#[cfg(feature = "ssr")]
fn dual_stack_addrs(primary: std::net::SocketAddr) -> Vec<std::net::SocketAddr> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    let port = primary.port();
    match primary.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port),
        ],
        IpAddr::V6(ip) if ip.is_unspecified() => vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port),
        ],
        _ => vec![primary],
    }
}

#[cfg(feature = "ssr")]
#[actix_web::get("favicon.ico")]
async fn favicon(
    leptos_options: actix_web::web::Data<leptos::config::LeptosOptions>,
) -> actix_web::Result<actix_files::NamedFile> {
    let leptos_options = leptos_options.into_inner();
    let site_root = &leptos_options.site_root;
    Ok(actix_files::NamedFile::open(format!(
        "{site_root}/favicon.ico"
    ))?)
}

#[cfg(not(any(feature = "ssr", feature = "csr")))]
pub fn main() {
    // no client-side main function
    // unless we want this to work with e.g., Trunk for pure client-side testing
    // see lib.rs for hydration function instead
    // see optional feature `csr` instead
}

#[cfg(all(not(feature = "ssr"), feature = "csr"))]
pub fn main() {
    // a client-side main function is required for using `trunk serve`
    // prefer using `cargo leptos serve` instead
    // to run: `trunk serve --open --features csr`
    use grok_build_quota::app::*;

    console_error_panic_hook::set_once();

    leptos::mount_to_body(App);
}
