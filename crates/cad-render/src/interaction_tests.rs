use super::*;
#[test]
fn grab_orbit_front_surface_follows_finger_and_scales_with_viewport() {
    let initial = Camera::default();
    let front = initial.direction();
    let p = initial.projected_axis(front);
    let mut right = initial;
    right.orbit([40., 0.], [640., 640.]);
    assert!(
        right.projected_axis(front)[0] > p[0],
        "front surface must follow rightward drag"
    );
    let mut down = initial;
    down.orbit([0., 40.], [640., 640.]);
    assert!(
        down.projected_axis(front)[1] > p[1],
        "front surface must follow downward drag"
    );
    let mut scaled = initial;
    scaled.orbit([80., 0.], [1280., 1280.]);
    assert_eq!(scaled, right);
    right.orbit([-40., 0.], [640., 640.]);
    assert!((right.yaw - initial.yaw).abs() < 1e-12);
    down.orbit([0., 100000.], [640., 640.]);
    assert_eq!(down.pitch, 1.4);
}
#[test]
fn touch_hit_slop_nearest_junction_ambiguity_and_hidden_edges() {
    let mut frame = Frame {
        metadata: json!({}),
        rgb: vec![0; 64 * 64 * 3],
        depth_rgb: vec![],
        width: 64,
        height: 64,
        faces: vec![0; 64 * 64],
        edges: vec![0; 64 * 64],
        face_refs: BTreeMap::new(),
        edge_refs: BTreeMap::from([
            (1, "left".into()),
            (2, "occluded".into()),
            (3, "right".into()),
        ]),
        candidates: BTreeMap::new(),
        truncated: BTreeSet::new(),
    };
    for y in 12..53 {
        frame.edges[y * 64 + 20] = 1;
        frame.edges[y * 64 + 44] = 3;
    }
    assert!(frame.pick_edge_near([29., 32.], [6., 6.]).unwrap()["reference"].is_null());
    assert_eq!(
        frame.pick_edge_near([29., 32.], [14., 14.]).unwrap()["reference"],
        "left"
    );
    assert_eq!(
        frame.pick_edge_near([39., 32.], [14., 14.]).unwrap()["reference"],
        "right"
    );
    let junction = frame.pick_edge_near([32.5, 32.], [14., 14.]).unwrap();
    assert_eq!(junction["ambiguous"], true);
    assert!(junction["reference"].is_null());
    for y in [0., 16., 32., 63.] {
        let p = frame.pick_edge_near([63., y], [14., 14.]).unwrap();
        assert_ne!(p["reference"], "occluded");
    }
    assert!(frame.pick_edge_near([f64::NAN, 0.], [14., 14.]).is_err());
}

#[test]
fn subpixel_micro_fillet_retains_both_edge_candidates_on_cpu_and_gpu() {
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/fixtures/micro-fillet.json")).unwrap();
    let mesh: Mesh = serde_json::from_value(fixture["mesh"].clone()).unwrap();
    let edges: Vec<Edge> = serde_json::from_value(fixture["edges"].clone()).unwrap();
    let targets: Vec<String> = serde_json::from_value(fixture["micro_edges"].clone()).unwrap();
    let camera = Camera {
        yaw: -0.3,
        pitch: 0.53,
        ..Default::default()
    };
    let (projection, _) = projection(&mesh, 460, 452, camera, false).unwrap();
    let target = edges.iter().find(|e| e.reference == targets[0]).unwrap();
    let mut renderer = Renderer::new();
    for backend in ["cpu", "gpu"] {
        let frame = renderer
            .render(&mesh, &edges, 460, 452, camera, false, backend)
            .unwrap();
        for z in [2., 5., 8.] {
            let p = target.points[0];
            let point = camera.transform([p[0].get(), p[1].get(), z]);
            let xy = projection.project(point);
            let hit = frame.pick_edge_near([xy[0], xy[1]], [14., 14.]).unwrap();
            assert_eq!(hit["ambiguous"], true, "{backend}: {hit}");
            for reference in &targets {
                assert!(
                    hit["candidates"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|r| r == reference),
                    "{backend}: {reference}: {hit}"
                );
            }
        }
    }
}
