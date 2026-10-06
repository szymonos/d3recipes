// Copyright 2026 FNG. Use, modification and redistribution are permitted under the conditions in LICENSE:
// credit the source, and visibly link to the site or repository if you use its outputs in a user-facing application.
//! Handing the item to heroes of other classes between cube steps: the played gloves search (see plan.rs) with switching
//! allowed can only get cheaper, finds routes that use another hero, and each route replays to the tooltip it reported.
use d3cube::data::Data;
use d3cube::plan::Query;
use d3cube::run_query;
use std::rc::Rc;

fn data() -> Rc<Data> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/data.json");
    Rc::new(Data::from_json(&std::fs::read_to_string(p).expect("web/data.json")).unwrap())
}

fn query(switch: &[usize]) -> Query {
    serde_json::from_value(serde_json::json!({
        "class": 0, "slots": ["Gloves"], "quality": "primal", "max_primalize": 2, "cost_p": 25, "cost_r": 5, "cost_switch": 0,
        "wants": [{"alts": ["CriticalD"]}, {"alts": ["CriticalChance"]}, {"alts": ["CooldownReduction"]}], "min_match": 3, "top": 3,
        "switch": switch
    }))
    .unwrap()
}

#[test]
fn switching_heroes() {
    let d = data();
    let alone = run_query(d.clone(), query(&[]), 10_000);
    let mixed = run_query(d, query(&[1, 5, 6]), 10_000);
    let (a, m) = (&alone.full[0], &mixed.full[0]);
    println!("alone: cost {} {:?} {:?}", a.cost, a.route, a.route_class);
    for h in &mixed.full {
        println!("mixed: cost {} hope {} {:?} {:?} {}", h.cost, h.hope, h.route, h.route_class, h.name);
    }
    // without switching every step is the query's own hero, grouped exactly as before
    assert!(alone.full.iter().all(|h| h.route_class.iter().all(|&c| c == 0) && h.route_class.len() == h.route.len()));
    // more heroes can only add routes
    assert!(m.cost <= a.cost);
    // gloves are an item of no class, so other heroes' weights give other items: some route hands it over
    assert!(mixed.full.iter().any(|h| h.route_class.iter().any(|&c| c != 0)));
    // replaying a route with the hero of each step lands on the tooltip it reported
    for h in mixed.full.iter().chain(&alone.full) {
        let last = h.checkpoints.last().unwrap();
        let got: Vec<(&str, f64)> = last.lines.iter().map(|l| (l.stem.as_str(), l.value)).collect();
        let want: Vec<(&str, f64)> = h.lines.iter().map(|l| (l.stem.as_str(), l.value)).collect();
        assert_eq!(got, want, "route {:?} {:?}", h.route, h.route_class);
    }
}

/// Classes `class_twins` merges must roll every item identically: same affixes and the same seed after a Reforge and an
/// Improve Legendary, over many seeds. On many items of no class the three main stats split the classes into three.
#[test]
fn class_twins_roll_alike() {
    use d3cube::sim::Sim;
    let d = data();
    let mut sim = Sim::new(d.clone(), 0, true);
    let mut groups = [0usize; 8];
    let generic: Vec<usize> = (0..d.items.len()).filter(|&i| d.items[i].icls.is_none() && d.slots.iter().any(|s| s.pools.iter().flatten().any(|p| p.0 == i))).collect();
    for &it in &generic {
        let twin = sim.class_twins(it);
        groups[(0..7).filter(|&c| twin[c] == c).count()] += 1;
        for c in (0..7).filter(|&c| twin[c] != c) {
            for k in 0..100u32 {
                let seed = k.wrapping_mul(2_654_435_761).wrapping_add(12_345);
                let (mut a, mut b) = (Sim::new(d.clone(), c, true), Sim::new(d.clone(), twin[c], true));
                let (ra, rb) = (a.reforge(it, seed), b.reforge(it, seed));
                assert_eq!((ra.affixes, ra.child_seed), (rb.affixes, rb.child_seed), "{} reforge {} vs {}", d.items[it].name, c, twin[c]);
                assert_eq!(a.primalize(it, seed), b.primalize(it, seed), "{} improve {} vs {}", d.items[it].name, c, twin[c]);
            }
        }
    }
    println!("items of no class by number of class groups (1..7): {:?} of {}", &groups[1..], generic.len());
    // the main stats alone split the classes into three; no item needs fewer, and a good share needs no more
    assert_eq!(groups[1] + groups[2], 0);
    assert!(groups[3] >= generic.len() / 3);
}

/// The route played in the game (season 40 softcore): a Crusader's Vigilante Belt, Hope of Cain #2, then 12 Reforges handed
/// between the Crusader and a hero of another class six times = natural primal with Str, Vit and All Res.
#[test]
fn played_switching_route_is_found() {
    let q: Query = serde_json::from_value(serde_json::json!({
        "class": 5, "slots": ["Belt"], "items": [3768352170u32], "season": 40, "quality": "primal", "max_primalize": 2, "maxsteps": 1000,
        "cost_h": 100, "cost_r": 500, "cost_p": 2500, "cost_c": 75, "cost_switch": 100, "cost_limit": 7000, "top": 3,
        "wants": [{"fam": ["Str"]}, {"fam": ["Vit"]}, {"fam": ["ResistAll"]}], "min_match": 3, "switch": [1]
    }))
    .unwrap();
    let r = run_query(data(), q, 10_000);
    let h = &r.full[0];
    println!("cost {} hope {} {:?} {:?}", h.cost, h.hope, h.route, h.route_class);
    assert_eq!(h.cost, 6800);
    assert_eq!(h.hope, 2);
    assert_eq!(h.route, vec![('R', 1), ('R', 2), ('R', 2), ('R', 1), ('R', 1), ('R', 1), ('R', 4)]);
    assert_eq!(h.route_class, vec![5, 1, 5, 1, 5, 1, 5]);
    let str_after: Vec<f64> = h.checkpoints.iter().map(|c| c.lines.iter().find(|l| l.stem == "Str").unwrap().value).collect();
    assert_eq!(str_after, vec![452.0, 453.0, 478.0, 488.0, 596.0, 433.0, 463.0, 650.0]);
    assert_eq!(h.quality, "primal");
}

/// On an item of no class every class reaches the same seed (the class changes which affixes are picked, not how many draws
/// they take) with different lines, so the search must register each hero's roll of a state it expands only once.
#[test]
fn another_class_rolls_the_same_seed_into_other_lines() {
    use d3cube::sim::Sim;
    let d = data();
    let it = d.items.iter().position(|i| i.name == "Stone Gauntlets").unwrap();
    let (mut dh, mut barb) = (Sim::new(d.clone(), 0, true), Sim::new(d.clone(), 1, true));
    let differ = (0..100u32)
        .filter(|&k| {
            let seed = k.wrapping_mul(2_654_435_761).wrapping_add(12_345);
            let (a, b) = (dh.reforge(it, seed), barb.reforge(it, seed));
            assert_eq!(a.child_seed, b.child_seed);
            a.affixes != b.affixes
        })
        .count();
    assert!(differ > 50, "{differ} of 100");
}
