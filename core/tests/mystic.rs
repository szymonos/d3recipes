// Copyright 2026 FNG. Use, modification and redistribution are permitted under the conditions in LICENSE:
// credit the source, and visibly link to the site or repository if you use its outputs in a user-facing application.
//! "Finish at the Mystic" results only when the Mystic can really add the missing stat: a line of the same kind (from the data,
//! so Crowd Control Reduction counts as a secondary), at a hero whose class can roll it; and one it cannot finish does not end
//! the search ahead of one it can.
use d3cube::data::Data;
use d3cube::plan::{Query, Results};
use d3cube::run_query;
use std::rc::Rc;

fn data() -> Rc<Data> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/data.json");
    Rc::new(Data::from_json(&std::fs::read_to_string(p).expect("web/data.json")).unwrap())
}

fn run(v: serde_json::Value) -> Results {
    let q: Query = serde_json::from_value(v).unwrap();
    run_query(data(), q, 10_000)
}

/// A Crusader's Vigilante Belt with Str, Vit and All Res as the page asks (`end_on_near`): it used to stop at Hope of Cain #38
/// + 3 Reforges, a primal with Lightning Resistance, where the Mystic cannot roll All Resistance.
#[test]
fn an_impossible_mystic_does_not_end_the_search() {
    let r = run(serde_json::json!({
        "class": 5, "slots": ["Belt"], "items": [3768352170u32], "season": 40, "quality": "primal", "max_primalize": 2, "maxsteps": 1000,
        "cost_h": 100, "cost_r": 500, "cost_p": 2500, "cost_c": 75, "top": 4, "min_match": 3, "end_on_near": true,
        "wants": [{"fam": ["Str"]}, {"fam": ["Vit"]}, {"fam": ["ResistAll"]}]
    }));
    assert!(!r.near.is_empty());
    for h in &r.near {
        println!("near cost {} hope {} {:?} mystic {:?} as {}", h.cost, h.hope, h.route, h.mystic, h.mystic_class);
        assert!(!(h.hope == 38 && h.route == vec![('R', 3)]), "the impossible Lightning Resistance belt is back");
        assert!(!h.mystic.is_empty());
        assert_eq!(h.mystic_class, 5);
    }
}

fn necklace(switch: &[usize]) -> Results {
    run(serde_json::json!({
        "class": 6, "slots": ["Amulet"], "items": [1187653737u32], "season": 40, "quality": "primal", "max_primalize": 2, "maxsteps": 1000,
        "cost_h": 100, "cost_r": 500, "cost_p": 2500, "cost_c": 75, "top": 4, "min_match": 3, "cost_limit": 13600,
        "wants": [{"fam": ["CriticalChance"]}, {"fam": ["CriticalD"]}, {"fam": ["DamageBonusLightning"]}], "switch": switch
    }))
}

/// Squirt's Necklace with Critical Hit Chance, Critical Hit Damage and Lightning damage from a Necromancer: the primal lands CHC
/// and CHD, and the Mystic swaps the main stat (or the socket) for Lightning damage. A Necromancer never rolls Lightning damage, so
/// that needs another hero at the Mystic: with none allowed there is no such result.
#[test]
fn the_mystic_offers_what_the_enchanting_class_can_roll() {
    assert!(necklace(&[]).near.iter().all(|h| h.cost != 13500), "a Necromancer enchanting Lightning damage");
    let r = necklace(&[2]);
    let h = r.near.iter().find(|h| h.cost == 13500).expect("the 135 route");
    println!("{:?} mystic {:?} as {}", h.route, h.mystic, h.mystic_class);
    assert!(h.mystic.contains(&"Int".to_string()));
    assert_eq!(h.mystic_class, 2);
}
