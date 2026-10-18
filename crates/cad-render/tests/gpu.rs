use cad_protocol::{Edge, Mesh};
use cad_render::{Camera, Renderer};
use serde_json::json;
#[test]
fn headless_gpu_ids_occlusion_ambiguity_cache_and_explicit_fallback() {
    let mesh:Mesh=serde_json::from_value(json!({"points":[[0,0,1],[10,0,1],[0,10,1],[0,0,-1],[10,0,-1],[0,10,-1]],"triangles":[{"vertices":[0,1,2],"face":"front"},{"vertices":[3,4,5],"face":"hidden"}]})).unwrap();
    let edges:Vec<Edge>=serde_json::from_value(json!([{"reference":"front-edge","points":[[0,0,1],[10,0,1]],"length":10},{"reference":"hidden-edge","points":[[0,0,-1],[10,0,-1]],"length":10}])).unwrap();
    let camera = Camera {
        yaw: 0.,
        pitch: std::f64::consts::FRAC_PI_2,
        zoom: 1.,
        pan: [0., 0.],
    };
    let mut renderer = Renderer::new();
    let gpu=renderer.render(&mesh,&edges,192,192,camera,false,"gpu").expect("P7 GPU acceptance requires a real headless wgpu adapter; Mesa software is allowed and reported");
    assert_eq!(gpu.metadata["backend"], "wgpu");
    assert!(gpu.metadata["images"]["depth"].as_str().unwrap().len() > 10);
    let cpu = renderer
        .render(&mesh, &edges, 192, 192, camera, false, "cpu")
        .unwrap();
    let mut visible = 0;
    let mut edge_hits = 0;
    for y in 0..192 {
        for x in 0..192 {
            let face = gpu.pick(x, y, "face").unwrap();
            assert_ne!(face["reference"], "hidden");
            if face["reference"] == "front" {
                visible += 1;
            }
            let edge = gpu.pick(x, y, "edge").unwrap();
            assert_ne!(edge["reference"], "hidden-edge");
            if edge["reference"] == "front-edge" {
                edge_hits += 1;
            }
            assert_eq!(
                edge["reference"],
                cpu.pick(x, y, "edge").unwrap()["reference"]
            );
        }
    }
    assert!(visible > 100 && edge_hits > 10);
    let cached = renderer
        .render(&mesh, &edges, 192, 192, camera, false, "gpu")
        .unwrap();
    assert_eq!(cached.metadata["geometry_cache_hit"], true);
    let mut thin_mesh = mesh.clone();
    for point in &mut thin_mesh.points[3..] {
        point[2] = cad_protocol::Finite::new(0.98).unwrap();
    }
    let mut thin_edges = edges.clone();
    for point in &mut thin_edges[1].points {
        point[2] = cad_protocol::Finite::new(0.98).unwrap();
    }
    let thin = renderer
        .render(&thin_mesh, &thin_edges, 192, 192, camera, false, "gpu")
        .unwrap();
    for y in 0..192 {
        for x in 0..192 {
            let pick = thin.pick(x, y, "edge").unwrap();
            assert!(
                !pick["candidates"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r == "hidden-edge"),
                "thin rear edge leaked through front depth"
            );
        }
    }
    let mut duplicate = edges.clone();
    let mut extra = edges[0].clone();
    extra.reference = "overlap".into();
    duplicate.push(extra);
    let ambiguous = renderer
        .render(&mesh, &duplicate, 192, 192, camera, false, "gpu")
        .unwrap();
    let mut hits = 0;
    for y in 0..192 {
        for x in 0..192 {
            let p = ambiguous.pick(x, y, "edge").unwrap();
            if p["ambiguous"] == true {
                assert!(p["reference"].is_null());
                assert!(p["candidates"].as_array().unwrap().len() >= 2);
                hits += 1;
            }
        }
    }
    assert!(hits > 10);
    let mut fallback = Renderer::cpu("adapter unavailable test");
    let frame = fallback
        .render(&mesh, &edges, 192, 192, camera, false, "auto")
        .unwrap();
    assert_eq!(frame.metadata["backend"], "cpu");
    assert_eq!(
        frame.metadata["fallback_reason"],
        "adapter unavailable test"
    );
    assert!(fallback
        .render(&mesh, &edges, 192, 192, camera, false, "gpu")
        .is_err());
}
