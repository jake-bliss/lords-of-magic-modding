//! What a unit, an army, a city or a player *is* to the engine, recovered from the operator bodies.
//!
//! `operator_bodies` answers "which objects does this operator touch". It stops one step short of
//! the thing a reimplementation needs, which is **the field layout of those objects**. The reason
//! it stops there is stated in its own documentation: the engine is C++ with singletons in `.data`,
//! an operator's body is `mov ecx,<object>` followed by a call, and the field access happens one
//! frame down through `this` where no absolute address appears at all.
//!
//! This module closes that step by joining the two halves the walk already sees separately:
//!
//! * the caller names the object — `this_call_bases` records what was in `ecx` at each call site;
//! * the callee names the offsets — `field_accesses` records every `[this+n]` the callee makes,
//!   with the displacement and the decoder's operand width.
//!
//! Neither half is an inference. The join is attribution, and its one assumption is stated where it
//! is made: that `ecx` at the call site is the callee's `this`, which is the `thiscall` convention
//! this compiler emits and which the network operators' recovered vtable slots already corroborate.
//!
//! ## Two instruments, kept apart on purpose
//!
//! **Indirect bases** — `mov ecx,[0x5ae958]` — are heap objects. Their fields have no absolute
//! address, so this join is the *only* instrument that can see them.
//!
//! **Static bases** — `mov ecx,0x5aa12c` — are objects allocated in `.data`. Their fields have an
//! absolute address as well, which means they can be seen twice: once through the join, and once as
//! a plain absolute reference in some other body. Those two readings are independent, and
//! [`Agreement`] counts where they agree. That is the strongest evidence this analysis can produce
//! about a static object's layout, because the two instruments share no mechanism.
//!
//! ## What this cannot reach
//!
//! Counted rather than waved at, in [`Coverage`]: call sites whose `ecx` the taint could not name,
//! indirect and virtual calls, and the one body the walk cannot finish. A statement of the form
//! "no operator writes field X" is only as strong as those counts.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::operator_bodies::{
    Analysis, BaseKind, BodyAnalysis, FieldAccess, GlobalAccess,
};

/// How many call levels the `this` chain is followed.
///
/// The curve is printed rather than the chosen depth being asserted, for the same reason
/// `operator_bodies` prints its import-reach curve: a number chosen to produce a pleasing answer is
/// not a measurement. See `reach_curve`.
pub const DEFAULT_DEPTH: usize = 2;

/// One field access, attributed to an object and to the operator that reached it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolvedAccess {
    pub access: FieldAccess,
    /// Call levels between the operator body and the instruction. Zero is the body itself.
    pub depth: usize,
}

/// Every field access one operator reaches, keyed so the shallowest depth wins.
pub fn resolve(
    body: &BodyAnalysis,
    bodies: &HashMap<u32, BodyAnalysis>,
    maximum_depth: usize,
) -> BTreeMap<FieldAccess, usize> {
    let mut out: BTreeMap<FieldAccess, usize> = BTreeMap::new();
    let mut record = |access: FieldAccess, depth: usize| {
        let entry = out.entry(access).or_insert(depth);
        *entry = (*entry).min(depth);
    };

    // Depth 0: what the operator's own body touches through a pointer it named itself.
    for access in &body.field_accesses {
        if access.kind != BaseKind::This {
            record(*access, 0);
        }
    }

    // Deeper: a method called on an object this operator named. The object comes from the call
    // site, the offsets from the callee's body.
    //
    // Only the callee's `[this+n]` accesses are taken. Its accesses to *other* objects are
    // deliberately dropped: they are reachability, not attribution, and folding them in is the
    // saturation that makes a depth-3 import column describe the call graph instead of the caller.
    let mut queue: Vec<(u32, crate::operator_bodies::PointerBase, usize)> = Vec::new();
    let mut seen: BTreeSet<(u32, u32, u32)> = BTreeSet::new();
    for (callee, bases) in &body.this_call_bases {
        for base in bases {
            // A dereferenced base names an object loaded *out of* the one the body named, which
            // this analysis cannot identify. Attributing a method's offsets to the container would
            // merge two structures.
            if base.kind != BaseKind::This
                && !base.dereferenced
                && seen.insert((*callee, base.address, base.offset))
            {
                queue.push((*callee, *base, 1));
            }
        }
    }
    while let Some((callee, base, depth)) = queue.pop() {
        if depth > maximum_depth {
            continue;
        }
        let Some(callee_body) = bodies.get(&callee) else {
            continue;
        };
        for access in &callee_body.field_accesses {
            if access.kind != BaseKind::This {
                continue;
            }
            // The call site may have passed an interior pointer — `lea ecx,[obj+0x4820]` — so the
            // callee's `[this+n]` is the object's `+0x4820+n`.
            let Some(offset) = base.offset.checked_add(access.offset) else {
                continue;
            };
            record(
                FieldAccess {
                    base: base.address,
                    kind: base.kind,
                    offset,
                    width: access.width,
                    write: access.write,
                    indexed: access.indexed || base.element,
                },
                depth,
            );
        }
        // A callee that passes its own `this` on in `ecx` extends the chain with the same object.
        for (next, next_bases) in &callee_body.this_call_bases {
            for forwarded in next_bases {
                if forwarded.kind != BaseKind::This || forwarded.dereferenced {
                    continue;
                }
                let Some(offset) = base.offset.checked_add(forwarded.offset) else {
                    continue;
                };
                let carried = crate::operator_bodies::PointerBase { offset, ..base };
                if seen.insert((*next, carried.address, carried.offset)) {
                    queue.push((*next, carried, depth + 1));
                }
            }
        }
    }
    out
}

/// Per-operator resolved access maps, plus the material the reports are built from.
pub struct StateModel {
    /// Operator name -> its resolved accesses, shallowest depth each.
    pub per_operator: BTreeMap<String, BTreeMap<FieldAccess, usize>>,
    /// Every address in writable data touched absolutely by some operator body, with the widths
    /// seen and whether any body wrote it.
    pub absolute: BTreeMap<u32, AbsoluteCell>,
    /// Addresses observed being materialised as an object pointer.
    pub static_bases: BTreeSet<u32>,
    pub coverage: Coverage,
    pub depth: usize,
}

/// One absolute address in writable data, as the bodies touch it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AbsoluteCell {
    pub widths: BTreeSet<u8>,
    pub written: bool,
    pub indexed: bool,
    pub operators: BTreeSet<String>,
}

/// What the instrument could not reach. The denominator for every bounded negative.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    pub operators: usize,
    /// Direct call sites in operator bodies whose `ecx` the taint could not name.
    pub untracked_calls: usize,
    /// Call sites whose `ecx` was named.
    pub tracked_calls: usize,
    /// Indirect calls in operator bodies: the destination is not known, so neither is anything it
    /// touches.
    pub indirect_calls: usize,
    /// Virtual-dispatch edges named by object and slot but never followed.
    pub virtual_calls: usize,
    /// Bodies the walk could not finish.
    pub incomplete_bodies: usize,
    /// Operators for which the join produced nothing at all.
    pub operators_without_fields: usize,
}

pub fn build(analysis: &Analysis, bodies: &HashMap<u32, BodyAnalysis>, depth: usize) -> StateModel {
    let mut per_operator = BTreeMap::new();
    let mut absolute: BTreeMap<u32, AbsoluteCell> = BTreeMap::new();
    let mut static_bases = BTreeSet::new();
    let mut coverage = Coverage {
        operators: analysis.reports.len(),
        ..Coverage::default()
    };

    for report in &analysis.reports {
        let body = &report.body;
        coverage.untracked_calls += body.untracked_calls;
        coverage.tracked_calls += body.this_call_bases.values().map(BTreeSet::len).sum::<usize>();
        coverage.indirect_calls += body.indirect_calls;
        coverage.virtual_calls += body.virtual_calls.len();
        if !body.boundary_complete() {
            coverage.incomplete_bodies += 1;
        }
        for access in &body.field_accesses {
            if access.kind == BaseKind::Static {
                static_bases.insert(access.base);
            }
        }
        for bases in body.this_call_bases.values() {
            for base in bases {
                if base.kind == BaseKind::Static {
                    static_bases.insert(base.address);
                }
            }
        }
        for global in &body.globals {
            if global.read_only {
                continue;
            }
            if global.access == GlobalAccess::Taken {
                static_bases.insert(global.address);
                continue;
            }
            let cell = absolute.entry(global.address).or_default();
            cell.written |= global.access == GlobalAccess::Write;
            cell.indexed |= global.indexed;
            cell.operators.insert(report.name.clone());
            if let Some(widths) = body.global_widths.get(&global.address) {
                cell.widths.extend(widths.iter().copied());
            }
        }

        let resolved = resolve(body, bodies, depth);
        if resolved.is_empty() {
            coverage.operators_without_fields += 1;
        }
        per_operator.insert(report.name.clone(), resolved);
    }

    StateModel {
        per_operator,
        absolute,
        static_bases,
        coverage,
        depth,
    }
}

// ---------------------------------------------------------------------------------------------
// Clustering into candidate structures
// ---------------------------------------------------------------------------------------------

/// One field of one candidate structure: an offset, the widths seen at it, and who touches it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Field {
    pub offset: u32,
    pub widths: BTreeSet<u8>,
    pub readers: BTreeSet<String>,
    pub writers: BTreeSet<String>,
    /// Whether any access used an index register, which makes the offset an array base rather than
    /// a scalar's address.
    pub indexed: bool,
    /// Shallowest call depth at which the field was seen.
    pub depth: usize,
}

impl Field {
    /// The widest access seen. A field read as a byte and as a dword is reported as both; this is
    /// for sorting and for the summary line only.
    pub fn width(&self) -> u8 {
        self.widths.iter().copied().max().unwrap_or(0)
    }
}

/// A base pointer and every field offset the operators reach through it.
///
/// **The grouping is observed**: these operators reached these offsets through this base. **The
/// reading of what the structure is, is inferred**, and `name_evidence` carries the only material
/// there is for it — the token frequencies of the operator names that converge on the base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    pub base: u32,
    pub kind: BaseKind,
    pub fields: BTreeMap<u32, Field>,
    pub operators: BTreeSet<String>,
    /// The highest offset seen plus its width, which is a **lower bound** on the object's size and
    /// never its size.
    ///
    /// A width-0 field — an address taken and never dereferenced here — still counts for one byte.
    /// It has to: the field exists at that offset, and letting the extent stop *at* it makes the
    /// structure's own last field fall outside the range the two-instrument comparison searches.
    pub observed_extent: u32,
}

impl Structure {
    /// Word-fragment frequencies across the operator names that reach this base.
    ///
    /// This is the material behind any name a reader might give the structure, presented as counts
    /// so the naming stays **inferred** and stays the reader's. Fragments are matched as
    /// substrings because operator names are unseparated — `getcitydata`, `nsetcitydata`.
    pub fn name_evidence(&self, vocabulary: &[&str]) -> Vec<(String, usize)> {
        let mut counts: Vec<(String, usize)> = vocabulary
            .iter()
            .map(|token| {
                (
                    (*token).to_owned(),
                    self.operators
                        .iter()
                        .filter(|name| name.contains(token))
                        .count(),
                )
            })
            .filter(|(_, count)| *count > 0)
            .collect();
        counts.sort_by_key(|(token, count)| (std::cmp::Reverse(*count), token.clone()));
        counts
    }
}

/// Subject words counted in operator names. Chosen from the engine's own vocabulary — every one of
/// these is a substring of at least one registered operator name — and not from a guess about what
/// the structures are.
pub const SUBJECT_VOCABULARY: [&str; 18] = [
    "army", "unit", "city", "player", "building", "artifact", "spell", "map", "terrain", "combat",
    "faith", "region", "sprite", "dialog", "imp", "net", "sound", "save",
];

pub fn structures(model: &StateModel) -> Vec<Structure> {
    let mut by_base: BTreeMap<(u32, BaseKind), Structure> = BTreeMap::new();
    for (operator, accesses) in &model.per_operator {
        for (access, depth) in accesses {
            let structure = by_base
                .entry((access.base, access.kind))
                .or_insert_with(|| Structure {
                    base: access.base,
                    kind: access.kind,
                    fields: BTreeMap::new(),
                    operators: BTreeSet::new(),
                    observed_extent: 0,
                });
            structure.operators.insert(operator.clone());
            structure.observed_extent = structure
                .observed_extent
                .max(access.offset + u32::from(access.width).max(1));
            let field = structure.fields.entry(access.offset).or_insert(Field {
                offset: access.offset,
                depth: *depth,
                ..Field::default()
            });
            field.widths.insert(access.width);
            field.indexed |= access.indexed;
            field.depth = field.depth.min(*depth);
            if access.write {
                field.writers.insert(operator.clone());
            } else {
                field.readers.insert(operator.clone());
            }
        }
    }
    let mut out: Vec<Structure> = by_base.into_values().collect();
    out.sort_by_key(|structure| {
        (
            std::cmp::Reverse(structure.operators.len()),
            structure.base,
        )
    });
    out
}

// ---------------------------------------------------------------------------------------------
// The independent cross-check on static objects
// ---------------------------------------------------------------------------------------------

/// For one statically allocated object, how the two instruments compare.
///
/// A field of a static object can be reached two ways that share no mechanism: through the `this`
/// join, and as a plain absolute address in some other body. Where both see the same offset the
/// evidence is as strong as this analysis gets. Where only one sees it, that is not a
/// contradiction — it is one instrument being blind — and the counts say which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agreement {
    pub base: u32,
    /// Where this comparison stops looking: `base + observed_extent`, the join's own lower bound on
    /// the object's size.
    ///
    /// The first version of this used "the next statically materialised base above it", which is
    /// wrong for the reason the numbers made obvious: C++ code takes the address of an object's
    /// *members*, so `0x005aa1dc` is materialised as a base while sitting 0xb0 bytes inside the
    /// object at `0x005aa12c`. That rule truncated a structure with fields out to `+0x6ccc` at
    /// `+0xb0` and threw away most of the comparison.
    pub territory_end: u32,
    /// Other statically materialised bases inside this range. Each is either a member whose address
    /// is taken, or a genuinely separate object this range has swallowed — the comparison cannot
    /// tell which, and `absolute_only` is inflated by however many of them are the second kind.
    pub interior_bases: usize,
    pub both: BTreeSet<u32>,
    pub join_only: BTreeSet<u32>,
    pub absolute_only: BTreeSet<u32>,
}

/// Compare the `this`-join field map against the absolute-address field map, per static base.
///
/// The territory bound is **inferred**, and it is the weakest link in this comparison: an absolute
/// address inside `[base, base + observed_extent)` is attributed to this object. `interior_bases`
/// reports how many other materialised bases the range contains, which is the measure of how much
/// that attribution could be wrong by.
pub fn agreement(model: &StateModel, structures: &[Structure]) -> Vec<Agreement> {
    let mut out = Vec::new();
    for structure in structures {
        if structure.kind != BaseKind::Static {
            continue;
        }
        let territory_end = structure.base.saturating_add(structure.observed_extent);
        let interior_bases = model
            .static_bases
            .range(structure.base.saturating_add(1)..territory_end)
            .count();
        let join: BTreeSet<u32> = structure.fields.keys().copied().collect();
        let absolute: BTreeSet<u32> = model
            .absolute
            .range(structure.base..territory_end)
            .map(|(address, _)| address - structure.base)
            .collect();
        out.push(Agreement {
            base: structure.base,
            territory_end,
            interior_bases,
            both: join.intersection(&absolute).copied().collect(),
            join_only: join.difference(&absolute).copied().collect(),
            absolute_only: absolute.difference(&join).copied().collect(),
        });
    }
    out.sort_by_key(|row| std::cmp::Reverse(row.both.len()));
    out
}

// ---------------------------------------------------------------------------------------------
// The saturation curve
// ---------------------------------------------------------------------------------------------

/// One row of the depth sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachRow {
    pub depth: usize,
    pub operators_with_fields: usize,
    pub distinct_bases: usize,
    pub distinct_fields: usize,
    pub accesses: usize,
}

/// How the join grows with depth, so the chosen depth can be judged rather than trusted.
///
/// The same argument `operator_bodies` uses for import depth applies here: a join that reaches
/// almost every object from almost every operator is describing the call graph, not the operator.
pub fn reach_curve(
    analysis: &Analysis,
    bodies: &HashMap<u32, BodyAnalysis>,
    depths: &[usize],
) -> Vec<ReachRow> {
    depths
        .iter()
        .map(|depth| {
            let mut operators_with_fields = 0;
            let mut distinct_bases = BTreeSet::new();
            let mut distinct_fields = BTreeSet::new();
            let mut accesses = 0;
            for report in &analysis.reports {
                let resolved = resolve(&report.body, bodies, *depth);
                if !resolved.is_empty() {
                    operators_with_fields += 1;
                }
                accesses += resolved.len();
                for access in resolved.keys() {
                    distinct_bases.insert((access.base, access.kind));
                    distinct_fields.insert((access.base, access.kind, access.offset));
                }
            }
            ReachRow {
                depth: *depth,
                operators_with_fields,
                distinct_bases: distinct_bases.len(),
                distinct_fields: distinct_fields.len(),
                accesses,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------------------------

/// The per-operator access map: one row per operator and field it reaches.
pub fn access_table(model: &StateModel) -> String {
    let mut text = String::from(
        "operator\tbase\tbase_kind\toffset\twidth\taccess\tindexed\tdepth\n",
    );
    for (operator, accesses) in &model.per_operator {
        for (access, depth) in accesses {
            text.push_str(&format!(
                "{operator}\t{:#010x}\t{}\t{:#x}\t{}\t{}\t{}\t{depth}\n",
                access.base,
                access.kind.label(),
                access.offset,
                access.width,
                if access.write { "write" } else { "read" },
                if access.indexed { "indexed" } else { "scalar" },
            ));
        }
    }
    text
}

/// One row per candidate structure.
pub fn structure_table(structures: &[Structure]) -> String {
    let mut text = String::from(
        "base\tbase_kind\toperators\tfields\twritten_fields\tobserved_extent\tname_evidence\toperator_names\n",
    );
    for structure in structures {
        let written = structure
            .fields
            .values()
            .filter(|field| !field.writers.is_empty())
            .count();
        let evidence: Vec<String> = structure
            .name_evidence(&SUBJECT_VOCABULARY)
            .into_iter()
            .take(6)
            .map(|(token, count)| format!("{token}:{count}"))
            .collect();
        text.push_str(&format!(
            "{:#010x}\t{}\t{}\t{}\t{written}\t{:#x}\t{}\t{}\n",
            structure.base,
            structure.kind.label(),
            structure.operators.len(),
            structure.fields.len(),
            structure.observed_extent,
            if evidence.is_empty() {
                "-".to_owned()
            } else {
                evidence.join(",")
            },
            structure
                .operators
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
    text
}

/// One row per recovered field of every candidate structure.
pub fn field_table(structures: &[Structure]) -> String {
    let mut text = String::from(
        "base\tbase_kind\toffset\twidths\taccess\tindexed\tdepth\treaders\twriters\twriter_names\n",
    );
    for structure in structures {
        for field in structure.fields.values() {
            let widths: Vec<String> = field.widths.iter().map(u8::to_string).collect();
            let access = match (field.readers.is_empty(), field.writers.is_empty()) {
                (false, false) => "read-write",
                (true, false) => "write",
                (false, true) => "read",
                (true, true) => "none",
            };
            let mut writers: Vec<String> = field.writers.iter().cloned().collect();
            writers.truncate(24);
            text.push_str(&format!(
                "{:#010x}\t{}\t{:#x}\t{}\t{access}\t{}\t{}\t{}\t{}\t{}\n",
                structure.base,
                structure.kind.label(),
                field.offset,
                widths.join("|"),
                if field.indexed { "indexed" } else { "scalar" },
                field.depth,
                field.readers.len(),
                field.writers.len(),
                if writers.is_empty() {
                    "-".to_owned()
                } else {
                    writers.join(",")
                },
            ));
        }
    }
    text
}

/// One row per static base, comparing the two instruments.
pub fn agreement_table(rows: &[Agreement]) -> String {
    let mut text = String::from(
        "base\tterritory_end\tinterior_bases\tboth\tjoin_only\tabsolute_only\tagreed_offsets\n",
    );
    for row in rows {
        let mut agreed: Vec<String> = row
            .both
            .iter()
            .map(|offset| format!("{offset:#x}"))
            .collect();
        agreed.truncate(32);
        text.push_str(&format!(
            "{:#010x}\t{:#010x}\t{}\t{}\t{}\t{}\t{}\n",
            row.base,
            row.territory_end,
            row.interior_bases,
            row.both.len(),
            row.join_only.len(),
            row.absolute_only.len(),
            if agreed.is_empty() {
                "-".to_owned()
            } else {
                agreed.join(",")
            },
        ));
    }
    text
}

/// Operators whose names contain a subject word, grouped by the base they converge on.
///
/// The convergence is **observed**; reading it as "this base is the unit record" is **inferred**,
/// and the ratio is what a reader needs to judge it: 40 of 41 `unit`-named operators on one base is
/// a different claim from 40 of 400.
pub fn subject_convergence(
    model: &StateModel,
    structures: &[Structure],
    minimum_operators: usize,
) -> String {
    let mut text = String::from("subject\toperators_with_word\tbase\tbase_kind\tmatching\tshare\n");
    for subject in SUBJECT_VOCABULARY {
        let total = model
            .per_operator
            .keys()
            .filter(|name| name.contains(subject))
            .count();
        if total == 0 {
            continue;
        }
        for structure in structures {
            if structure.operators.len() < minimum_operators {
                continue;
            }
            let matching = structure
                .operators
                .iter()
                .filter(|name| name.contains(subject))
                .count();
            if matching == 0 {
                continue;
            }
            text.push_str(&format!(
                "{subject}\t{total}\t{:#010x}\t{}\t{matching}\t{:.0}%\n",
                structure.base,
                structure.kind.label(),
                100.0 * matching as f64 / total as f64,
            ));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operator_bodies::PointerBase;

    fn body() -> BodyAnalysis {
        BodyAnalysis {
            entry_point: 0x1000,
            instructions: 1,
            decoded_extent_end: 0x1004,
            globals: Vec::new(),
            calls: BTreeSet::new(),
            tail_calls: BTreeSet::new(),
            direct_imports: BTreeSet::new(),
            writes_through_pointer: BTreeSet::new(),
            writes_through_this: false,
            field_accesses: BTreeSet::new(),
            global_widths: BTreeMap::new(),
            this_call_bases: BTreeMap::new(),
            untracked_calls: 0,
            string_refs: BTreeSet::new(),
            inline_pops: 0,
            inline_pushes: 0,
            helper_pops: 0,
            helper_pushes: 0,
            arity: None,
            arity_candidates: BTreeSet::new(),
            operand_loop_carried: false,
            operand_state_cap_hit: false,
            returns_arguments: 0,
            indirect_calls: 0,
            virtual_calls: BTreeSet::new(),
            unresolved_indirect_jumps: 0,
            resolved_jump_tables: 0,
            invalid_instructions: 0,
            truncated: false,
            floating_point: false,
            returns: 1,
        }
    }

    fn this_access(offset: u32, width: u8, write: bool) -> FieldAccess {
        FieldAccess {
            base: u32::MAX,
            kind: BaseKind::This,
            offset,
            width,
            write,
            indexed: false,
        }
    }

    #[test]
    fn a_callee_offset_is_attributed_to_the_object_the_caller_named() {
        let mut caller = body();
        caller.this_call_bases.insert(
            0x2000,
            BTreeSet::from([PointerBase {
                address: 0x005a_a12c,
                kind: BaseKind::Static,
                offset: 0,
                dereferenced: false,
                element: false,
            }]),
        );
        let mut callee = body();
        callee.field_accesses.insert(this_access(0x520, 4, true));
        let bodies = HashMap::from([(0x2000, callee)]);

        let resolved = resolve(&caller, &bodies, 2);
        let (access, depth) = resolved.iter().next().expect("one access");
        assert_eq!(access.base, 0x005a_a12c);
        assert_eq!(access.kind, BaseKind::Static);
        assert_eq!(access.offset, 0x520);
        assert!(access.write);
        assert_eq!(*depth, 1);
    }

    #[test]
    fn a_callee_reached_with_an_unnamed_this_contributes_nothing() {
        // The failure this guards is attributing a callee's offsets to whatever object happened to
        // be nearby. Only a call site that named its object may contribute.
        let mut caller = body();
        caller.this_call_bases.insert(
            0x2000,
            BTreeSet::from([PointerBase {
                address: u32::MAX,
                kind: BaseKind::This,
                offset: 0,
                dereferenced: false,
                element: false,
            }]),
        );
        let mut callee = body();
        callee.field_accesses.insert(this_access(0x10, 4, false));
        let bodies = HashMap::from([(0x2000, callee)]);

        assert!(resolve(&caller, &bodies, 3).is_empty());
    }

    #[test]
    fn the_chain_continues_only_while_this_is_forwarded_and_only_to_the_depth_asked_for() {
        let mut caller = body();
        caller.this_call_bases.insert(
            0x2000,
            BTreeSet::from([PointerBase {
                address: 0x005a_e958,
                kind: BaseKind::Indirect,
                offset: 0,
                dereferenced: false,
                element: false,
            }]),
        );
        let mut middle = body();
        middle.field_accesses.insert(this_access(0x4, 4, false));
        middle.this_call_bases.insert(
            0x3000,
            BTreeSet::from([PointerBase {
                address: u32::MAX,
                kind: BaseKind::This,
                offset: 0,
                dereferenced: false,
                element: false,
            }]),
        );
        let mut deep = body();
        deep.field_accesses.insert(this_access(0x8, 2, true));
        let bodies = HashMap::from([(0x2000, middle), (0x3000, deep)]);

        let one = resolve(&caller, &bodies, 1);
        assert_eq!(one.len(), 1, "depth 1 sees only the directly called method");
        let two = resolve(&caller, &bodies, 2);
        assert_eq!(two.len(), 2, "depth 2 reaches the forwarded call");
        assert_eq!(
            two.values().copied().max(),
            Some(2),
            "and records how far away it was"
        );
    }

    #[test]
    fn a_callees_access_to_another_object_is_not_attributed_to_the_callers() {
        // Reachability is not attribution. A method on object A that also pokes object B must not
        // make B's offsets into A's fields.
        let mut caller = body();
        caller.this_call_bases.insert(
            0x2000,
            BTreeSet::from([PointerBase {
                address: 0x005a_a12c,
                kind: BaseKind::Static,
                offset: 0,
                dereferenced: false,
                element: false,
            }]),
        );
        let mut callee = body();
        callee.field_accesses.insert(FieldAccess {
            base: 0x005a_e958,
            kind: BaseKind::Indirect,
            offset: 0x30,
            width: 4,
            write: true,
            indexed: false,
        });
        let bodies = HashMap::from([(0x2000, callee)]);

        assert!(resolve(&caller, &bodies, 3).is_empty());
    }

    #[test]
    fn a_this_forwarding_cycle_terminates() {
        let mut caller = body();
        caller.this_call_bases.insert(
            0x2000,
            BTreeSet::from([PointerBase {
                address: 0x1234_5678,
                kind: BaseKind::Indirect,
                offset: 0,
                dereferenced: false,
                element: false,
            }]),
        );
        let mut looping = body();
        looping.field_accesses.insert(this_access(0, 4, false));
        looping.this_call_bases.insert(
            0x2000,
            BTreeSet::from([PointerBase {
                address: u32::MAX,
                kind: BaseKind::This,
                offset: 0,
                dereferenced: false,
                element: false,
            }]),
        );
        let bodies = HashMap::from([(0x2000, looping)]);

        assert_eq!(resolve(&caller, &bodies, 50).len(), 1);
    }

    #[test]
    fn name_evidence_counts_substrings_and_never_invents_a_name() {
        let structure = Structure {
            base: 0x1000,
            kind: BaseKind::Indirect,
            fields: BTreeMap::new(),
            operators: BTreeSet::from([
                "getcitydata".to_owned(),
                "nsetcitydata".to_owned(),
                "armyat".to_owned(),
            ]),
            observed_extent: 0,
        };
        let evidence = structure.name_evidence(&SUBJECT_VOCABULARY);
        assert_eq!(evidence.first(), Some(&("city".to_owned(), 2)));
        assert!(evidence.iter().any(|(token, count)| token == "army" && *count == 1));
        assert!(
            evidence.iter().all(|(_, count)| *count > 0),
            "a token no operator carries is absent, not reported as zero"
        );
    }

    #[test]
    fn the_extent_is_a_lower_bound_that_includes_the_width_of_the_last_field() {
        let mut model = StateModel {
            per_operator: BTreeMap::new(),
            absolute: BTreeMap::new(),
            static_bases: BTreeSet::new(),
            coverage: Coverage::default(),
            depth: 1,
        };
        model.per_operator.insert(
            "probe".to_owned(),
            BTreeMap::from([(
                FieldAccess {
                    base: 0x4000,
                    kind: BaseKind::Indirect,
                    offset: 0x50ac,
                    width: 1,
                    write: false,
                    indexed: false,
                },
                1,
            )]),
        );
        let structures = structures(&model);
        assert_eq!(structures[0].observed_extent, 0x50ad);
    }
}
