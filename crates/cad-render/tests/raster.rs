use base64::{engine::general_purpose::STANDARD, Engine};
use cad_protocol::{Edge, Mesh};
use serde_json::json;
fn geometry() -> (Mesh, Vec<Edge>) {
    let mesh=serde_json::from_value(json!({"points":[[0,0,0],[10,0,0],[0,10,0]],"triangles":[{"vertices":[0,1,2],"face":"face-1"}]})).unwrap();
    let edges = serde_json::from_value(
        json!([{"reference":"edge-1","points":[[0,0,0],[10,0,0]],"length":10}]),
    )
    .unwrap();
    (mesh, edges)
}
#[test]
fn visible_faces_edges_background_and_png_ids_agree() {
    let (mesh, edges) = geometry();
    let frame = cad_render::render(&mesh, &edges, 96, 80).unwrap();
    for target in ["edge", "face"] {
        let mode = format!("{target}-id");
        let encoded = frame.metadata["images"][&mode].as_str().unwrap();
        let bytes = STANDARD.decode(encoded).unwrap();
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut buffer = vec![0; decoder.output_buffer_size()];
        let info = decoder.next_frame(&mut buffer).unwrap();
        assert_eq!((info.width, info.height), (96, 80));
        let mut hits = 0;
        for (index, rgb) in buffer[..info.buffer_size()].chunks_exact(3).enumerate() {
            let id = rgb[0] as u32 + ((rgb[1] as u32) << 8) + ((rgb[2] as u32) << 16);
            let picked = frame
                .pick(index as u32 % 96, index as u32 / 96, target)
                .unwrap();
            if id == 0 {
                assert!(picked["reference"].is_null());
            } else {
                hits += 1;
                assert_eq!(
                    picked["reference"],
                    frame.metadata[format!("{target}_ids")][id.to_string()]
                );
            }
        }
        assert!(hits > 0);
    }
    assert!(frame.pick(96, 0, "edge").is_err());
    assert!(frame.pick(0, 0, "vertex").is_err());
}
#[test]
fn ambiguous_edges_are_never_silently_selected_and_bad_mesh_is_rejected() {
    let (mut mesh, mut edges) = geometry();
    let mut second = edges[0].clone();
    second.reference = "edge-2".into();
    edges.push(second);
    let frame = cad_render::render(&mesh, &edges, 96, 96).unwrap();
    let mut ambiguous = 0;
    for y in 0..96 {
        for x in 0..96 {
            let pick = frame.pick(x, y, "edge").unwrap();
            assert!(pick["reference"].is_null());
            if pick["ambiguous"] == true {
                ambiguous += 1
            }
        }
    }
    assert!(ambiguous > 0);
    assert!(cad_render::render(&mesh, &edges, 10000, 96).is_err());
    mesh.triangles[0].vertices[0] = 99;
    assert!(cad_render::render(&mesh, &edges, 96, 96)
        .err()
        .unwrap()
        .contains("vertex"));
}

#[test]
fn local_orbit_has_stable_scale_and_independent_pick_maps() {
    let (mesh, edges) = geometry();
    let first =
        cad_render::render_view(&mesh, &edges, 192, 160, cad_render::Camera::default()).unwrap();
    let mut camera = cad_render::Camera::default();
    camera.yaw += 0.7;
    let second = cad_render::render_view(&mesh, &edges, 192, 160, camera).unwrap();
    assert_ne!(first.rgb(), second.rgb());
    assert_eq!(
        first.metadata["camera"]["scale"],
        second.metadata["camera"]["scale"]
    );
    assert!(first.metadata["images"].as_object().unwrap().is_empty());
    camera.zoom = f64::NAN;
    assert!(cad_render::render_view(&mesh, &edges, 192, 160, camera).is_err());
}

#[test]
fn packed_planes_roundtrip_and_invalid_payloads_are_rejected() {
    let (mesh, edges) = geometry();
    let frame = cad_render::render(&mesh, &edges, 96, 96).unwrap();
    let wire = serde_json::to_value(&frame).unwrap();
    assert!(wire["rgb"].is_string() && wire["faces"].is_string());
    let restored: cad_render::Frame = serde_json::from_value(wire.clone()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.rgb(), frame.rgb());
    for (x, y) in [(0, 0), (48, 48), (60, 60)] {
        assert_eq!(
            frame.pick(x, y, "edge").unwrap(),
            restored.pick(x, y, "edge").unwrap()
        );
    }
    let mut bad = wire.clone();
    bad["rgb"] = json!("not-base64");
    assert!(serde_json::from_value::<cad_render::Frame>(bad).is_err());
    let mut bad = wire;
    bad["width"] = json!(95);
    let bad: cad_render::Frame = serde_json::from_value(bad).unwrap();
    assert!(bad.validate().is_err());
    assert!(bad.pick(0, 0, "edge").is_err());
}
