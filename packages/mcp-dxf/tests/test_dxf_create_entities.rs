use mcp_dxf::create::{add_dxf_entities, create_dxf_entities};
use mcp_dxf::edit::{DxfEditOp, DxfSelect};
use mcp_dxf::parser::collect_pairs;
use mcp_dxf::writer::DxfNewEntity;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn file_hash(path: &Path) -> u64 {
    let bytes = fs::read(path).expect("read file for hash");
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn make_temp_workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp_dxf_task02_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn entity_type_counts(path: &Path) -> std::collections::BTreeMap<String, usize> {
    let content = fs::read_to_string(path).unwrap();
    let pairs = collect_pairs(&content).unwrap();
    let mut counts = std::collections::BTreeMap::new();
    let mut in_entities = false;
    let mut i = 0usize;
    while i < pairs.len() {
        let (c, v) = pairs[i];
        if c == 0 && v == "SECTION" {
            in_entities = pairs
                .get(i + 1)
                .is_some_and(|(c2, v2)| *c2 == 2 && *v2 == "ENTITIES");
            i += 2;
            continue;
        }
        if c == 0 && v == "ENDSEC" {
            in_entities = false;
            i += 1;
            continue;
        }
        if in_entities && c == 0 && v != "EOF" {
            *counts.entry(v.to_uppercase()).or_insert(0) += 1;
        }
        i += 1;
    }
    counts
}

#[test]
fn test_dxf_create_roundtrip_all_entity_types() {
    let ws = make_temp_workspace("create_roundtrip");
    let entities = vec![
        DxfNewEntity::Line {
            layer: "WALLS".into(),
            start: vec![0.0, 0.0, 0.0],
            end: vec![10.0, 5.0, 0.0],
            color: Some(1),
        },
        DxfNewEntity::Circle {
            layer: "WALLS".into(),
            center: vec![5.0, 5.0],
            radius: 2.5,
            color: Some(3),
        },
        DxfNewEntity::Arc {
            layer: "DOORS".into(),
            center: vec![10.0, 0.0],
            radius: 1.0,
            start_angle_deg: 0.0,
            end_angle_deg: 90.0,
            color: None,
        },
        DxfNewEntity::Lwpolyline {
            layer: "CONTOUR".into(),
            points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            closed: true,
            color: Some(5),
        },
        DxfNewEntity::Polyline3d {
            layer: "BREAKLINES".into(),
            points: vec![[0.0, 0.0, 100.5], [10.0, 0.0, 101.2], [20.0, 5.0, 102.0]],
            color: None,
        },
        DxfNewEntity::Text {
            layer: "LABELS".into(),
            insert: vec![2.0, 2.0],
            height: 0.5,
            text: "Station 33+400".into(),
            rotation_deg: Some(45.0),
            color: Some(2),
        },
        DxfNewEntity::Point {
            layer: "SURVEY".into(),
            at: vec![3.0, 4.0, 99.8],
            color: None,
        },
    ];

    let out = ws.join("outputs/from_scratch.dxf");
    let (stats, clamped) = create_dxf_entities(
        &out,
        &entities,
        None,
        &["EXTRA_EMPTY_LAYER".to_string()],
    )
    .unwrap();
    assert_eq!(stats.created, 7);
    assert!(stats.layers.contains(&"WALLS".to_string()));
    assert!(stats.layers.contains(&"EXTRA_EMPTY_LAYER".to_string()));
    assert_eq!(clamped, None);

    let counts = entity_type_counts(&out);
    assert_eq!(counts.get("LINE"), Some(&1));
    assert_eq!(counts.get("CIRCLE"), Some(&1));
    assert_eq!(counts.get("ARC"), Some(&1));
    assert_eq!(counts.get("LWPOLYLINE"), Some(&1));
    assert_eq!(counts.get("POLYLINE"), Some(&1));
    assert_eq!(counts.get("VERTEX"), Some(&3));
    assert_eq!(counts.get("SEQEND"), Some(&1));
    assert_eq!(counts.get("TEXT"), Some(&1));
    assert_eq!(counts.get("POINT"), Some(&1));
}

#[test]
fn test_dxf_add_entities_preserves_input_and_combines_edit_pipeline() {
    let ws = make_temp_workspace("add_entities");
    let seed = ws.join("outputs/seed.dxf");
    create_dxf_entities(
        &seed,
        &[DxfNewEntity::Line {
            layer: "OLD".into(),
            start: vec![0.0, 0.0],
            end: vec![5.0, 0.0],
            color: None,
        }],
        None,
        &[],
    )
    .unwrap();
    let before_hash = file_hash(&seed);

    let (stats, _) = add_dxf_entities(
        &seed,
        &ws.join("outputs/combined.dxf"),
        &[
            DxfNewEntity::Circle {
                layer: "NEW".into(),
                center: vec![10.0, 10.0],
                radius: 3.0,
                color: Some(4),
            },
            DxfNewEntity::Point {
                layer: "NEW".into(),
                at: vec![1.0, 2.0, 3.0],
                color: None,
            },
        ],
        Some(&DxfSelect {
            layer: Some("OLD".into()),
            ..Default::default()
        }),
        &[DxfEditOp::Move { dx: 10.0, dy: 0.0 }],
        None,
    )
    .unwrap();
    assert_eq!(stats.added, 2);
    assert_eq!(stats.edited_existing, 1);
    assert_eq!(stats.entity_type_counts_before.get("LINE"), Some(&1));
    assert_eq!(stats.entity_type_counts_after.get("CIRCLE"), Some(&1));
    assert_eq!(stats.entity_type_counts_after.get("POINT"), Some(&1));
    assert_eq!(before_hash, file_hash(&seed), "input file must remain untouched");

    let out = ws.join("outputs/combined.dxf");
    let counts = entity_type_counts(&out);
    assert_eq!(counts.get("LINE"), Some(&1));
    assert_eq!(counts.get("CIRCLE"), Some(&1));
    assert_eq!(counts.get("POINT"), Some(&1));
}

#[test]
fn test_rejections_and_200_entity_clamp() {
    let ws = make_temp_workspace("reject_clamp");

    let r_nan = create_dxf_entities(
        &ws.join("outputs/bad_nan.dxf"),
        &[DxfNewEntity::Point {
            layer: "0".into(),
            at: vec![f64::NAN, 1.0],
            color: None,
        }],
        None,
        &[],
    );
    assert_eq!(r_nan.unwrap_err().code, "invalid_input");

    let r_empty = create_dxf_entities(&ws.join("outputs/empty.dxf"), &[], None, &[]);
    assert_eq!(r_empty.unwrap_err().code, "invalid_input");

    let many: Vec<DxfNewEntity> = (0..250)
        .map(|i| DxfNewEntity::Point {
            layer: "PTS".into(),
            at: vec![i as f64, 0.0],
            color: None,
        })
        .collect();
    let (stats, clamped) =
        create_dxf_entities(&ws.join("outputs/clamped.dxf"), &many, None, &[]).unwrap();
    assert_eq!(stats.created, 200);
    assert_eq!(clamped, Some(200));

    let r_collision = create_dxf_entities(
        &ws.join("outputs/clamped.dxf"),
        &[DxfNewEntity::Point {
            layer: "0".into(),
            at: vec![0.0, 0.0],
            color: None,
        }],
        None,
        &[],
    );
    assert_eq!(r_collision.unwrap_err().code, "output_exists");

    let seed = ws.join("seed2.dxf");
    create_dxf_entities(
        &seed,
        &[DxfNewEntity::Point {
            layer: "0".into(),
            at: vec![0.0, 0.0],
            color: None,
        }],
        None,
        &[],
    )
    .unwrap();
    let r_missing = add_dxf_entities(
        &ws.join("missing.dxf"),
        &ws.join("outputs/trav.dxf"),
        &[DxfNewEntity::Point {
            layer: "0".into(),
            at: vec![0.0, 0.0],
            color: None,
        }],
        None,
        &[],
        None,
    );
    assert_eq!(r_missing.unwrap_err().code, "cannot_open_dxf");
}
