// Copyright 2026 FNG. Use, modification and redistribution are permitted under the conditions in LICENSE:
// credit the source, and visibly link to the site or repository if you use its outputs in a user-facing application.
//! Route planner: best-first (Dijkstra) search over the deterministic Kanai's Cube tree.
//!
//! State = (item, seed, Improve Legendary count so far).  Moves: Reforge (always), Improve Legendary (while under the cap).  Roots = every
//! Hope of Cain result of the chosen slots (chain positions n0+1 ..= maxpos).  Cost = nH*cost_h + nR*cost_r + nP*cost_p, so the first
//! routes found are the cheapest; the search can be run in slices (`Search::run`) and stopped at any time.
use crate::data::Data;
use crate::sim::{chain_roots, Line, Sim};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Q {
    Normal = 0,
    Ancient = 1,
    Primal = 2, // natural primal (Hope of Cain / Reforge roll)
    Crafted = 3, // Improve Legendary result
}

impl Q {
    fn name(self) -> &'static str {
        match self {
            Q::Normal => "normal",
            Q::Ancient => "ancient",
            Q::Primal => "primal",
            Q::Crafted => "crafted",
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct Want {
    /// substrings of the affix label (case-insensitive); any one matches ("synonyms")
    #[serde(default)]
    pub alts: Vec<String>,
    /// exact stat families (see `data::stem_of`, case-insensitive); any one matches
    #[serde(default)]
    pub fam: Vec<String>,
    /// optional minimum value of the matching line
    #[serde(default)]
    pub min: Option<f64>,
}

fn d_true() -> bool {
    true
}
fn d_season() -> u32 {
    40
}
fn d_maxpos() -> u32 {
    4096
}
fn d_maxsteps() -> u32 {
    40
}
fn d_p() -> u32 {
    2
}
fn d_one() -> u64 {
    1
}
fn d_pcost() -> u64 {
    25
}
fn d_top() -> usize {
    8
}
fn d_quality() -> String {
    "any".into()
}

#[derive(Deserialize, Clone)]
pub struct Query {
    pub class: usize,
    pub slots: Vec<String>,
    #[serde(default = "d_season")]
    pub season: u32,
    #[serde(default)]
    pub hardcore: bool,
    #[serde(default = "d_true")]
    pub eligible: bool,
    #[serde(default)]
    pub n0: u32,
    #[serde(default = "d_maxpos")]
    pub maxpos: u32,
    #[serde(default = "d_maxsteps")]
    pub maxsteps: u32,
    #[serde(default = "d_p")]
    pub max_primalize: u32,
    /// Convert Set Item steps allowed per route; 0 = off. Unavailable on a source whose set has
    /// <= 2 members (checked at expand time via Sim::set_pool).
    #[serde(default)]
    pub max_convert: u32,
    #[serde(default = "d_one")]
    pub cost_h: u64,
    #[serde(default = "d_one")]
    pub cost_r: u64,
    #[serde(default = "d_pcost")]
    pub cost_p: u64,
    #[serde(default = "d_one")]
    pub cost_c: u64,
    /// "any" | "normal" | "ancient" | "primal" (natural) | "crafted" | "anyprimal"
    #[serde(default = "d_quality")]
    pub quality: String,
    #[serde(default)]
    pub wants: Vec<Want>,
    /// how many of `wants` a result must show (0 = all of them)
    #[serde(default)]
    pub min_match: usize,
    /// only roots of these item ids; when given, a result must also END on one of them
    #[serde(default)]
    pub items: Vec<u32>,
    /// with `items` and Convert allowed: also start from every other piece of their sets (sets of more than 2 pieces), in every
    /// slot those pieces come from, so a route can craft a cheap piece and Convert into the one asked for
    #[serde(default)]
    pub set_roots: bool,
    /// nodes kept before the search gives up (0 = 3,000,000); bounds memory however long the caller lets it run
    #[serde(default)]
    pub node_cap: usize,
    /// also return every step of each route (`Hit::trail`), not only the grouped checkpoints
    #[serde(default)]
    pub trail: bool,
    /// a full result must leave the Mystic able to finish it: the stat in `mystic` (any of these stems) when the item lacks it, or the
    /// one wanted stat it lacks when `min_match` allows one short, must legally replace a line of the same kind that is not wanted
    #[serde(default)]
    pub mystic_finish: bool,
    #[serde(default)]
    pub mystic: Vec<String>,
    /// stems the Mystic must never replace, besides the wanted ones (e.g. stats the item always rolls)
    #[serde(default)]
    pub keep: Vec<String>,
    #[serde(default)]
    pub end_on_primalize: bool,
    /// stop at the first route that lands every wanted stat OR all but one (the Mystic finishes it): the search is cheapest-first, so
    /// nothing found later is cheaper than that route
    #[serde(default)]
    pub end_on_near: bool,
    /// other hero classes that may do the cube steps after the Hope of Cain root. The item keeps its seed from hero to
    /// hero; the hero's class only changes the affix weights (on items of no class) and costs a Reforge one extra draw
    /// on another class's item, so a step can be handed to whichever hero rolls what is wanted
    #[serde(default)]
    pub switch: Vec<usize>,
    /// cost of handing the item to a hero of another class (0 = free); a small one keeps routes from switching for nothing
    #[serde(default)]
    pub cost_switch: u64,
    #[serde(default = "d_top")]
    pub top: usize,
    /// stop when the next node would cost more than this (0 = unlimited)
    #[serde(default)]
    pub cost_limit: u64,
    /// a wanted stat only counts when its rolled value is at least this fraction of that line's maximum roll
    /// (0 = any roll; natural and crafted primals are always at their maximum, so this only bites ancients and legendaries)
    #[serde(default)]
    pub min_frac: f64,
}

#[derive(Serialize, Clone)]
pub struct LineOut {
    pub label: String,
    pub stem: String,
    pub value: f64,
    /// the maximum roll of this line (equal to `value` on a primal)
    pub max: f64,
}

/// The item as it stands at a point along a route (right after the Hope of Cain root, and after each grouped step),
/// so the page can tell the player what to look for before they carry on.
#[derive(Serialize, Clone)]
pub struct Checkpoint {
    pub name: String,
    pub quality: String,
    pub lines: Vec<LineOut>,
}

#[derive(Serialize, Clone)]
pub struct Hit {
    pub cost: u64,
    pub steps: u32,
    pub slot: String,
    /// Hope of Cain transmutes needed (chain position minus n0)
    pub hope: u32,
    /// grouped route below the root: [["R", 8], ["P", 1], ...]
    pub route: Vec<(char, u32)>,
    /// the hero class doing each group of `route` (all the query's class unless `switch` allowed others)
    pub route_class: Vec<usize>,
    pub item: u32,
    pub name: String,
    /// the item Hope of Cain itself lands on — usually equal to `name`, but can differ once a Convert Set Item
    /// step ('C' in `route`) changes identity partway through the chain. The page's "keep the
    /// last result" line must say this, not the final `name`, or it tells the player to keep an item their
    /// Hope of Cain draw was never going to give them.
    pub root_name: String,
    pub quality: String,
    pub seed: u32,
    /// indices of the wants that appear on the tooltip
    pub matched: Vec<usize>,
    pub lines: Vec<LineOut>,
    /// on a recipe one wanted stat short: the stems of the spare lines the Mystic may swap for that stat (the roll rules: affix
    /// groups, exclusion keys, budget; the same kind, from the data); never empty on a `near` hit asking for stat families
    pub mystic: Vec<String>,
    /// the class of the hero who enchants at the Mystic: the query's own when its class can roll the stat, else one from `switch`
    pub mystic_class: usize,
    /// every step of the route, root first (only with `Query::trail`)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub trail: Vec<Checkpoint>,
    /// [root, after group 1, after group 2, ...] (filled in `results`, only for the routes actually returned)
    pub checkpoints: Vec<Checkpoint>,
    #[serde(skip)]
    idx: u32,
}

#[derive(Serialize, Default)]
pub struct Status {
    pub nodes: u64,
    pub queue: usize,
    pub cost_reached: u64,
    pub done: bool,
    /// the search stopped at NODE_CAP without finishing (as opposed to running out of candidates)
    pub capped: bool,
}

#[derive(Serialize)]
pub struct Results {
    pub status: Status,
    /// routes that satisfy the target (quality + at least `min_match` wants), cheapest first
    pub full: Vec<Hit>,
    /// routes one want short (use the Mystic for the missing one), cheapest first
    pub near: Vec<Hit>,
    /// natural primals and ancients met on the way that show at least `min_match - 1` wants
    pub notable: Vec<Hit>,
    pub warnings: Vec<String>,
}

struct NodeRec {
    item: u32,
    seed: u32,
    q: Q,
    pc: u8,
    cc: u8, // Convert steps used so far
    depth: u16,
    parent: u32,
    op: u8, // b'H' root, b'R', b'P', b'C' (Convert)
    cls: u8, // the hero class that did this step (the root: the query's class)
    slot: u16,
    n: u16,
}

fn clean(label: &str) -> String {
    label.replace("1xx_", "").replace("21x_", "").replace("Skill_DemonHunter_", "")
}

/// One slot's Hope of Cain sequence, materialised lazily: positions 1..=`upto` are already roots.
struct Chain {
    slot: usize, // index into Data::slots
    name_idx: u16,
    upto: u32,
}

/// Weapon damage lines: never worth rolling off at the Mystic (the page's RANGE_STEMS).
const RANGE_STEMS: [&str; 8] = ["MinMaxDam", "ArcaneD", "ColdD", "FireD", "HolyD", "LightningD", "PoisonD", "PhysicalD"];

/// Positions added to a chain at a time.
const CHAIN_CHUNK: u32 = 32;

/// Nodes kept before the search gives up, which bounds memory (a few hundred MB at most) however long the caller lets it run.
const NODE_CAP: usize = 3_000_000;

pub struct Search {
    d: Rc<Data>,
    sim: Sim,
    q: Query,
    /// classes that may do a step: the query's own first, then `switch`
    heroes: Vec<usize>,
    quality: String,
    wants_lc: Vec<(Vec<String>, Vec<String>, Option<f64>)>,
    min_match: usize,
    nodes: Vec<NodeRec>,
    heap: BinaryHeap<Reverse<(u64, u64, u32)>>,
    seen: HashSet<(u32, u32, u8, u8)>,
    seq: u64,
    full: Vec<(u64, Hit)>,
    near: Vec<(u64, Hit)>,
    notable: Vec<(u64, Hit)>,
    slot_names: Vec<String>,
    /// item ids a route may start from (empty = any): `q.items`, plus their set-mates with `q.set_roots`
    root_items: Vec<u32>,
    chains: Vec<Chain>,
    root_x0: HashMap<u32, u64>,
    warnings: Vec<String>,
    processed: u64,
    cost_reached: u64,
    done: bool,
    capped: bool,
}

impl Search {
    pub fn new(d: Rc<Data>, q: Query) -> Search {
        let sim = Sim::new(d.clone(), q.class, q.eligible);
        let wants_lc: Vec<(Vec<String>, Vec<String>, Option<f64>)> = q
            .wants
            .iter()
            .map(|w| (w.alts.iter().map(|a| a.to_lowercase()).collect(), w.fam.iter().map(|a| a.to_lowercase()).collect(), w.min))
            .collect();
        let min_match = if q.min_match == 0 { wants_lc.len() } else { q.min_match.min(wants_lc.len()) };
        let mut heroes = vec![q.class];
        for &c in &q.switch {
            if c < 7 && !heroes.contains(&c) {
                heroes.push(c);
            }
        }
        let mut s = Search {
            heroes,
            quality: q.quality.clone(),
            d,
            sim,
            wants_lc,
            min_match,
            q,
            nodes: Vec::new(),
            heap: BinaryHeap::new(),
            seen: HashSet::new(),
            seq: 0,
            full: Vec::new(),
            near: Vec::new(),
            notable: Vec::new(),
            slot_names: Vec::new(),
            root_items: Vec::new(),
            chains: Vec::new(),
            root_x0: HashMap::new(),
            warnings: Vec::new(),
            processed: 0,
            cost_reached: 0,
            done: false,
            capped: false,
        };
        s.init_roots();
        s
    }

    fn init_roots(&mut self) {
        let d = self.d.clone();
        self.root_items = self.q.items.clone();
        let mut slots = self.q.slots.clone();
        if self.q.set_roots && self.q.max_convert > 0 {
            for &id in self.q.items.iter() {
                let Some(ii) = d.items.iter().position(|it| it.id == id) else { continue };
                let mates = self.sim.set_pool(ii);
                if mates.len() <= 2 {
                    continue; // Convert Set Item needs a set of more than 2 pieces
                }
                for &m in mates.iter() {
                    if !self.root_items.contains(&d.items[m].id) {
                        self.root_items.push(d.items[m].id);
                    }
                }
                for s in d.slots.iter() {
                    if !slots.contains(&s.name) && s.pools[self.q.class].iter().any(|&(i, _)| mates.contains(&i)) {
                        slots.push(s.name.clone());
                    }
                }
            }
        }
        for name in slots.iter() {
            let si = match d.slots.iter().position(|s| &s.name == name) {
                Some(i) => i,
                None => {
                    self.warnings.push(format!("unknown slot {}", name));
                    continue;
                }
            };
            if d.slots[si].pools[self.q.class].is_empty() {
                self.warnings.push(format!("{} has no items for {}", name, d.classes[self.q.class]));
                continue;
            }
            self.slot_names.push(name.clone());
            self.chains.push(Chain { slot: si, name_idx: (self.slot_names.len() - 1) as u16, upto: 0 });
        }
        self.feed_chains();
    }

    /// Cost of the next Hope of Cain position of a chain that still has room (`maxpos` is only a safety ceiling).
    fn next_chain_cost(&self, c: &Chain) -> Option<u64> {
        if c.upto >= self.q.maxpos {
            return None;
        }
        Some((c.upto + 1).saturating_sub(self.q.n0) as u64 * self.q.cost_h)
    }

    /// Add chain positions as the search reaches their cost, so no cap on the chain is needed up front.
    fn feed_chains(&mut self) {
        loop {
            let top = self.heap.peek().map_or(u64::MAX, |Reverse((c, _, _))| *c);
            let pick = (0..self.chains.len())
                .filter_map(|i| self.next_chain_cost(&self.chains[i]).map(|c| (c, i)))
                .filter(|(c, _)| *c <= top)
                .min();
            match pick {
                Some((_, i)) => self.extend_chain(i),
                None => break,
            }
        }
    }

    fn extend_chain(&mut self, ci: usize) {
        let d = self.d.clone();
        let (si, name_idx, done) = (self.chains[ci].slot, self.chains[ci].name_idx, self.chains[ci].upto);
        let to = (done + CHAIN_CHUNK).min(self.q.maxpos);
        self.chains[ci].upto = to;
        let slot = &d.slots[si];
        let pool = &slot.pools[self.q.class];
        let roots = chain_roots(pool, slot.key, self.q.season, self.q.hardcore, to, self.q.eligible, self.q.class, &d.items);
        for r in roots {
            if r.n <= done || r.n <= self.q.n0 {
                continue;
            }
            let it = &d.items[r.item];
            if !self.root_items.is_empty() && !self.root_items.contains(&it.id) {
                continue;
            }
            let q = if r.primal { Q::Primal } else if r.ancient { Q::Ancient } else { Q::Normal };
            let cost = (r.n - self.q.n0) as u64 * self.q.cost_h;
            let idx = self.nodes.len() as u32;
            self.nodes.push(NodeRec { item: r.item as u32, seed: r.seed, q, pc: 0, cc: 0, depth: 0, parent: u32::MAX, op: b'H', cls: self.q.class as u8, slot: name_idx, n: r.n as u16 });
            self.root_x0.insert(idx, r.x0);
            if self.need_lines(q) {
                let aff = self.sim.drop_item(r.item, r.x0, r.ancient || r.primal, r.primal);
                let lines = if r.primal { self.sim.values_max(r.item, &aff) } else { self.sim.values(r.item, r.seed, &aff) };
                self.register(idx, cost, q, r.item, &aff, lines);
            }
            if self.seen.insert((r.item as u32, r.seed, 0, 0)) {
                self.push(cost, idx);
            }
        }
    }

    /// Duplicate check: a count with no real limit (255 or more) is not part of the state, so an item reached again by another
    /// route is not searched twice (the first time is the cheapest: the search is cost-ordered).
    fn key(&self, item: u32, seed: u32, pc: u8, cc: u8) -> (u32, u32, u8, u8) {
        (item, seed, if self.q.max_primalize >= 255 { 0 } else { pc }, if self.q.max_convert >= 255 { 0 } else { cc })
    }

    fn push(&mut self, cost: u64, idx: u32) {
        self.seq += 1;
        self.heap.push(Reverse((cost, self.seq, idx)));
    }

    /// does a node of this quality need its tooltip computed (as a target candidate or a notable find)?
    fn need_lines(&self, q: Q) -> bool {
        match q {
            Q::Primal | Q::Ancient => true,
            Q::Normal => matches!(self.quality.as_str(), "any" | "normal"),
            Q::Crafted => self.q.end_on_primalize && matches!(self.quality.as_str(), "any" | "crafted" | "anyprimal"),
        }
    }

    fn quality_ok(&self, q: Q) -> bool {
        match self.quality.as_str() {
            "any" => true,
            "normal" => q == Q::Normal,
            "ancient" => q == Q::Ancient,
            "ancient+" => q == Q::Ancient || q == Q::Primal,
            "primal" => q == Q::Primal,
            "crafted" => q == Q::Crafted,
            "anyprimal" => q == Q::Primal || q == Q::Crafted,
            _ => true,
        }
    }

    fn make_lines(&self, aff: &[usize], raw: Vec<Line>, mx: Option<Vec<Line>>) -> Vec<LineOut> {
        let mut out = Vec::with_capacity(raw.len() + 2);
        for (i, l) in raw.into_iter().enumerate() {
            let (label, stem) = match l.aff {
                Some(a) => (clean(&self.d.affixes[a].label), self.d.affixes[a].stem.clone()),
                None => ("item power".to_string(), "item power".to_string()),
            };
            let max = mx.as_ref().and_then(|m| m.get(i)).map_or(l.value, |m| m.value);
            out.push(LineOut { label, stem, value: l.value, max });
        }
        for &a in aff {
            if self.d.affixes[a].specs.is_empty() {
                out.push(LineOut { label: clean(&self.d.affixes[a].label), stem: self.d.affixes[a].stem.clone(), value: 0.0, max: 0.0 });
            }
        }
        out
    }

    /// `Query::mystic_finish`: the stats a result still lacks (the Mystic stat, and a wanted stat when `min_match` allows one short) must
    /// be at most one, and the Mystic must be able to add it: a line that is not wanted, not kept, not the weapon damage range and of the
    /// same kind can legally be replaced by it.
    fn mystic_can_finish(&mut self, item: usize, aff: &[usize], lines: &[LineOut], matched: &[usize]) -> bool {
        let mut missing: Vec<Vec<String>> =
            (0..self.wants_lc.len()).filter(|w| !matched.contains(w)).map(|w| self.wants_lc[w].1.clone()).collect();
        let lc = |v: &[String]| v.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
        let mystic = lc(&self.q.mystic);
        if !mystic.is_empty() && !lines.iter().any(|l| mystic.contains(&l.stem.to_lowercase())) {
            missing.push(mystic);
        }
        match missing.len() {
            0 => return true,
            1 => {}
            _ => return false,
        }
        let keep = lc(&self.q.keep);
        let spare = |stem: &str| {
            let s = stem.to_lowercase();
            !self.wants_lc.iter().any(|w| w.1.contains(&s)) && !keep.contains(&s) && !RANGE_STEMS.iter().any(|r| r.to_lowercase() == s) && s != "indestructible"
        };
        let ok: Vec<bool> = aff.iter().map(|&a| spare(&self.d.affixes[a].stem)).collect();
        self.sim.mystic_swaps(item, aff, &missing[0], true, self.q.class).into_iter().any(|p| ok[p])
    }

    fn matched(&self, lines: &[LineOut]) -> Vec<usize> {
        let mut m = Vec::new();
        for (wi, (alts, fam, min)) in self.wants_lc.iter().enumerate() {
            let hit = lines.iter().any(|l| {
                let lab = l.label.to_lowercase();
                (alts.iter().any(|a| lab.contains(a.as_str())) || (!fam.is_empty() && fam.iter().any(|f| l.stem.to_lowercase() == *f)))
                    && min.map_or(true, |t| l.value >= t)
                    && (self.q.min_frac <= 0.0 || l.max <= 0.0 || l.value >= self.q.min_frac * l.max - 1e-9)
            });
            if hit {
                m.push(wi);
            }
        }
        m
    }

    fn route_of(&self, mut idx: u32) -> (Vec<(char, u32)>, Vec<usize>, u16, u16, usize) {
        let mut ops: Vec<(u8, u8)> = Vec::new();
        loop {
            let n = &self.nodes[idx as usize];
            if n.parent == u32::MAX {
                ops.reverse();
                let mut route: Vec<(char, u32)> = Vec::new();
                let mut who: Vec<usize> = Vec::new();
                for (o, cls) in ops {
                    let c = o as char;
                    match route.last_mut() {
                        Some(l) if l.0 == c && who.last() == Some(&(cls as usize)) => l.1 += 1,
                        _ => {
                            route.push((c, 1));
                            who.push(cls as usize);
                        }
                    }
                }
                return (route, who, n.slot, n.n, n.item as usize);
            }
            ops.push((n.op, n.cls));
            idx = n.parent;
        }
    }

    /// record a tooltip if it satisfies the target, is one want short, or is a notable natural primal / ancient
    fn register(&mut self, idx: u32, cost: u64, q: Q, item: usize, aff: &[usize], raw: Vec<Line>) {
        if q == Q::Crafted && !self.q.end_on_primalize {
            return;
        }
        // a search for given items only answers with those items: a Convert step can move the route onto a set-mate
        if !self.q.items.is_empty() && !self.q.items.contains(&self.d.items[item].id) {
            return;
        }
        let mx =if matches!(q, Q::Primal | Q::Crafted) { None } else { Some(self.sim.values_max(item, aff)) };
        let lines = self.make_lines(aff, raw, mx);
        let matched = self.matched(&lines);
        let qual_ok = self.quality_ok(q);
        let nw = self.wants_lc.len();
        let is_full = qual_ok && matched.len() >= self.min_match && (!self.q.mystic_finish || self.mystic_can_finish(item, aff, &lines, &matched));
        let mut is_near = qual_ok && nw > 0 && matched.len() + 1 == self.min_match;
        // a recipe one stat short: the spare lines (not wanted, not the weapon damage range) the Mystic may legally swap for the
        // missing stat, of the same kind, at the first hero whose class can roll it (the query's own, then `switch`). With none,
        // it is no recipe one short at all, so it cannot end the search (`end_on_near`) ahead of one the Mystic can finish.
        let (mut mystic, mut mystic_class) = (Vec::new(), self.q.class);
        if let Some(w) = (0..nw).find(|w| is_near && !matched.contains(w) && !self.wants_lc[*w].1.is_empty()) {
            let fams = self.wants_lc[w].1.clone();
            let spare = |stem: &String| {
                let s = stem.to_lowercase();
                !self.wants_lc.iter().any(|w| w.1.contains(&s)) && !RANGE_STEMS.iter().any(|r| r.to_lowercase() == s) && s != "indestructible"
            };
            for c in self.heroes.clone() {
                let lines: Vec<String> =
                    self.sim.mystic_swaps(item, aff, &fams, true, c).into_iter().map(|p| self.d.affixes[aff[p]].stem.clone()).filter(|s| spare(s)).collect();
                if !lines.is_empty() {
                    (mystic, mystic_class) = (lines, c);
                    break;
                }
            }
            is_near = !mystic.is_empty();
        }
        let notable_min = self.min_match.saturating_sub(1).max(1);
        let is_notable = !is_full && !is_near && (q == Q::Primal || q == Q::Ancient) && nw > 0 && matched.len() >= notable_min;
        if !(is_full || is_near || is_notable) {
            return;
        }
        let (route, route_class, slot, n, root_item) = self.route_of(idx);
        let node = &self.nodes[idx as usize];
        let hit = Hit {
            cost,
            steps: (n as u32 - self.q.n0) + route.iter().map(|r| r.1).sum::<u32>(),
            slot: self.slot_names[slot as usize].clone(),
            hope: n as u32 - self.q.n0,
            route,
            route_class,
            item: self.d.items[node.item as usize].id,
            name: self.d.items[node.item as usize].name.clone(),
            root_name: self.d.items[root_item].name.clone(),
            quality: q.name().to_string(),
            seed: node.seed,
            matched,
            lines,
            mystic,
            mystic_class,
            trail: Vec::new(),
            checkpoints: Vec::new(),
            idx,
        };
        let cap = self.q.top.max(1) * 4;
        let list = if is_full {
            &mut self.full
        } else if is_near {
            &mut self.near
        } else {
            &mut self.notable
        };
        list.push((cost, hit));
        if list.len() > cap * 2 {
            list.sort_by_key(|h| (h.0, h.1.steps));
            list.truncate(cap);
        }
    }

    fn kth_full_cost(&self) -> Option<u64> {
        let top = self.q.top.max(1);
        if self.full.len() < top {
            return None;
        }
        let mut c: Vec<u64> = self.full.iter().map(|h| h.0).collect();
        c.sort_unstable();
        Some(c[top - 1])
    }

    fn expand(&mut self, idx: u32, cost: u64) {
        let (item, seed, pc, cc, depth, q) = {
            let n = &self.nodes[idx as usize];
            (n.item as usize, n.seed, n.pc, n.cc, n.depth, n.q)
        };
        let _ = q;
        if depth as u32 >= self.q.maxsteps {
            return;
        }
        let (slot, n0, cur) = {
            let n = &self.nodes[idx as usize];
            (n.slot, n.n, n.cls as usize)
        };
        // The heroes that may do the next step, the one holding the item first: a hero whose step gives the same item
        // as an earlier one is skipped, so staying put wins every tie. A class item rolls with its own class's weights
        // whoever transmutes it, and a hero of another class only adds one draw to a Reforge; an item of no class rolls
        // with the hero's weights.
        let icls = self.d.items[item].icls;
        let order: Vec<usize> = std::iter::once(cur).chain(self.heroes.iter().copied().filter(|&c| c != cur)).collect();
        let distinct = |key: &dyn Fn(usize) -> (usize, bool)| -> Vec<usize> {
            let mut keys = Vec::new();
            order.iter().copied().filter(|&c| !keys.contains(&key(c)) && { keys.push(key(c)); true }).collect()
        };
        // and classes that give every affix of the item the same weight roll it the same (Sim::class_twins)
        let twin = self.sim.class_twins(item);
        let reforgers = distinct(&|c| (icls.unwrap_or(twin[c]), icls.map_or(false, |ic| ic != c)));
        let improvers = distinct(&|c| (icls.unwrap_or(twin[c]), false));
        let cs = self.q.cost_switch;
        let hand = move |c: usize| if c == cur { 0 } else { cs };
        // Reforge
        for c in reforgers {
            self.sim.hero = c;
            let g = self.sim.reforge(item, seed);
            let cq = if g.primal { Q::Primal } else if g.ancient { Q::Ancient } else { Q::Normal };
            let ccost = cost + self.q.cost_r.max(1) + hand(c);
            if self.seen.insert(self.key(item as u32, g.child_seed, pc, cc)) {
                let cidx = self.nodes.len() as u32;
                self.nodes.push(NodeRec { item: item as u32, seed: g.child_seed, q: cq, pc, cc, depth: depth + 1, parent: idx, op: b'R', cls: c as u8, slot, n: n0 });
                if self.need_lines(cq) {
                    let raw = if g.primal { self.sim.values_max(item, &g.affixes) } else { self.sim.values(item, g.child_seed, &g.affixes) };
                    self.register(cidx, ccost, cq, item, &g.affixes, raw);
                }
                self.push(ccost, cidx);
            }
        }
        // Improve Legendary
        if (pc as u32) < self.q.max_primalize {
            for c in improvers {
                self.sim.hero = c;
                let (aff, child) = self.sim.primalize(item, seed);
                let pcost = cost + self.q.cost_p.max(1) + hand(c);
                if self.seen.insert(self.key(item as u32, child, pc + 1, cc)) {
                    let cidx = self.nodes.len() as u32;
                    self.nodes.push(NodeRec { item: item as u32, seed: child, q: Q::Crafted, pc: pc + 1, cc, depth: depth + 1, parent: idx, op: b'P', cls: c as u8, slot, n: n0 });
                    if self.need_lines(Q::Crafted) {
                        let raw = self.sim.values_max(item, &aff);
                        self.register(cidx, pcost, Q::Crafted, item, &aff, raw);
                    }
                    self.push(pcost, cidx);
                }
            }
        }
        // Convert Set Item: a DIFFERENT item id, always non-Ancient, unavailable on <= 2-piece sets. The target is picked
        // with the hero's weights, so every hero is tried; one landing on the same item and seed is dropped by `seen`.
        if (cc as u32) < self.q.max_convert && self.sim.set_pool(item).len() > 2 {
            for c in order.clone() {
                self.sim.hero = c;
                let g = self.sim.convert(item, seed);
                let vcost = cost + self.q.cost_c.max(1) + hand(c);
                if self.seen.insert(self.key(g.target as u32, g.child_seed, pc, cc + 1)) {
                    let cidx = self.nodes.len() as u32;
                    self.nodes.push(NodeRec { item: g.target as u32, seed: g.child_seed, q: Q::Normal, pc, cc: cc + 1, depth: depth + 1, parent: idx, op: b'C', cls: c as u8, slot, n: n0 });
                    if self.need_lines(Q::Normal) {
                        let raw = self.sim.values(g.target, g.child_seed, &g.affixes);
                        self.register(cidx, vcost, Q::Normal, g.target, &g.affixes, raw);
                    }
                    self.push(vcost, cidx);
                }
            }
        }
        // Hope of Cain roots are always rolled by the query's own hero
        self.sim.hero = self.q.class;
    }

    /// Process up to `max_nodes` queue entries; returns true when the search is finished (target found with top results, limit reached, or exhausted).
    pub fn run(&mut self, max_nodes: u32) -> bool {
        if self.done {
            return true;
        }
        let mut left = max_nodes;
        loop {
            self.feed_chains();
            if self.nodes.len() >= if self.q.node_cap == 0 { NODE_CAP } else { self.q.node_cap } {
                self.done = true;
                self.capped = true;
                self.heap.clear();
                return true;
            }
            let Some(Reverse((cost, _, idx))) = self.heap.pop() else { break };
            self.cost_reached = cost;
            let past_limit = self.q.cost_limit > 0 && cost > self.q.cost_limit;
            let enough = self.kth_full_cost().map_or(false, |k| cost >= k)
                || (self.q.end_on_near && self.near.iter().map(|h| h.0).min().map_or(false, |k| cost >= k));
            if past_limit || enough {
                self.done = true;
                self.heap.clear();
                return true;
            }
            self.expand(idx, cost);
            self.processed += 1;
            left -= 1;
            if left == 0 {
                return false;
            }
        }
        self.done = true;
        true
    }

    /// The item's tooltip at the root and after each grouped route step.
    fn group(states: &[Checkpoint], route: &[(char, u32)]) -> Vec<Checkpoint> {
        let mut out = vec![states[0].clone()];
        let mut at = 0usize;
        for &(_, n) in route {
            at += n as usize;
            out.push(states[at.min(states.len() - 1)].clone());
        }
        out
    }

    /// The item's tooltip at every step of the route to `idx`, root first (replays the route from its Hope of Cain root).
    fn states(&mut self, idx: u32) -> Vec<Checkpoint> {
        let mut path = Vec::new();
        let mut i = idx;
        loop {
            path.push(i);
            let p = self.nodes[i as usize].parent;
            if p == u32::MAX {
                break;
            }
            i = p;
        }
        path.reverse();
        let mut states: Vec<Checkpoint> = Vec::with_capacity(path.len());
        for k in 0..path.len() {
            let (item, seed, q, op, cls) = {
                let n = &self.nodes[path[k] as usize];
                (n.item as usize, n.seed, n.q, n.op, n.cls as usize)
            };
            self.sim.hero = cls;
            let (aff, raw) = if k == 0 {
                let x0 = self.root_x0[&path[k]];
                let aff = self.sim.drop_item(item, x0, q != Q::Normal, q == Q::Primal);
                let raw = if q == Q::Primal { self.sim.values_max(item, &aff) } else { self.sim.values(item, seed, &aff) };
                (aff, raw)
            } else {
                let (pitem, pseed) = {
                    let n = &self.nodes[path[k - 1] as usize];
                    (n.item as usize, n.seed)
                };
                match op {
                    b'R' => {
                        let g = self.sim.reforge(pitem, pseed);
                        let raw = if g.primal { self.sim.values_max(item, &g.affixes) } else { self.sim.values(item, seed, &g.affixes) };
                        (g.affixes, raw)
                    }
                    b'P' => {
                        let (aff, _) = self.sim.primalize(pitem, pseed);
                        let raw = self.sim.values_max(item, &aff);
                        (aff, raw)
                    }
                    _ => {
                        let g = self.sim.convert(pitem, pseed);
                        let raw = self.sim.values(item, seed, &g.affixes);
                        (g.affixes, raw)
                    }
                }
            };
            let mx = if matches!(q, Q::Primal | Q::Crafted) { None } else { Some(self.sim.values_max(item, &aff)) };
            let lines = self.make_lines(&aff, raw, mx);
            states.push(Checkpoint { name: self.d.items[item].name.clone(), quality: q.name().to_string(), lines });
        }
        self.sim.hero = self.q.class;
        states
    }

    pub fn results(&mut self) -> Results {
        let top = self.q.top.max(1);
        let pick = |v: &Vec<(u64, Hit)>| -> Vec<Hit> {
            let mut w: Vec<&(u64, Hit)> = v.iter().collect();
            w.sort_by_key(|h| (h.0, h.1.steps));
            w.into_iter().take(top).map(|h| h.1.clone()).collect()
        };
        let mut full = pick(&self.full);
        let mut near = pick(&self.near);
        let notable = pick(&self.notable);
        for h in full.iter_mut().chain(near.iter_mut()) {
            let states = self.states(h.idx);
            h.checkpoints = Self::group(&states, &h.route);
            if self.q.trail {
                h.trail = states;
            }
        }
        Results {
            status: Status { nodes: self.nodes.len() as u64, queue: self.heap.len(), cost_reached: self.cost_reached, done: self.done, capped: self.capped },
            full,
            near,
            notable,
            warnings: self.warnings.clone(),
        }
    }
}
