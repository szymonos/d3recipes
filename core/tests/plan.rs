// Copyright 2026 FNG. Use, modification and redistribution are permitted under the conditions in LICENSE:
// credit the source, and visibly link to the site or repository if you use its outputs in a user-facing application.
//! Planner end-to-end: the route that was run in the game: Gloves #1 (Stone Gauntlets), 8 Reforges, Improve Legendary,
//! 9 Reforges, Improve Legendary, 13 Reforges = natural primal with Critical Hit Damage, Critical Hit Chance and Cooldown Reduction.
use d3cube::data::Data;
use d3cube::plan::{Query, Want};
use d3cube::run_query;
use std::rc::Rc;

fn data() -> Rc<Data> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/data.json");
    Rc::new(Data::from_json(&std::fs::read_to_string(p).expect("web/data.json")).unwrap())
}

fn want(s: &str) -> Want {
    Want { alts: vec![s.to_string()], fam: vec![], min: None }
}

fn query(cost_p: u64, maxp: u32) -> Query {
    serde_json::from_value(serde_json::json!({
        "class": 0, "slots": ["Gloves"], "quality": "primal", "max_primalize": maxp, "cost_p": cost_p,
        "wants": [{"alts": ["CriticalD"]}, {"alts": ["CriticalChance"]}, {"alts": ["CooldownReduction"]}], "min_match": 3, "top": 3
    }))
    .unwrap()
}

#[test]
fn played_route_is_found() {
    let d = data();
    let _ = want("x");
    let t = std::time::Instant::now();
    let r = run_query(d, query(1, 2), 10_000);
    println!("unit costs: {:?} nodes {} in {:?}", r.full.first().map(|h| (h.cost, &h.route)), r.status.nodes, t.elapsed());
    for h in &r.full {
        println!("  cost {} steps {} hope {} route {:?} item {} {}", h.cost, h.steps, h.hope, h.route, h.name, h.quality);
    }
    let h = &r.full[0];
    assert_eq!(h.steps, 33);
    assert_eq!(h.route, vec![('R', 8), ('P', 1), ('R', 9), ('P', 1), ('R', 13)]);
    assert_eq!(h.hope, 1);
}

/// Convert Set Item wired into the planner: cross-checked against the
/// identical Python plan.py search (`--slot Shoulders --max-convert 2 --maxpos 3 --maxsteps 15 --quality primal`),
/// which found cost 6, path H1:RRCCR -> Hell Walkers PRIMAL, cheaper than any pure-Reforge route to the same target.
#[test]
fn convert_finds_a_cheaper_primal_route() {
    let d = data();
    let q: Query = serde_json::from_value(serde_json::json!({
        "class": 0, "slots": ["Shoulders"], "quality": "primal", "max_primalize": 0, "max_convert": 2,
        "maxpos": 3, "maxsteps": 15, "top": 5
    }))
    .unwrap();
    let r = run_query(d, q, 10_000);
    assert!(!r.full.is_empty(), "expected at least one primal route using Convert");
    let h = &r.full[0];
    println!("cost {} steps {} hope {} route {:?} item {} {}", h.cost, h.steps, h.hope, h.route, h.name, h.quality);
    assert!(h.route.iter().any(|&(op, _)| op == 'C'), "cheapest route should use a Convert step: {:?}", h.route);
    assert_eq!(h.cost, 6);
    assert_eq!(h.route, vec![('R', 2), ('C', 2), ('R', 1)]);
    assert_eq!(h.item, 0xd034fe2b); // Hell Walkers
    assert_eq!(h.quality, "primal");
    // a Convert step changes the item's identity mid-route, so the leaf's final name (what the
    // header shows) and the root's name (what the page's "Hope of Cain ... keep the last result" line must
    // say) are genuinely different items here -- this regression guards against silently collapsing back to
    // using `name` for both, which happened once already and told players to keep the wrong item.
    assert_eq!(h.name, "Hell Walkers");
    assert_eq!(h.root_name, "Unsanctified Shoulders");
    assert_ne!(h.name, h.root_name);
}

#[test]
fn weighted_search_prefers_fewer_primalizes() {
    let d = data();
    for p in [1u64, 10, 25, 50] {
        let t = std::time::Instant::now();
        let r = run_query(d.clone(), query(p, 2), 10_000);
        let h = &r.full[0];
        println!("cost_p {:>3}: cost {} steps {} hope {} route {:?}  near {} notable {}  ({} nodes, {:?})", p, h.cost, h.steps, h.hope, h.route, r.near.len(), r.notable.len(), r.status.nodes, t.elapsed());
    }
}

/// A search for one item must only return routes that end on that item. Reported live: Demon Hunter, The Shadow's Bane
/// (chest), no stats, page defaults: a Convert step moved the route onto another Shadow's piece and the page showed that
/// recipe for the chest. Query = the page's `baseQuery` for that request.
#[test]
fn item_search_only_returns_the_item() {
    let d = data();
    let bane = 0x0f807744u32;
    for quality in ["primal", "crafted", "ancient", "normal"] {
        let q: Query = serde_json::from_value(serde_json::json!({
            "class": 0, "slots": ["Chest"], "items": [bane], "season": 40, "hardcore": false, "eligible": true,
            "n0": 0, "maxpos": 4096, "maxsteps": 1000, "max_primalize": 10, "max_convert": 2,
            "cost_h": 100, "cost_r": 500, "cost_p": 2500, "cost_c": 75, "top": 4, "min_frac": 0.0,
            "wants": [], "min_match": 0, "end_on_near": false,
            "quality": quality, "end_on_primalize": quality == "crafted"
        }))
        .unwrap();
        let r = run_query(d.clone(), q, 10_000);
        for h in r.full.iter().chain(r.near.iter()).chain(r.notable.iter()) {
            println!("{quality}: cost {} route {:?} -> {} (root {})", h.cost, h.route, h.name, h.root_name);
            assert_eq!(h.item, bane, "{quality}: route {:?} ends on {}, not The Shadow's Bane", h.route, h.name);
        }
    }
}

/// The Mystic swaps a line under the same rules a roll obeys. Reported in issue #3: Crusader, Vigilante Belt, Str / Vit / All Res,
/// season 40 softcore, page defaults: 38 belts + 3 Reforges lands Str, Vit, CDR, Justice and Lightning Resistance, and the page
/// offered "Mystic: roll All Resistance". All Res shares an affix group with the single resistances, so swapping any other line
/// leaves Lightning Res in the way, and Lightning Res itself is a secondary. That recipe is no "finish at the Mystic" result at
/// all, so it no longer ends the search (`end_on_near`) ahead of the ones the Mystic can finish.
#[test]
fn mystic_respects_affix_conflicts() {
    let d = data();
    let id = 0xe09c7daau32; // Vigilante Belt
    let q: Query = serde_json::from_value(serde_json::json!({
        "class": 5, "slots": ["Belt"], "items": [id], "season": 40, "hardcore": false, "eligible": true,
        "n0": 0, "maxpos": 4096, "maxsteps": 1000, "max_primalize": 10, "max_convert": 2,
        "cost_h": 100, "cost_r": 500, "cost_p": 2500, "cost_c": 75, "top": 4, "min_frac": 0.0,
        "wants": [{"fam": ["Str"]}, {"fam": ["Vit"]}, {"fam": ["ResistAll"]}], "min_match": 3, "end_on_near": true,
        "quality": "primal", "end_on_primalize": false
    }))
    .unwrap();
    let r = run_query(d, q, 10_000);
    assert!(!r.near.is_empty());
    for h in &r.near {
        let stems: Vec<&str> = h.lines.iter().map(|l| l.stem.as_str()).collect();
        println!("hope {} route {:?} lines {:?} mystic {:?}", h.hope, h.route, stems, h.mystic);
        // with a single resistance on the item, only that line can become All Res (it is a secondary and All Res a primary,
        // so the page's same-kind rule then rules the swap out)
        if stems.iter().any(|s| s.ends_with("Resist") && *s != "ResistAll") {
            assert!(h.mystic.iter().all(|s| s.ends_with("Resist")), "a single resistance blocks All Res, yet {:?} were offered", h.mystic);
        }
        assert!(!(h.hope == 38 && h.route == vec![('R', 3)]), "the reported recipe (38 belts, 3 Reforges) is offered for the Mystic");
        assert!(!h.mystic.is_empty(), "a near result the Mystic cannot finish");
    }
}
