#[cfg(not(target_arch = "wasm32"))]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = cad_web::gateway::run().await {
        eprintln!("cad-web: {error}");
        std::process::exit(1);
    }
}
#[cfg(target_arch = "wasm32")]
fn main() {}
