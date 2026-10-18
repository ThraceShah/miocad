//! Deterministic smoke example. A real Agent can supply x/y from the shaded image.
use base64::{engine::general_purpose::STANDARD, Engine};
use cad_access::{Action, Config, Frontend};
use serde_json::{json, Value};
async fn call(
    frontend: &mut Frontend,
    command: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let response = frontend.command(command).await;
    if response["ok"] != true {
        return Err(response.to_string().into());
    }
    Ok(response["result"].clone())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = match Config::parse()? {
        Action::Run(config) => config,
        _ => return Err("use --runtime-dir DIR, --service PATH and --worker PATH".into()),
    };
    let mut frontend = Frontend::new(config);
    let doc = call(&mut frontend, json!({"op":"document.create"})).await?["document"].clone();
    call(
        &mut frontend,
        json!({"op":"model.box","document":doc,"revision":0,"size":[10,10,10]}),
    )
    .await?;
    let before = call(
        &mut frontend,
        json!({"op":"render.snapshot","document":doc,"directory":"artifacts/p4-demo"}),
    )
    .await?;
    let bytes = STANDARD.decode(before["images"]["edge-id"].as_str().ok_or("missing PNG")?)?;
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info()?;
    let mut rgb = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgb)?;
    let index = rgb[..info.buffer_size()]
        .chunks_exact(3)
        .position(|p| p != [0, 0, 0])
        .ok_or("no visible edge")?;
    let pick=call(&mut frontend,json!({"op":"render.pick","render_id":before["render_id"],"x":index as u32%info.width,"y":index as u32/info.width})).await?;
    let state=call(&mut frontend,json!({"op":"model.fillet","document":doc,"revision":1,"edge":pick["reference"],"radius":1})).await?;
    let after = call(
        &mut frontend,
        json!({"op":"render.snapshot","document":doc,"directory":"artifacts/p4-demo"}),
    )
    .await?;
    call(
        &mut frontend,
        json!({"op":"export.native","document":doc,"path":"artifacts/p4-demo/box-fillet.smb"}),
    )
    .await?;
    let report =
        json!({"document":doc,"state":state,"before":before,"selection":pick,"after":after});
    std::fs::write(
        "artifacts/p4-demo/demo.json",
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "{}",
        json!({"document":doc,"state":state,"files":after["files"],"report":"artifacts/p4-demo/demo.json","shared_service_remains_running":true})
    );
    Ok(())
}
