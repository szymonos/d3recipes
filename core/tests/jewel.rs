// Copyright 2026 FNG. Use, modification and redistribution are permitted under the conditions in LICENSE:
// credit the source, and visibly link to the site or repository if you use its outputs in a user-facing application.
//! A primal item that can have sockets always has them all (weapons excepted). Played in the game (season 40 softcore, Barbarian): Squirt's Necklace from
//! Hope of Cain #29, Reforge, Improve Legendary (crafted primal with a socket), Reforge (natural primal with a socket).
use d3cube::data::Data;
use d3cube::sim::{chain_roots, Sim};
use std::rc::Rc;

fn data() -> Rc<Data> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/data.json");
    Rc::new(Data::from_json(&std::fs::read_to_string(p).expect("web/data.json")).unwrap())
}

/// (stem, value) of every line, a socket as ("Sockets", 0)
fn lines(sim: &Sim, d: &Data, item: usize, aff: &[usize]) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = sim.values_max(item, aff).iter().filter_map(|l| l.aff.map(|a| (d.affixes[a].stem.clone(), l.value))).collect();
    v.extend(aff.iter().filter(|&&a| d.affixes[a].specs.is_empty()).map(|&a| (d.affixes[a].stem.clone(), 0.0)));
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn want(v: &[(&str, f64)]) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = v.iter().map(|&(s, x)| (s.to_string(), x)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[test]
fn played_primal_necklace_has_a_socket() {
    let d = data();
    let item = d.items.iter().position(|i| i.name == "Squirts Necklace").unwrap();
    let slot = d.slots.iter().find(|s| s.name == "Amulet").unwrap();
    let root = chain_roots(&slot.pools[1], slot.key, 40, false, 29, true, 1, &d.items).into_iter().find(|r| r.n == 29).unwrap();
    assert_eq!(root.item, item);
    let mut sim = Sim::new(d.clone(), 1, true);
    let r1 = sim.reforge(item, root.seed);
    let (crafted, seed) = sim.primalize(item, r1.child_seed);
    // played: Str 1,000, CHD 100%, Socket, Gold 80%, Life from Health Globes 38,625 (the model adds Area Damage 20%)
    let got = lines(&sim, &d, item, &crafted);
    for w in want(&[("Gold", 0.8), ("Str", 1000.0), ("CriticalD", 1.0), ("Sockets", 0.0), ("HealthGlobeBonus", 38625.0)]) {
        assert!(got.contains(&w), "crafted primal {:?} lacks {:?}", got, w);
    }
    let r3 = sim.reforge(item, seed);
    assert!(r3.primal);
    assert_eq!(
        lines(&sim, &d, item, &r3.affixes),
        want(&[("Gold", 0.8), ("Str", 1000.0), ("CriticalD", 1.0), ("Vit", 1000.0), ("ColdResist", 210.0), ("Sockets", 0.0)])
    );
}

/// Ring of the Zodiac has no primary pick (all four primaries are fixed slots), so a primal one never has a socket.
#[test]
fn primal_zodiac_has_no_socket() {
    let d = data();
    let item = d.items.iter().position(|i| i.name == "Ring of the Zodiac").unwrap();
    let mut sim = Sim::new(d.clone(), 0, true);
    for k in 0..50u32 {
        let (aff, _) = sim.primalize(item, k.wrapping_mul(2_654_435_761));
        assert!(!aff.iter().any(|&a| d.affixes[a].socket));
    }
}
